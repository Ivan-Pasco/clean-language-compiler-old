//! MIR to WebAssembly Code Generation
//!
//! This module implements code generation from MIR (Medium-level Intermediate Representation)
//! to WebAssembly bytecode. It provides a cleaner, more optimized path from typed code
//! to WASM compared to the direct AST-to-WASM generation.

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::mir::mir_types::{
    AnyTypeTag, BasicBlockId, MirBasicBlock, MirBinaryOp, MirConstant, MirFunction, MirInstruction,
    MirOperand, MirOperation, MirProgram, MirTerminator, MirType, MirUnaryOp, ValueId,
};
use crate::resolver::SymbolId;
use std::collections::{HashMap, HashSet};
use wasm_encoder::{BlockType, Function as WasmFunction, Instruction, ValType};

// Conditional debug macro for MIR code generation using tracing
macro_rules! debug_mir {
    ($($arg:tt)*) => {
        tracing::trace!($($arg)*)
    };
}

/// MIR to WASM code generator
pub struct MirCodeGenerator<'a> {
    /// The underlying WASM code generator
    wasm_generator: CodeGenerator,

    /// Mapping from MIR ValueId to WASM local indices
    value_to_local: HashMap<ValueId, u32>,

    /// Mapping from MIR BasicBlockId to WASM block indices
    block_labels: HashMap<BasicBlockId, u32>,

    /// Current WASM local index counter
    next_local_index: u32,

    /// Current WASM block label counter
    next_block_label: u32,

    /// Stack of WASM instructions for current function
    current_instructions: Vec<Instruction<'a>>,

    /// Current function being generated
    current_function: Option<MirFunction>,

    /// String pool from MIR program for string constant handling
    string_pool: Option<Vec<String>>,

    /// Mapping from ValueId to string pool index (for string constants loaded as locals)
    value_to_string_index: HashMap<ValueId, usize>,

    /// CRITICAL FIX: Mapping from SymbolId to function name for proper function resolution
    function_symbol_map: HashMap<SymbolId, String>,

    /// CRITICAL FIX: Direct mapping from SymbolId to WASM function index
    /// This avoids name collisions for constructors/methods with same names
    symbol_to_function_index: HashMap<SymbolId, u32>,

    /// Function signature map for proper parameter/return handling
    function_signatures: HashMap<SymbolId, MirFunction>,

    /// Type tracking for values (needed to expand string pointers)
    value_to_type: HashMap<ValueId, MirType>,

    /// CRITICAL FIX: Track types of temporary locals created during codegen
    /// Maps local index -> WASM type for temporaries (e.g., string expansion temps)
    temp_local_types: HashMap<u32, ValType>,

    /// Plugin bridge functions to register as WASM imports
    /// These come from plugin.toml [bridge] sections
    bridge_functions: Vec<crate::plugins::BridgeFunction>,

    /// CRITICAL FIX: Pending wrapper functions to be registered AFTER ALL imports are done
    /// These are for expand_strings bridge functions that need wrapper functions
    /// to convert Clean Language strings (ptr) to raw format (ptr+4, len)
    pending_bridge_wrappers: Vec<PendingBridgeWrapper>,
}

/// Info for pending bridge wrapper functions that need to be registered
/// AFTER all imports are done to avoid function index collisions
#[derive(Clone)]
struct PendingBridgeWrapper {
    name: String,
    params: Vec<crate::types::WasmType>,
    wasm_return: Option<crate::types::WasmType>,
    raw_func_index: u32,
    param_types: Vec<crate::builtins::registry::BuiltinType>,
}

/// Result of MIR code generation
#[derive(Debug)]
pub struct MirCodegenResult {
    /// Generated WASM bytecode
    pub wasm_bytes: Vec<u8>,

    /// Generation statistics
    pub stats: MirCodegenStats,

    /// Warnings during generation
    pub warnings: Vec<CompilerError>,
}

/// Statistics about MIR code generation
#[derive(Debug, Default)]
pub struct MirCodegenStats {
    /// Number of functions generated
    pub functions_generated: usize,

    /// Number of basic blocks generated
    pub blocks_generated: usize,

    /// Number of instructions generated
    pub instructions_generated: usize,

    /// Generation time in microseconds
    pub generation_time_us: u64,
}

impl MirCodeGenerator<'_> {
    /// Create a new MIR code generator
    pub fn new() -> Self {
        Self {
            wasm_generator: CodeGenerator::new(),
            value_to_local: HashMap::new(),
            block_labels: HashMap::new(),
            next_local_index: 0,
            next_block_label: 0,
            current_instructions: Vec::new(),
            current_function: None,
            string_pool: None,
            value_to_string_index: HashMap::new(),
            function_symbol_map: HashMap::new(),
            symbol_to_function_index: HashMap::new(),
            function_signatures: HashMap::new(),
            value_to_type: HashMap::new(),
            temp_local_types: HashMap::new(),
            bridge_functions: Vec::new(),
            pending_bridge_wrappers: Vec::new(),
        }
    }

    /// Create a new MIR code generator for testing (without runtime imports)
    pub fn new_minimal() -> Self {
        Self {
            wasm_generator: CodeGenerator::new_minimal(),
            value_to_local: HashMap::new(),
            block_labels: HashMap::new(),
            next_local_index: 0,
            next_block_label: 0,
            current_instructions: Vec::new(),
            current_function: None,
            string_pool: None,
            value_to_string_index: HashMap::new(),
            function_symbol_map: HashMap::new(),
            symbol_to_function_index: HashMap::new(),
            function_signatures: HashMap::new(),
            value_to_type: HashMap::new(),
            temp_local_types: HashMap::new(),
            bridge_functions: Vec::new(),
            pending_bridge_wrappers: Vec::new(),
        }
    }

    /// Set plugin bridge functions to be registered as WASM imports
    ///
    /// Bridge functions are declared in plugin.toml [bridge] sections and
    /// need to be registered as WASM imports before code generation.
    pub fn set_bridge_functions(&mut self, bridge_functions: Vec<crate::plugins::BridgeFunction>) {
        self.bridge_functions = bridge_functions;
    }

    /// Generate WASM from MIR program
    pub fn generate(
        &mut self,
        mir_program: MirProgram,
    ) -> Result<MirCodegenResult, Vec<CompilerError>> {
        tracing::debug!(
            functions = mir_program.functions.len(),
            "MirCodeGenerator::generate called"
        );
        for (symbol_id, function) in &mir_program.functions {
            tracing::debug!(
                symbol_id = symbol_id.0,
                name = %function.name,
                blocks = function.blocks.len(),
                "Function basic blocks"
            );
        }

        let start_time = std::time::Instant::now();
        let mut stats = MirCodegenStats::default();
        let warnings = Vec::new();

        // CRITICAL FIX: Set up the underlying WASM generator with runtime imports
        if self.wasm_generator.include_runtime_imports {
            self.wasm_generator
                .register_print_imports()
                .map_err(|e| vec![e])?;

            // CRITICAL: Register console input function imports (input, input_integer, input_float, etc.)
            debug_mir!("DEBUG MIR: Registering console input imports");
            self.wasm_generator
                .register_console_imports()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: Console input imports registered");

            // CRITICAL: Register type conversion imports for .toString() methods
            debug_mir!("DEBUG MIR: Registering type conversion imports (int_to_string, etc.)");
            self.wasm_generator
                .register_type_conversion_imports()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: Type conversion imports registered");

            // CRITICAL: Register HTTP imports (http_get, http_post, etc.)
            // Collect function names that plugins will handle with expand_strings wrappers
            // These need to be skipped here since the plugin will register them with __raw suffix
            let skip_http_functions: HashSet<String> = self
                .bridge_functions
                .iter()
                .filter(|f| f.expand_strings && f.params.iter().any(|p| p == "string"))
                .map(|f| f.name.clone())
                .collect();

            // Only include HTTP server imports if web framework plugin is loaded
            // (detected by presence of _http_route or similar bridge functions)
            let include_server_imports = self
                .bridge_functions
                .iter()
                .any(|f| f.name.starts_with("_http_") || f.name.starts_with("_req_"));

            debug_mir!(
                "DEBUG MIR: Registering HTTP imports (skipping {} for plugin expand_strings, server_imports={})",
                skip_http_functions.len(),
                include_server_imports
            );
            self.wasm_generator
                .register_http_imports(&skip_http_functions, include_server_imports)
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: HTTP imports registered");

            // CRITICAL: Register file imports (file_read, file_write, file_exists, etc.)
            debug_mir!("DEBUG MIR: Registering file imports");
            self.wasm_generator
                .register_file_imports()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: File imports registered");

            // CRITICAL: Register string.split as an import BEFORE any stdlib functions
            // This must happen before math_operations which adds internal functions
            // because imports must all be registered before internal functions start
            debug_mir!("DEBUG MIR: Registering string.split import");
            self.wasm_generator
                .register_string_split_import()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: string.split import registered");

            // CRITICAL: Register plugin bridge functions as WASM imports BEFORE any internal functions
            // These are declared in plugin.toml [bridge] sections
            // MUST be registered here because WASM requires all imports before internal functions
            if !self.bridge_functions.is_empty() {
                debug_mir!(
                    "DEBUG MIR: Registering {} plugin bridge function imports",
                    self.bridge_functions.len()
                );
                self.register_plugin_bridge_imports().map_err(|e| vec![e])?;
                debug_mir!("DEBUG MIR: Plugin bridge function imports registered");
            }

            // CRITICAL: Register math operations (abs, max, min, sqrt, pow, etc.)
            debug_mir!("DEBUG MIR: Registering math operation imports");
            self.wasm_generator
                .register_math_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: Math operation imports registered");

            // CRITICAL: Register string class operations (toUpperCase, toLowerCase, etc.)
            debug_mir!("DEBUG MIR: Registering string class operations");
            self.wasm_generator
                .register_string_class_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: String class operations registered");

            // Register list class operations (size, push, pop, get)
            debug_mir!("DEBUG MIR: Registering list class operations");
            self.wasm_generator
                .register_list_class_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: List class operations registered");

            // CRITICAL: Register conditional operations (compare.integer.*, logical.*, etc.)
            debug_mir!("DEBUG MIR: Registering conditional operations");
            self.wasm_generator
                .register_conditional_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: Conditional operations registered");

            // CRITICAL: Register HTTP operations (http.get, http.post, etc.)
            debug_mir!("DEBUG MIR: Registering HTTP operations");
            self.wasm_generator
                .register_http_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: HTTP operations registered");

            // CRITICAL: Register file operations (file.read, file.write, file.exists, etc.)
            debug_mir!("DEBUG MIR: Registering file operations");
            self.wasm_generator
                .register_file_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: File operations registered");

            // CRITICAL: Register validator operations (validator.create, validator.ok, etc.)
            debug_mir!("DEBUG MIR: Registering validator operations");
            self.wasm_generator
                .register_validator_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: Validator operations registered");

            // NATIVE: Register memory operations (malloc, memcpy, string_last_index_of, etc.) for standalone WASM execution
            // NOTE: This also registers string operations (indexOf, lastIndexOf, contains, etc.)
            debug_mir!("DEBUG MIR: Registering native memory operations (includes string ops)");
            self.wasm_generator
                .register_memory_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: Native memory operations registered");

            // CRITICAL: Register native list operations (length, get, set, push, pop, etc.)
            debug_mir!("DEBUG MIR: Registering native list operations");
            self.wasm_generator
                .register_list_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: Native list operations registered");

            // BOOK: json-module - Register JSON operations (json.textToData, json.dataToText, etc.)
            debug_mir!("DEBUG MIR: Registering JSON operations");
            self.wasm_generator
                .register_json_operations()
                .map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: JSON operations registered");

            // CRITICAL FIX: Register pending plugin bridge wrapper functions AFTER all imports
            // These are internal WASM functions that wrap raw imports with expand_strings=true
            // They MUST be registered after ALL imports to avoid function index collisions
            if !self.pending_bridge_wrappers.is_empty() {
                debug_mir!(
                    "DEBUG MIR: Registering {} pending bridge wrapper functions",
                    self.pending_bridge_wrappers.len()
                );
                self.register_pending_bridge_wrappers()
                    .map_err(|e| vec![e])?;
                debug_mir!("DEBUG MIR: Bridge wrapper functions registered");
            }

            // CRITICAL FIX: Register HTTP server wrapper functions for string expansion
            // _http_route and _req_param need wrappers to expand string pointers to (ptr+4, len) pairs
            // This MUST be called after all imports are registered
            debug_mir!("DEBUG MIR: Registering HTTP server wrapper functions");
            self.register_http_server_wrappers().map_err(|e| vec![e])?;
            debug_mir!("DEBUG MIR: HTTP server wrapper functions registered");
        }

        // Set up memory section
        self.setup_memory_section().map_err(|e| vec![e])?;

        // CRITICAL FIX: Transfer string pool to WASM module BEFORE function generation
        // Functions need access to string pool during code generation
        self.setup_string_pool(&mir_program.string_pool)
            .map_err(|e| vec![e])?;

        // CRITICAL FIX: Build function symbol mapping for proper function resolution
        // This allows us to map SymbolId to function name during function calls
        for (symbol_id, function) in &mir_program.functions {
            self.function_symbol_map
                .insert(*symbol_id, function.name.clone());
            // Also store full function signature for parameter/return type handling
            self.function_signatures
                .insert(*symbol_id, function.clone());
            tracing::debug!(
                symbol_id = symbol_id.0,
                name = %function.name,
                "Mapped SymbolId to function name"
            );
        }

        // CRITICAL FIX: Register builtin function signatures
        // Builtin functions need signatures in the HashMap for proper return type handling
        self.register_builtin_function_signatures();

        // CRITICAL FIX: Pre-register ALL functions in function_map BEFORE generating code
        // This ensures that when function A calls function B, function B is already in the map
        // even if B hasn't been generated yet
        // IMPORTANT: Sort functions by SymbolId to ensure deterministic ordering
        debug_mir!("Starting function pre-registration");
        debug_mir!(
            function_count = self.wasm_generator.function_count,
            mir_functions = mir_program.functions.len(),
            "Pre-registration state"
        );

        // CRITICAL FIX: Initialize function_symbol_map from MirProgram's symbol_name_map
        // This includes ALL functions: builtins (print, math.*, etc.) AND user-defined
        debug_mir!(
            symbol_map_entries = mir_program.symbol_name_map.len(),
            "Initializing function_symbol_map from MirProgram"
        );
        for (symbol_id, name) in &mir_program.symbol_name_map {
            self.function_symbol_map.insert(*symbol_id, name.clone());
            debug_mir!(symbol_id = symbol_id.0, name = %name, "Symbol init");

            // CRITICAL FIX: Also populate symbol_to_function_index for builtin functions
            // Builtin functions are already registered in wasm_generator.function_map (name -> index)
            // but their SymbolIds were never mapped to their WASM indices
            if let Some(&wasm_index) = self.wasm_generator.function_map.get(name) {
                self.symbol_to_function_index.insert(*symbol_id, wasm_index);
                debug_mir!(
                    symbol_id = symbol_id.0,
                    name = %name,
                    wasm_index = wasm_index,
                    "Builtin function mapped"
                );
            }
        }

        // Collect and sort functions by SymbolId for deterministic ordering
        let mut sorted_functions: Vec<_> = mir_program.functions.into_iter().collect();
        sorted_functions.sort_by_key(|(symbol_id, _)| symbol_id.0);

        for (i, (symbol_id, function)) in sorted_functions.iter().enumerate() {
            let function_index = self.wasm_generator.function_count + i as u32;
            debug_mir!(
                function_name = %function.name,
                function_index = function_index,
                i = i,
                function_count = self.wasm_generator.function_count,
                "Pre-registering function"
            );
            self.wasm_generator
                .function_map
                .insert(function.name.clone(), function_index);

            // CRITICAL FIX: Populate function_symbol_map for base() call resolution
            // Map SymbolId -> function name so get_function_name_by_symbol can resolve user-defined functions
            self.function_symbol_map
                .insert(*symbol_id, function.name.clone());

            // CRITICAL FIX: Direct SymbolId -> WASM index mapping
            // Avoids name collisions for constructors/methods with same names
            self.symbol_to_function_index
                .insert(*symbol_id, function_index);

            debug_mir!(
                symbol_id = symbol_id.0,
                function_name = %function.name,
                wasm_index = function_index,
                "Symbol map entry inserted"
            );
        }
        debug_mir!(
            functions_registered = sorted_functions.len(),
            "All functions pre-registered in function_map"
        );
        debug_mir!(
            total_entries = self.function_symbol_map.len(),
            "Function symbol map complete"
        );

        // Generate all functions in the same sorted order
        for (_symbol_id, function) in sorted_functions {
            let func_name = function.name.clone();
            let func_index = self.wasm_generator.function_count + stats.functions_generated as u32;
            debug_mir!(
                function_name = %func_name,
                func_index = func_index,
                "Generating function"
            );
            match self.generate_function(function) {
                Ok(function_stats) => {
                    debug_mir!(
                        function_name = %func_name,
                        func_index = func_index,
                        "Successfully generated function"
                    );
                    stats.functions_generated += 1;
                    stats.blocks_generated += function_stats.blocks_generated;
                    stats.instructions_generated += function_stats.instructions_generated;
                }
                Err(error) => {
                    tracing::error!(
                        function_name = %func_name,
                        error = ?error,
                        "ERROR generating function"
                    );
                    // CRITICAL FIX: Function generation failures must be hard errors
                    // If we allow them as warnings, we get phantom function indices that don't exist in WASM
                    // This causes "function variable out of range" errors when calling pre-registered but failed functions
                    return Err(vec![error]);
                }
            }
        }

        // CRITICAL FIX: Update function_count to reflect all generated functions
        // This must happen AFTER generation because we used pre-registered indices
        self.wasm_generator.function_count += stats.functions_generated as u32;
        tracing::debug!(
            functions_generated = stats.functions_generated,
            new_function_count = self.wasm_generator.function_count,
            "Updated function_count after generation"
        );

        // CRITICAL FIX: Handle entry point if it exists
        if let Some(entry_symbol_id) = mir_program.entry_point {
            self.generate_start_function_export(entry_symbol_id)
                .map_err(|e| vec![e])?;
        }

        // Finalize WASM module
        let wasm_bytes = self.finalize_module().map_err(|e| vec![e])?;

        stats.generation_time_us = start_time.elapsed().as_micros() as u64;

        Ok(MirCodegenResult {
            wasm_bytes,
            stats,
            warnings,
        })
    }

    /// Generate WASM function from MIR function
    fn generate_function(&mut self, function: MirFunction) -> Result<FunctionStats, CompilerError> {
        tracing::debug!(
            name = %function.name,
            blocks = function.blocks.len(),
            "Starting generate_function"
        );
        let mut stats = FunctionStats::default();

        // Reset per-function state
        self.value_to_local.clear();
        self.block_labels.clear();
        self.value_to_string_index.clear();
        self.value_to_type.clear();
        self.temp_local_types.clear();
        self.next_local_index = 0;
        self.next_block_label = 0;
        self.current_instructions.clear();
        self.current_function = Some(function.clone());

        // Populate value_to_type from function parameters
        debug_mir!(
            function_name = %function.name,
            "Populating value_to_type for function"
        );
        for param in &function.parameters {
            debug_mir!(
                value_id = param.value_id.0,
                param_type = ?param.param_type,
                "Parameter type mapping"
            );
            self.value_to_type
                .insert(param.value_id, param.param_type.clone());
        }

        // Populate value_to_type from function locals
        for (value_id, local) in &function.locals {
            debug_mir!(
                value_id = value_id.0,
                local_type = ?local.local_type,
                "Local type mapping"
            );
            self.value_to_type
                .insert(*value_id, local.local_type.clone());
        }

        // Convert MIR function signature to WASM
        let wasm_signature = self.convert_function_signature(&function)?;

        // Allocate locals for function parameters
        debug_mir!(
            function_name = %function.name,
            parameters = function.parameters.len(),
            "Allocating parameter locals"
        );
        for param in &function.parameters {
            let local_index = self.next_local_index;
            debug_mir!(
                param_name = %param.name,
                value_id = param.value_id.0,
                local_index = local_index,
                "Adding parameter to local"
            );
            self.value_to_local.insert(param.value_id, local_index);
            self.next_local_index += 1;
        }
        debug_mir!(
            entries = self.value_to_local.len(),
            "value_to_local entries after parameters"
        );

        // CRITICAL FIX: Allocate locals for function local variables (excluding parameters)
        // Parameters are already in function.locals, so we must skip them to avoid duplication
        // IMPORTANT: Sort by ValueId to ensure consistent allocation order!
        debug_mir!(
            function_name = %function.name,
            locals_count = function.locals.len(),
            parameters = function.parameters.len(),
            "Function locals allocation"
        );

        // Collect and sort locals by ValueId for deterministic allocation
        let mut sorted_locals: Vec<_> = function.locals.iter().collect();
        sorted_locals.sort_by_key(|(value_id, _)| value_id.0);

        for (value_id, _local) in sorted_locals {
            // Skip if this ValueId was already allocated (i.e., it's a parameter)
            if self.value_to_local.contains_key(value_id) {
                debug_mir!(
                    value_id = value_id.0,
                    "Skipping ValueId - already allocated as parameter"
                );
                continue;
            }

            let local_index = self.next_local_index;
            debug_mir!(
                value_id = value_id.0,
                local_index = local_index,
                "Adding ValueId to local"
            );
            self.value_to_local.insert(*value_id, local_index);
            self.next_local_index += 1;
        }
        debug_mir!(
            entries = self.value_to_local.len(),
            "After locals allocation, value_to_local entries"
        );

        // Pre-assign block labels
        for &block_id in function.blocks.keys() {
            self.block_labels.insert(block_id, self.next_block_label);
            self.next_block_label += 1;
        }

        // CRITICAL FIX: Use function.entry_block instead of hardcoded BasicBlockId(0)
        // Functions whose entry block was renumbered will now emit code correctly
        let entry_block_id = function.entry_block;
        tracing::debug!(
            entry_block = ?entry_block_id,
            name = %function.name,
            "Starting code generation from entry block"
        );

        let mut generated_blocks = std::collections::HashSet::new();
        self.generate_structured_blocks(&function, entry_block_id, &mut generated_blocks)?;
        debug_mir!(
            function_name = %function.name,
            instructions = self.current_instructions.len(),
            "After generate_structured_blocks"
        );

        stats.blocks_generated = generated_blocks.len();
        debug_mir!(
            "DEBUG MIR: Generated {} blocks using structured control flow",
            stats.blocks_generated
        );

        // Create WASM function with generated instructions
        tracing::debug!(
            name = %function.name,
            "Computing local types for function"
        );
        let local_types = self.compute_local_types(&function);
        tracing::debug!(
            local_types = local_types.len(),
            instructions = self.current_instructions.len(),
            "Creating WASM function"
        );
        // CRITICAL FIX: For void functions, check if we need to drop a value
        // This prevents "type mismatch at end of function, expected [] but got [X]" errors
        // NOTE: Ptr(Void) represents the "any" type and DOES return a value (i32), so don't treat it as void
        let is_void_function = matches!(function.return_type, MirType::Void);

        debug_mir!(
            function_name = %function.name,
            is_void = is_void_function,
            instructions = self.current_instructions.len(),
            "Void function check"
        );
        if is_void_function && self.current_instructions.len() >= 10 {
            debug_mir!("Last 10 instructions:");
            for (i, inst) in self.current_instructions.iter().rev().take(10).enumerate() {
                debug_mir!(index = -(i as i32 + 1), instruction = ?inst, "Instruction");
            }
        }

        debug_mir!(
            function_name = %function.name,
            instructions = self.current_instructions.len(),
            local_types = local_types.len(),
            "Before copy to WASM function"
        );
        let mut wasm_function = WasmFunction::new(local_types);
        let mut instruction_count = 0;
        debug_mir!(
            instructions = self.current_instructions.len(),
            function_name = %function.name,
            "Copying instructions"
        );
        for (idx, instruction) in self.current_instructions.iter().enumerate() {
            if matches!(instruction, Instruction::Drop) {
                debug_mir!(idx = idx, "Instruction: DROP");
            }
            if let Instruction::Call(func_idx) = instruction {
                debug_mir!(idx = idx, func_idx = func_idx, "Instruction: Call");
            }
            wasm_function.instruction(instruction);
            instruction_count += 1;
        }
        debug_mir!(
            function_name = %function.name,
            instructions_copied = instruction_count,
            "After copy to WASM function"
        );

        // NOTE: Void functions don't need a final DROP instruction.
        // With structured control flow generation, all execution paths are properly handled:
        // - Paths with explicit returns will have Return instructions
        // - Paths that fall through will naturally END the function (valid for void functions)
        // Adding a DROP here causes validation errors when the stack is already empty.

        // CONSTRUCTOR FIX: Add implicit return of 'this' pointer for constructors
        // Constructors must return the instance pointer (i32) which is parameter 0
        let is_constructor =
            function.name == "constructor" || function.name.ends_with(".constructor");
        if is_constructor {
            debug_mir!(
                function_name = %function.name,
                "Adding implicit return of 'this' for constructor"
            );
            wasm_function.instruction(&Instruction::LocalGet(0));
        }

        // CRITICAL FIX: For non-void functions, ensure all paths return
        // If the function returns a value and doesn't end with a Return instruction, add Unreachable
        let is_non_void = !matches!(function.return_type, MirType::Void);
        let last_instruction_is_return = self
            .current_instructions
            .last()
            .is_some_and(|inst| matches!(inst, Instruction::Return | Instruction::Unreachable));

        if is_non_void && !last_instruction_is_return && !is_constructor {
            debug_mir!(
                function_name = %function.name,
                return_type = ?function.return_type,
                "Non-void function missing Return/Unreachable, adding Unreachable"
            );
            wasm_function.instruction(&Instruction::Unreachable);
        }

        // CRITICAL: Add END instruction to properly close the function
        wasm_function.instruction(&Instruction::End);

        // Add function to WASM module
        tracing::debug!(
            name = %function.name,
            instructions = self.current_instructions.len(),
            "Adding function to WASM module"
        );
        self.add_function_to_module(function.name.clone(), wasm_function, wasm_signature)?;
        tracing::debug!(
            name = %function.name,
            "Successfully added function to WASM module"
        );

        Ok(stats)
    }

    /// Generate WASM instructions for a basic block
    #[allow(dead_code)] // Used internally by generate_function
    fn generate_basic_block(&mut self, block: &MirBasicBlock) -> Result<(), CompilerError> {
        tracing::trace!(
            predecessors = block.predecessors.len(),
            "Starting basic block generation"
        );

        // Start block if it has predecessors (not entry block)
        if !block.predecessors.is_empty() {
            if let Some(&label) = self.block_labels.get(&block.id) {
                self.current_instructions
                    .push(Instruction::Block(BlockType::Empty));
                debug_mir!(label = label, "Added Block instruction for label");
            }
        }

        // Generate instructions
        tracing::trace!(
            instructions = block.instructions.len(),
            "Generating block instructions"
        );
        for (i, instruction) in block.instructions.iter().enumerate() {
            debug_mir!("DEBUG MIR: Processing instruction {}: {:?}", i, instruction);
            // debug_mir!("DEBUG MIR: Processing instruction {}: {:?}, dest: {:?}", i, instruction.operation, instruction.dest);
            self.generate_instruction(instruction)?;
        }

        // Generate terminator
        debug_mir!("DEBUG MIR: Generating terminator: {:?}", block.terminator);
        self.generate_terminator(&block.terminator)?;

        // End block if it was started
        if !block.predecessors.is_empty() {
            self.current_instructions.push(Instruction::End);
        }

        debug_mir!("DEBUG MIR: generate_basic_block completed successfully");
        Ok(())
    }

    /// Helper function to check if a block and all its successors eventually return
    /// Returns true if all execution paths from this block lead to a return
    /// Check if a block directly returns (without following Jump terminators).
    /// This is used to determine if an if-else branch exits via return/unreachable,
    /// versus exiting via a Jump to a continuation block.
    fn block_directly_returns(&self, function: &MirFunction, block_id: BasicBlockId) -> bool {
        let mut visited = std::collections::HashSet::new();
        self.block_directly_returns_recursive(function, block_id, &mut visited)
    }

    fn block_directly_returns_recursive(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        // Prevent infinite loops
        if visited.contains(&block_id) {
            return false;
        }
        visited.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            return false;
        };

        match &block.terminator {
            MirTerminator::Return { .. } | MirTerminator::Unreachable => true,
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Both branches must directly return
                self.block_directly_returns_recursive(function, *true_block, visited)
                    && self.block_directly_returns_recursive(function, *false_block, visited)
            }
            MirTerminator::Jump { .. } => {
                // Jump means this branch exits to a continuation - NOT a direct return
                false
            }
        }
    }

    /// Helper function to check if false_block is a continuation (not a real else clause).
    /// A false_block is a continuation if:
    /// 1. It's empty (no instructions) with Unreachable/Jump/Return terminator, OR
    /// 2. The true_block jumps directly to it (indicating no else clause in source), OR
    /// 3. The true_block has nested control flow and one of its exit paths jumps to false_block
    /// This is used to detect when an if statement has NO else clause.
    fn is_continuation_not_else(
        &self,
        function: &MirFunction,
        true_block: BasicBlockId,
        false_block: BasicBlockId,
    ) -> bool {
        let Some(false_blk) = function.blocks.get(&false_block) else {
            return false;
        };

        // Check #1: Empty block with simple terminator
        if false_blk.instructions.is_empty() {
            match &false_blk.terminator {
                MirTerminator::Unreachable | MirTerminator::Jump { .. } => return true,
                MirTerminator::Return { value } => {
                    // Empty continuation if returning nothing or undefined
                    if matches!(
                        value,
                        None | Some(MirOperand::Constant(MirConstant::Undefined))
                    ) {
                        return true;
                    }
                }
                _ => {}
            }
        }

        // Check #2: True branch jumps to false_block
        if let Some(true_blk) = function.blocks.get(&true_block) {
            if let MirTerminator::Jump { target } = &true_blk.terminator {
                if *target == false_block {
                    // True branch jumps to false_block, so false_block is continuation
                    return true;
                }
            }

            // Check #3: True branch has nested control flow (Branch) and one of its exit paths
            // jumps to false_block. This handles cases like:
            //   if outer_condition
            //       if inner_condition
            //           return x
            //   return y  // This is continuation, not else!
            //
            // In this case, true_block has a Branch terminator (the nested if), and the nested if's
            // continuation block jumps to false_block.
            if let MirTerminator::Branch {
                true_block: inner_true,
                false_block: inner_false,
                ..
            } = &true_blk.terminator
            {
                // Check if either branch of the nested if jumps to our false_block
                if self.block_eventually_jumps_to(function, *inner_true, false_block)
                    || self.block_eventually_jumps_to(function, *inner_false, false_block)
                {
                    return true;
                }
            }
        }

        false
    }

    /// Helper to check if a block eventually jumps to a target block (following Jump terminators).
    fn block_eventually_jumps_to(
        &self,
        function: &MirFunction,
        start_block: BasicBlockId,
        target_block: BasicBlockId,
    ) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut to_visit = vec![start_block];

        while let Some(block_id) = to_visit.pop() {
            if visited.contains(&block_id) {
                continue;
            }
            visited.insert(block_id);

            let Some(block) = function.blocks.get(&block_id) else {
                continue;
            };

            match &block.terminator {
                MirTerminator::Jump { target } => {
                    if *target == target_block {
                        return true;
                    }
                    // Follow the jump
                    to_visit.push(*target);
                }
                MirTerminator::Branch {
                    true_block,
                    false_block,
                    ..
                } => {
                    // Check both branches
                    to_visit.push(*true_block);
                    to_visit.push(*false_block);
                }
                MirTerminator::Return { .. } | MirTerminator::Unreachable => {
                    // Dead end
                }
            }
        }

        false
    }

    /// Find the eventual continuation block from a block that may have nested control flow.
    /// This follows through nested Branches to find where non-returning paths eventually jump.
    /// Returns None if all paths return or if there's no common continuation.
    fn find_eventual_continuation(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
    ) -> Option<BasicBlockId> {
        let mut visited = std::collections::HashSet::new();
        let mut continuations = std::collections::HashSet::new();
        self.collect_jump_targets(function, block_id, &mut visited, &mut continuations);

        // If all non-returning paths lead to the same block, that's our continuation
        if continuations.len() == 1 {
            continuations.into_iter().next()
        } else {
            None
        }
    }

    /// Recursively collect all Jump targets from a block's control flow.
    /// This follows through nested Branches to find where non-returning paths jump.
    fn collect_jump_targets(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
        targets: &mut std::collections::HashSet<BasicBlockId>,
    ) {
        if visited.contains(&block_id) {
            return;
        }
        visited.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            return;
        };

        match &block.terminator {
            MirTerminator::Jump { target } => {
                targets.insert(*target);
            }
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Follow both branches
                self.collect_jump_targets(function, *true_block, visited, targets);
                self.collect_jump_targets(function, *false_block, visited, targets);
            }
            MirTerminator::Return { .. } | MirTerminator::Unreachable => {
                // Dead end - no continuation from here
            }
        }
    }

    /// Detects if a block is a loop header by checking for backedges using DFS.
    /// A block is a loop header if there's a cycle in the CFG that includes this block
    /// as the target of a backedge. This is more robust than block ID ordering.
    fn is_loop_header(&self, function: &MirFunction, block_id: BasicBlockId) -> bool {
        // Build successors map for DFS traversal
        let mut successors: std::collections::HashMap<BasicBlockId, Vec<BasicBlockId>> =
            std::collections::HashMap::new();

        for (id, block) in &function.blocks {
            let succs = match &block.terminator {
                MirTerminator::Jump { target } => vec![*target],
                MirTerminator::Branch {
                    true_block,
                    false_block,
                    ..
                } => vec![*true_block, *false_block],
                MirTerminator::Return { .. } | MirTerminator::Unreachable => vec![],
            };
            successors.insert(*id, succs);
        }

        // Find all back edges using DFS from entry block
        let mut visited = std::collections::HashSet::new();
        let mut on_stack = std::collections::HashSet::new();
        let mut back_edges = Vec::new();

        // Start DFS from entry block (block 0)
        let entry_block = BasicBlockId(0);
        if function.blocks.contains_key(&entry_block) {
            self.find_back_edges_dfs_internal(
                entry_block,
                &successors,
                &mut visited,
                &mut on_stack,
                &mut back_edges,
            );
        }

        // Check if this block is the target (header) of any back edge
        back_edges.iter().any(|(_tail, head)| *head == block_id)
    }

    /// Internal DFS helper for finding back edges
    fn find_back_edges_dfs_internal(
        &self,
        block: BasicBlockId,
        successors: &std::collections::HashMap<BasicBlockId, Vec<BasicBlockId>>,
        visited: &mut std::collections::HashSet<BasicBlockId>,
        on_stack: &mut std::collections::HashSet<BasicBlockId>,
        back_edges: &mut Vec<(BasicBlockId, BasicBlockId)>,
    ) {
        visited.insert(block);
        on_stack.insert(block);

        if let Some(succs) = successors.get(&block) {
            for &successor in succs {
                if on_stack.contains(&successor) {
                    // Back edge found: current block -> successor (which is already on stack)
                    back_edges.push((block, successor));
                } else if !visited.contains(&successor) {
                    self.find_back_edges_dfs_internal(
                        successor, successors, visited, on_stack, back_edges,
                    );
                }
            }
        }

        on_stack.remove(&block);
    }

    /// Check if a block is the exit continuation of any loop
    /// (i.e., the false_block of a loop header's Branch)
    fn is_loop_exit_continuation(&self, function: &MirFunction, block_id: BasicBlockId) -> bool {
        function.blocks.iter().any(|(loop_id, loop_block)| {
            // Check if this block is a loop header
            let is_header = self.is_loop_header(function, *loop_id);
            if !is_header {
                return false;
            }

            // Check if the loop header's Branch has our block as the false_block (exit)
            matches!(&loop_block.terminator,
                MirTerminator::Branch { false_block, .. } if *false_block == block_id)
        })
    }

    #[allow(dead_code)]
    fn block_always_returns(&self, function: &MirFunction, block_id: BasicBlockId) -> bool {
        let mut visited = std::collections::HashSet::new();
        let mut on_stack = std::collections::HashSet::new();
        self.block_always_returns_recursive(function, block_id, &mut visited, &mut on_stack)
    }

    #[allow(dead_code)]
    fn block_always_returns_recursive(
        &self,
        function: &MirFunction,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
        on_stack: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        // If we encounter a block on the recursion stack, it's a back-edge (loop)
        // Treat loop back-edges as "okay" - the exit path must return
        if on_stack.contains(&block_id) {
            return true;
        }

        // If already fully analyzed, return cached result
        if visited.contains(&block_id) {
            return false;
        }

        // Mark this block as being on the recursion stack
        on_stack.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            on_stack.remove(&block_id);
            return false;
        };

        let result = match &block.terminator {
            MirTerminator::Return { .. } | MirTerminator::Unreachable => true,
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Both branches must return (back-edges return true automatically)
                self.block_always_returns_recursive(function, *true_block, visited, on_stack)
                    && self.block_always_returns_recursive(
                        function,
                        *false_block,
                        visited,
                        on_stack,
                    )
            }
            MirTerminator::Jump { target } => {
                // Follow the jump
                self.block_always_returns_recursive(function, *target, visited, on_stack)
            }
        };

        // Remove from recursion stack
        on_stack.remove(&block_id);

        // If this block doesn't return, cache it
        if !result {
            visited.insert(block_id);
        }

        result
    }

    /// Find the block that eventually leads to the loop's increment/backedge.
    /// This handles cases where the body contains nested control flow (if statements)
    /// and needs to follow through to find what eventually jumps to increment.
    fn find_loop_increment_block(
        &self,
        function: &MirFunction,
        body_block_id: BasicBlockId,
        header_block_id: BasicBlockId,
    ) -> Option<BasicBlockId> {
        // Look for a block that:
        // 1. Jumps back to the header (backedge)
        // 2. Has a block ID GREATER than the body_block_id (comes after in CFG order)
        // 3. Is labeled as an increment block (for explicit for-loop increments)
        //
        // The second condition is crucial: the init block (block 0) also jumps to header,
        // but it's not the increment block - it's the entry point.
        for (block_id, block) in &function.blocks {
            if let MirTerminator::Jump { target } = &block.terminator {
                if *target == header_block_id
                    && block_id.0 > body_block_id.0
                    && block_id.0 > header_block_id.0
                {
                    // Found a block that jumps back to header - this is the increment block
                    // Must be AFTER both the header and body in block ID order
                    debug_mir!(
                        "DEBUG find_loop_increment_block: Found increment block {:?} (label={:?}) that jumps to header {:?}",
                        block_id, block.label, header_block_id
                    );
                    return Some(*block_id);
                }
            }
        }
        None
    }

    /// Check if a path from start_block eventually reaches target_block
    #[allow(dead_code)]
    fn path_reaches_block(
        &self,
        function: &MirFunction,
        start_block: BasicBlockId,
        target_block: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        if start_block == target_block {
            return true;
        }
        if visited.contains(&start_block) {
            return false;
        }
        visited.insert(start_block);

        if let Some(block) = function.blocks.get(&start_block) {
            match &block.terminator {
                MirTerminator::Jump { target } => {
                    self.path_reaches_block(function, *target, target_block, visited)
                }
                MirTerminator::Branch {
                    true_block,
                    false_block,
                    ..
                } => {
                    self.path_reaches_block(function, *true_block, target_block, visited)
                        || self.path_reaches_block(function, *false_block, target_block, visited)
                }
                MirTerminator::Return { .. } | MirTerminator::Unreachable => false,
            }
        } else {
            false
        }
    }

    /// Generate structured control flow for blocks
    /// Generate a branch block body (for if/else branches) without following Jump terminators.
    /// Jump terminators in branch blocks represent exits to continuation blocks that should
    /// be generated after the if-else structure, not inside the branch.
    fn generate_branch_block(
        &mut self,
        function: &MirFunction,
        block_id: BasicBlockId,
        generated: &mut std::collections::HashSet<BasicBlockId>,
    ) -> Result<(), CompilerError> {
        // Skip if already generated
        if generated.contains(&block_id) {
            debug_mir!(
                "DEBUG BRANCH_BLOCK: Skipping already-generated block {:?} in function '{}'",
                block_id,
                function.name
            );
            return Ok(());
        }
        debug_mir!(
            "DEBUG BRANCH_BLOCK: Inserting block {:?} into generated set for function '{}'",
            block_id,
            function.name
        );
        generated.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            return Ok(());
        };

        // Generate block instructions
        debug_mir!(
            "DEBUG BRANCH_BLOCK: Block {:?} has {} instructions in function '{}'",
            block_id,
            block.instructions.len(),
            function.name
        );
        for instruction in &block.instructions {
            self.generate_instruction(instruction)?;
        }

        // Handle terminator - but DON'T follow Jump terminators (those are exits to continuations)
        debug_mir!(
            "DEBUG BRANCH_BLOCK: Block {:?} terminator is {:?} in function '{}'",
            block_id,
            block.terminator,
            function.name
        );
        match &block.terminator {
            MirTerminator::Return { value } => {
                if let Some(return_value) = value {
                    if !matches!(return_value, MirOperand::Constant(MirConstant::Undefined)) {
                        self.load_operand(return_value)?;
                    }
                }
                self.current_instructions.push(Instruction::Return);
            }

            MirTerminator::Jump { .. } => {
                // Don't follow jumps in branch blocks - they jump to continuations
                // The continuation will be generated after the if-else structure
            }

            MirTerminator::Branch {
                condition,
                true_block,
                false_block,
            } => {
                // Check if false_block is a continuation (no else clause in source)
                let has_else_clause =
                    !self.is_continuation_not_else(function, *true_block, *false_block);

                debug_mir!("DEBUG BRANCH_BLOCK: Processing nested Branch in function '{}', true_block={:?}, false_block={:?}, has_else_clause={}",
                    function.name, true_block, false_block, has_else_clause);

                // Nested if-else: generate it fully (including its own continuation handling)
                self.load_operand(condition)?;
                self.current_instructions
                    .push(Instruction::If(BlockType::Empty));

                self.generate_branch_block(function, *true_block, generated)?;

                // Only generate else clause if false_block is NOT a loop exit continuation
                // CRITICAL FIX: If false_block is the exit continuation of any loop in the function,
                // it will be generated by the loop structure itself. Don't generate it as an else clause.
                let is_loop_exit = self.is_loop_exit_continuation(function, *false_block);

                debug_mir!("DEBUG BRANCH_BLOCK: function='{}', block={:?}, false_block={:?}, is_loop_exit={}",
                    function.name, block_id, false_block, is_loop_exit);

                if has_else_clause && !is_loop_exit {
                    self.current_instructions.push(Instruction::Else);
                    self.generate_branch_block(function, *false_block, generated)?;
                } else if is_loop_exit {
                    debug_mir!("DEBUG BRANCH_BLOCK: Skipping false_block {:?} - detected as loop exit continuation in function '{}'",
                        false_block, function.name);
                }

                self.current_instructions.push(Instruction::End);

                // After generating nested if-else, check if there's a continuation to inline.
                // If both branches jump to the same continuation, generate it inline.
                // If one branch returns and the other jumps, generate the jumped-to continuation.
                let true_has_return = self.block_directly_returns(function, *true_block);
                let false_has_return = if has_else_clause {
                    self.block_directly_returns(function, *false_block)
                } else {
                    false // No else clause means false branch doesn't return
                };

                if true_has_return && false_has_return {
                    // Both nested branches return - add unreachable
                    self.current_instructions.push(Instruction::Unreachable);
                } else {
                    // Find continuation block (if any) to inline
                    let mut continuation: Option<BasicBlockId> = None;

                    if !true_has_return {
                        if let Some(true_blk) = function.blocks.get(true_block) {
                            if let MirTerminator::Jump { target } = &true_blk.terminator {
                                continuation = Some(*target);
                            }
                        }
                    }

                    if !false_has_return && has_else_clause {
                        if let Some(false_blk) = function.blocks.get(false_block) {
                            if let MirTerminator::Jump { target } = &false_blk.terminator {
                                // If true branch also jumps, verify same target
                                if let Some(true_cont) = continuation {
                                    if true_cont == *target {
                                        // Both jump to same place - inline it
                                        continuation = Some(*target);
                                    } else {
                                        // Different targets - don't inline
                                        continuation = None;
                                    }
                                } else {
                                    continuation = Some(*target);
                                }
                            }
                        }
                    }

                    // If no else clause, false branch goes to continuation directly
                    if !has_else_clause && continuation.is_none() {
                        debug_mir!("DEBUG BRANCH_BLOCK: No else clause in nested if, setting continuation to false_block {:?} in function '{}'",
                            false_block, function.name);
                        continuation = Some(*false_block);
                    }

                    // Inline continuation if found
                    // BUT: Don't inline if the continuation block will be generated by an outer structure
                    // Check if continuation is already marked for generation by checking if it's
                    // already in the generated set (if so, skip)
                    if let Some(cont) = continuation {
                        if !generated.contains(&cont) {
                            debug_mir!("DEBUG BRANCH_BLOCK: Inlining continuation block {:?} for nested if in function '{}'",
                                cont, function.name);
                            self.generate_branch_block(function, cont, generated)?;
                        } else {
                            debug_mir!("DEBUG BRANCH_BLOCK: Skipping continuation block {:?} - already marked for generation by outer structure in function '{}'",
                                cont, function.name);
                        }
                    }
                }
            }

            MirTerminator::Unreachable => {
                // CRITICAL FIX: Skip adding Unreachable - see comment in generate_structured_blocks
                // MirTerminator::Unreachable is a placeholder that should not generate WASM Unreachable
                // for void functions ending naturally.
            }
        }

        Ok(())
    }

    fn generate_structured_blocks(
        &mut self,
        function: &MirFunction,
        block_id: BasicBlockId,
        generated: &mut std::collections::HashSet<BasicBlockId>,
    ) -> Result<(), CompilerError> {
        // Skip if already generated
        if generated.contains(&block_id) {
            debug_mir!("DEBUG GENERATE_BLOCKS: Skipping already-generated block {:?} in function '{}', generated set contains {} blocks",
                block_id, function.name, generated.len());
            return Ok(());
        }
        debug_mir!(
            "DEBUG GENERATE_BLOCKS: Inserting block {:?} into generated set for function '{}'",
            block_id,
            function.name
        );
        generated.insert(block_id);

        let Some(block) = function.blocks.get(&block_id) else {
            debug_mir!(
                "DEBUG GENERATE_BLOCKS: Block {:?} not found in function '{}'",
                block_id,
                function.name
            );
            return Ok(());
        };

        debug_mir!(
            "DEBUG GENERATE_BLOCKS: Generating block {:?} in function '{}', terminator={:?}",
            block_id,
            function.name,
            block.terminator
        );

        // CRITICAL: Check if this block is a loop header BEFORE generating instructions
        // For loop headers, instructions must be generated INSIDE the loop
        let is_loop_header = self.is_loop_header(function, block_id);

        // Generate block instructions (UNLESS this is a loop header - those go inside the loop)
        if !is_loop_header {
            for instruction in &block.instructions {
                self.generate_instruction(instruction)?;
            }
        }

        // Handle terminator with structured control flow
        match &block.terminator {
            MirTerminator::Return { value } => {
                if let Some(return_value) = value {
                    if !matches!(return_value, MirOperand::Constant(MirConstant::Undefined)) {
                        self.load_operand(return_value)?;
                    }
                }
                self.current_instructions.push(Instruction::Return);
            }

            MirTerminator::Jump { target } => {
                // Just continue to next block inline
                self.generate_structured_blocks(function, *target, generated)?;
            }

            MirTerminator::Branch {
                condition,
                true_block,
                false_block,
            } => {
                // CRITICAL FIX: Check if this block is a loop header (has backedge)
                let is_loop = self.is_loop_header(function, block_id);

                debug_mir!("DEBUG BRANCH: Block {:?} in function '{}', is_loop={}, true_block={:?}, false_block={:?}",
                    block_id, function.name, is_loop, true_block, false_block);

                if is_loop {
                    debug_mir!("DEBUG LOOP: Block {:?} is a loop header in function '{}', generating loop structure with false_block={:?} as continuation",
                        block_id, function.name, false_block);

                    // Generate loop structure:
                    // block (outer - for exit via br_if 1)
                    //   loop (inner - for continue via br 0)
                    //     header block instructions (condition evaluation)
                    //     condition check
                    //     br_if 1 (!condition) - exit if condition is false
                    //     body (true_block)
                    //     br 0 - jump back to loop header (backedge handled by MirTerminator::Jump)
                    //   end
                    // end
                    // continuation (false_block)

                    self.current_instructions
                        .push(Instruction::Block(BlockType::Empty)); // label @1 (exit target)
                    self.current_instructions
                        .push(Instruction::Loop(BlockType::Empty)); // label @0 (loop target)

                    // CRITICAL: Generate header block instructions INSIDE the loop
                    // This ensures condition is re-evaluated on each iteration
                    for instruction in &block.instructions {
                        self.generate_instruction(instruction)?;
                    }

                    // Load condition and negate it (br_if when condition is FALSE)
                    self.load_operand(condition)?;
                    self.current_instructions.push(Instruction::I32Eqz); // Negate: br_if when 0 (false)
                    self.current_instructions.push(Instruction::BrIf(1)); // Exit to block @1 if condition is false

                    // Generate loop body (true_block) - this will have a Jump to increment or back to header
                    // Mark the loop body as generated to prevent infinite recursion
                    self.generate_branch_block(function, *true_block, generated)?;

                    // CRITICAL FIX: Find and generate the increment block for for-loops
                    // The body may contain nested control flow (if statements), so we can't just
                    // check the body's direct terminator. Instead, find the increment block
                    // by looking for any block that jumps back to the header.
                    let increment_block_id =
                        self.find_loop_increment_block(function, *true_block, block_id);

                    if let Some(increment_id) = increment_block_id {
                        // CRITICAL FIX: Check if increment block was already generated
                        // This happens when a nested if statement's continuation is the same
                        // as the increment block (e.g., while loop with if inside).
                        // The if's continuation handling may have already generated this block.
                        if !generated.contains(&increment_id) {
                            if let Some(increment_block) = function.blocks.get(&increment_id) {
                                debug_mir!(
                                    "DEBUG LOOP: Found increment block {:?} that jumps back to header {:?}",
                                    increment_id, block_id
                                );

                                // Generate increment block instructions INSIDE the loop
                                for instruction in &increment_block.instructions {
                                    self.generate_instruction(instruction)?;
                                }
                                generated.insert(increment_id);
                            }
                        } else {
                            debug_mir!(
                                "DEBUG LOOP: Increment block {:?} already generated (by nested if continuation), skipping",
                                increment_id
                            );
                        }
                        // Always add br 0 to jump back to loop header (regardless of whether we generated the block)
                        self.current_instructions.push(Instruction::Br(0));
                    } else {
                        // No separate increment block - check if body jumps directly back to header
                        if let Some(body_block) = function.blocks.get(true_block) {
                            if let MirTerminator::Jump { target } = &body_block.terminator {
                                if *target == block_id {
                                    // Simple while loop - body jumps directly back to header
                                    debug_mir!("DEBUG LOOP: Body block {:?} jumps directly back to header {:?}, adding br 0", true_block, block_id);
                                    self.current_instructions.push(Instruction::Br(0));
                                }
                            }
                        }
                    }

                    self.current_instructions.push(Instruction::End); // end loop
                    self.current_instructions.push(Instruction::End); // end block

                    // Generate continuation (false_block) - this is where we exit to
                    debug_mir!(
                        "DEBUG LOOP: Generating continuation block {:?} for loop in function '{}'",
                        false_block,
                        function.name
                    );
                    self.generate_structured_blocks(function, *false_block, generated)?;
                } else {
                    // Regular if/else (not a loop)
                    // Check if false_block is a continuation (no else clause in source)
                    let has_else_clause =
                        !self.is_continuation_not_else(function, *true_block, *false_block);

                    // Generate if/else structure
                    self.load_operand(condition)?;
                    self.current_instructions
                        .push(Instruction::If(BlockType::Empty));

                    // Use generate_branch_block to avoid following Jump terminators inside branches
                    self.generate_branch_block(function, *true_block, generated)?;

                    // Only generate else clause if false_block is NOT an empty continuation
                    if has_else_clause {
                        self.current_instructions.push(Instruction::Else);
                        self.generate_branch_block(function, *false_block, generated)?;
                    }

                    self.current_instructions.push(Instruction::End);

                    // Check if both branches directly return (without following Jumps to continuations)
                    let true_has_return = self.block_directly_returns(function, *true_block);
                    let false_has_return = if has_else_clause {
                        self.block_directly_returns(function, *false_block)
                    } else {
                        false // No else clause means false branch doesn't return
                    };

                    debug_mir!("DEBUG RETURN CHECK: Function '{}', Block {:?}, true_has_return={}, false_has_return={}, has_else_clause={}",
                        function.name, block_id, true_has_return, false_has_return, has_else_clause);

                    if true_has_return && false_has_return {
                        // Both branches return - add unreachable to indicate code after if-else is never reached
                        self.current_instructions.push(Instruction::Unreachable);
                    } else {
                        // Find and generate continuation block
                        // We need to find the continuation that at least one non-returning branch jumps to
                        // CRITICAL FIX: Use find_eventual_continuation to handle nested control flow
                        let mut continuation: Option<BasicBlockId> = None;

                        // Check if true branch jumps to a continuation (may have nested control flow)
                        if !true_has_return {
                            if let Some(true_blk) = function.blocks.get(true_block) {
                                match &true_blk.terminator {
                                    MirTerminator::Jump { target } => {
                                        continuation = Some(*target);
                                    }
                                    MirTerminator::Branch { .. } => {
                                        // Nested control flow - find eventual continuation
                                        continuation =
                                            self.find_eventual_continuation(function, *true_block);
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // Check if false branch jumps to a continuation (should be same as true if both jump)
                        if !false_has_return {
                            if has_else_clause {
                                // Real else clause - check where it jumps
                                if let Some(false_blk) = function.blocks.get(false_block) {
                                    let false_cont = match &false_blk.terminator {
                                        MirTerminator::Jump { target } => Some(*target),
                                        MirTerminator::Branch { .. } => {
                                            // Nested control flow - find eventual continuation
                                            self.find_eventual_continuation(function, *false_block)
                                        }
                                        _ => None,
                                    };

                                    if let Some(fc) = false_cont {
                                        if continuation.is_none() {
                                            continuation = Some(fc);
                                        }
                                        // Both branches should lead to same continuation
                                    }
                                }
                            } else {
                                // No else clause - false_block IS the continuation
                                if continuation.is_none() {
                                    continuation = Some(*false_block);
                                }
                            }
                        }

                        // Generate the continuation block if we found one
                        if let Some(cont) = continuation {
                            self.generate_structured_blocks(function, cont, generated)?;
                        }
                    }
                }
            }

            MirTerminator::Unreachable => {
                // CRITICAL FIX: Only add Unreachable for truly unreachable code (inside branches with both returning)
                // For void functions that reach the end naturally, we should NOT add Unreachable.
                // The function will end with the End instruction added later.
                //
                // We only add Unreachable here if this block is NOT reachable from normal control flow.
                // Since MirTerminator::Unreachable is used as a placeholder during MIR construction,
                // reaching it at the end of function generation means the function ends naturally.
                // For void functions, this is valid - no Unreachable needed.
                //
                // Skip adding Unreachable - let function end naturally with End instruction
            }
        }

        Ok(())
    }

    /// Generate WASM instruction from MIR instruction
    fn generate_instruction(&mut self, instruction: &MirInstruction) -> Result<(), CompilerError> {
        match &instruction.operation {
            MirOperation::Copy { source } => {
                // Load source operand and store to destination
                self.load_operand(source)?;
                if let Some(dest) = instruction.dest {
                    // Track string constants being copied to locals
                    if let MirOperand::Constant(MirConstant::String(index)) = source {
                        tracing::trace!(
                            value_id = ?dest.0,
                            string_index = index,
                            "Tracking string constant"
                        );
                        self.value_to_string_index.insert(dest, *index);
                    }
                    // CRITICAL FIX: Pass source type for automatic type conversion
                    let source_type = self.get_operand_mir_type(source);
                    self.store_to_local_with_conversion(dest, source_type)?;
                } else {
                    // No destination - drop the value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::Phi { incoming } => {
                // SSA Phi nodes are NO-OPs in WASM structured control flow.
                //
                // Why: Phi marks where values from different control flow paths merge.
                // In proper SSA with Phi nodes for loops:
                // 1. INIT block: Sets initial value (e.g., counter = 0) to the Phi result local
                // 2. HEADER block: Phi node (THIS - just a marker, NO-OP in WASM)
                // 3. BODY block: Updates value, Copy to Phi result local
                // 4. Jump back to HEADER
                //
                // The Phi result local ALREADY has the right value from:
                // - First iteration: INIT block set it
                // - Subsequent iterations: BODY's Copy instruction updated it
                //
                // If we generate code for Phi, we'd RESET the value every iteration!
                // Therefore: Phi is a complete NO-OP in WASM codegen.
                debug_mir!(
                    "DEBUG PHI: Phi node is NO-OP: dest={:?}, incoming={:?}",
                    instruction.dest,
                    incoming
                );
            }

            MirOperation::BinaryOp { op, left, right } => {
                // CRITICAL FIX: Type-aware binary operations with automatic conversions
                let left_is_float = self.is_float_operand(left);
                let right_is_float = self.is_float_operand(right);

                // Load left operand
                self.load_operand(left)?;
                // Convert left to f64 if right is float and left is not
                if !left_is_float && right_is_float {
                    self.current_instructions.push(Instruction::F64ConvertI32S);
                }

                // Load right operand
                self.load_operand(right)?;
                // Convert right to f64 if left is float and right is not
                if left_is_float && !right_is_float {
                    self.current_instructions.push(Instruction::F64ConvertI32S);
                }

                // Generate the operation
                self.generate_binary_operation(op, left, right)?;
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                } else {
                    // No destination - drop the result to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::UnaryOp { op, operand } => {
                // BOOK: required-operator - Special handling for Required operator
                // Required needs to check if value is null and trap if so
                if matches!(op, MirUnaryOp::Required) {
                    // Required assertion: value! traps if value is null
                    self.load_operand(operand)?;

                    if let Some(dest) = instruction.dest {
                        // Use dest local to store value and perform null check
                        if let Some(&local_index) = self.value_to_local.get(&dest) {
                            // Store to local and keep on stack with tee
                            self.current_instructions
                                .push(Instruction::LocalTee(local_index));
                            // Check if null (0)
                            self.current_instructions.push(Instruction::I32Eqz);
                            // If null, trap
                            self.current_instructions
                                .push(Instruction::If(wasm_encoder::BlockType::Empty));
                            self.current_instructions.push(Instruction::Unreachable);
                            self.current_instructions.push(Instruction::End);
                            // Value is still in local, load it back for the result
                            self.current_instructions
                                .push(Instruction::LocalGet(local_index));
                            // Store to dest (which is the same local)
                            self.current_instructions
                                .push(Instruction::LocalSet(local_index));
                        } else {
                            // No local mapping - just do the check and drop
                            self.current_instructions.push(Instruction::I32Eqz);
                            self.current_instructions
                                .push(Instruction::If(wasm_encoder::BlockType::Empty));
                            self.current_instructions.push(Instruction::Unreachable);
                            self.current_instructions.push(Instruction::End);
                        }
                    } else {
                        // No destination - just check and drop
                        // Stack: [value]
                        // Check if null
                        self.current_instructions.push(Instruction::I32Eqz);
                        self.current_instructions
                            .push(Instruction::If(wasm_encoder::BlockType::Empty));
                        self.current_instructions.push(Instruction::Unreachable);
                        self.current_instructions.push(Instruction::End);
                        // Value was consumed by the check, nothing to drop
                    }
                } else {
                    // Normal unary operation
                    self.load_operand(operand)?;
                    self.generate_unary_operation(op)?;
                    if let Some(dest) = instruction.dest {
                        self.store_to_local(dest)?;
                    } else {
                        // No destination - drop the result to avoid stack pollution
                        self.current_instructions.push(Instruction::Drop);
                    }
                }
            }

            MirOperation::Load { source } => {
                tracing::trace!(
                    source = ?source,
                    "Processing Load operation"
                );
                // Load from memory
                match self.load_operand(source) {
                    Ok(_) => debug_mir!("Load operand successful"),
                    Err(e) => {
                        debug_mir!(error = ?e, "Load operand failed");
                        return Err(e);
                    }
                }

                // Add memory load instruction based on destination type
                if let Some(dest) = instruction.dest {
                    // Get the type of the destination to determine which load instruction to use
                    let dest_type = self
                        .value_to_type
                        .get(&dest)
                        .cloned()
                        .unwrap_or(MirType::I32);

                    match dest_type {
                        MirType::F64 => {
                            self.current_instructions.push(Instruction::F64Load(
                                wasm_encoder::MemArg {
                                    offset: 0,
                                    align: 3, // f64 alignment is 8 bytes (2^3)
                                    memory_index: 0,
                                },
                            ));
                            debug_mir!("Added F64Load instruction");
                        }
                        MirType::F32 => {
                            self.current_instructions.push(Instruction::F32Load(
                                wasm_encoder::MemArg {
                                    offset: 0,
                                    align: 2, // f32 alignment is 4 bytes (2^2)
                                    memory_index: 0,
                                },
                            ));
                            debug_mir!("Added F32Load instruction");
                        }
                        _ => {
                            // Default to I32Load for integer types and pointers
                            self.current_instructions.push(Instruction::I32Load(
                                wasm_encoder::MemArg {
                                    offset: 0,
                                    align: 2, // i32 alignment is 4 bytes (2^2)
                                    memory_index: 0,
                                },
                            ));
                            debug_mir!("Added I32Load instruction");
                        }
                    }

                    match self.store_to_local(dest) {
                        Ok(_) => debug_mir!("Load operation completed successfully"),
                        Err(e) => {
                            debug_mir!(error = ?e, "Failed to store Load result");
                            return Err(e);
                        }
                    }
                } else {
                    // No destination - use I32Load as default and drop the loaded value
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                    self.current_instructions.push(Instruction::Drop);
                }
            }

            MirOperation::Store { destination, value } => {
                // Store to memory
                self.load_operand(destination)?;
                self.load_operand(value)?;

                // Determine store instruction based on value type
                let value_type = if let MirOperand::Value(value_id) = value {
                    self.value_to_type
                        .get(value_id)
                        .cloned()
                        .unwrap_or(MirType::I32)
                } else {
                    MirType::I32 // Default for constants and other operands
                };

                match value_type {
                    MirType::F64 => {
                        self.current_instructions.push(Instruction::F64Store(
                            wasm_encoder::MemArg {
                                offset: 0,
                                align: 3, // f64 alignment is 8 bytes (2^3)
                                memory_index: 0,
                            },
                        ));
                    }
                    MirType::F32 => {
                        self.current_instructions.push(Instruction::F32Store(
                            wasm_encoder::MemArg {
                                offset: 0,
                                align: 2, // f32 alignment is 4 bytes (2^2)
                                memory_index: 0,
                            },
                        ));
                    }
                    _ => {
                        // Default to I32Store for integer types and pointers
                        self.current_instructions.push(Instruction::I32Store(
                            wasm_encoder::MemArg {
                                offset: 0,
                                align: 2, // i32 alignment is 4 bytes (2^2)
                                memory_index: 0,
                            },
                        ));
                    }
                }
            }

            MirOperation::Call {
                function,
                arguments,
            } => {
                debug_mir!(
                    "DEBUG CALL START: function={:?}, arguments_len={}",
                    function,
                    arguments.len()
                );

                tracing::trace!(
                    function = ?function,
                    arguments = arguments.len(),
                    "Processing Call operation"
                );

                // Flag to track if print calls were already emitted (for multi-arg print)
                let mut call_already_emitted = false;

                // Get function signature to determine parameter types
                let (mut function_name, function_signature, symbol_id_opt) = match function {
                    MirOperand::Function(symbol_id) => {
                        debug_mir!(" CALL SYMBOL: SymbolId({})", symbol_id.0);
                        let name = self.get_function_name_by_symbol(*symbol_id);
                        debug_mir!(" CALL NAME FROM SYMBOL: {:?}", name);
                        let sig = self.function_signatures.get(symbol_id).cloned();
                        (name, sig, Some(*symbol_id))
                    }
                    MirOperand::NamedFunction { name, symbol_id } => {
                        debug_mir!(
                            "DEBUG CALL NAMED FUNCTION: name='{}', SymbolId({})",
                            name,
                            symbol_id.0
                        );
                        // CRITICAL FIX: For namespace functions (SymbolId(0)), don't use the signature
                        // because SymbolId(0) is shared by all namespace functions and maps to "print"
                        // which has a Void return type. This causes namespace functions like list.add
                        // to incorrectly be treated as void functions.
                        let sig = if symbol_id.0 == 0 {
                            None // Don't use signature for namespace functions
                        } else {
                            self.function_signatures.get(symbol_id).cloned()
                        };
                        (Some(name.clone()), sig, Some(*symbol_id))
                    }
                    _ => (None, None, None),
                };

                // CRITICAL FIX: For stdlib namespace functions (SymbolId(0)), try reverse lookup
                // NamedFunction operands already have the correct name, so skip reverse lookup for them
                // Only do reverse lookup for plain Function operands with missing/wrong names
                let needs_reverse_lookup = matches!(function, MirOperand::Function(_))
                    && (function_name.is_none()
                        || (symbol_id_opt.is_some_and(|id| id.0 == 0)
                            && function_name.as_deref() == Some("print")));

                if needs_reverse_lookup {
                    if let MirOperand::Function(symbol_id) = function {
                        if let Some(&function_index) = self.symbol_to_function_index.get(symbol_id)
                        {
                            // Reverse-lookup: find the function name that maps to this index
                            for (name, &index) in &self.wasm_generator.function_map {
                                if index == function_index {
                                    debug_mir!(
            "DEBUG REVERSE LOOKUP: SymbolId({}) -> index {} -> name '{}'",
                                        symbol_id.0, function_index, name
                                    );
                                    function_name = Some(name.clone());
                                    break;
                                }
                            }
                        }
                    }
                }

                debug_mir!(function_name = ?function_name, "Function name resolved");

                // CRITICAL FIX: String expansion should only happen for built-in functions
                // User-defined functions receive string pointers (to [len|content] structure)
                // Functions that need string arguments expanded to (content_ptr, len)
                debug_mir!(
                    "DEBUG FUNCTION MATCH: function_name={:?}, arguments={}",
                    function_name,
                    arguments.len()
                );
                match function_name.as_deref() {
                    Some("print") | Some("printl") | Some("println") => {
                        debug_mir!(": Matched print function, loading {} arguments", arguments.len());
                        // CRITICAL FIX: For multi-argument print, we must call print ONCE PER ARGUMENT
                        // The print function takes (content_ptr, length) - only 2 params
                        // So print("Value:", x) should emit TWO print calls, not one

                        // Get the print function index once
                        let print_func_name = function_name.as_deref().unwrap_or("print");
                        let print_idx = *self.wasm_generator.function_map.get(print_func_name)
                            .ok_or_else(|| CompilerError::Codegen {
                                context: Box::new(crate::error::ErrorContext::new(
                                    format!("Print function '{}' not found in function map", print_func_name),
                                    None,
                                    crate::error::ErrorType::Codegen,
                                    Some(instruction.location.clone()),
                                )),
                            })?;

                        for (i, arg) in arguments.iter().enumerate() {
                            debug_mir!(": Loading print arg[{}]: {:?}", i, arg);
                            // Load this argument's (content_ptr, length) onto stack
                            self.load_string_argument_for_print(arg)?;
                            // Call print immediately for this argument
                            self.current_instructions.push(Instruction::Call(print_idx));
                            debug_mir!(": Called print for arg[{}]", i);
                        }

                        // Mark that we've already emitted the print calls
                        call_already_emitted = true;
                    }
                    Some("string.concat") | Some("string_concat") | Some("native_string_concat") => {
                        debug_mir!(": Matched string.concat");
                        // FIXED: native_string_concat expects 2 i32 arguments: (str_ptr1, str_ptr2) -> result_ptr
                        // Each pointer points to a string structure: [4-byte length][data bytes]
                        // DO NOT expand to (ptr, len) pairs - just pass the struct pointers
                        for arg in arguments {
                            self.load_string_pointer_only(arg)?;
                        }
                    }
                    Some("input")
                    | Some("input_string")
                    | Some("input_integer")
                    | Some("input_float")
                    | Some("input_yesno")
                    | Some("input.integer")  // Dot notation variants
                    | Some("input.float")
                    | Some("input.yesNo") => {
                        debug_mir!(": Matched input function - using load_string_pointer_only");
                        // FIXED: Input functions now expect only (prompt_ptr) -> result
                        // They no longer take length parameter
                        for arg in arguments {
                            self.load_string_pointer_only(arg)?;
                        }
                    }
                    Some("input_range") => {
                        // input_range expects (prompt_ptr, prompt_len, min, max) -> result
                        // Only expand the first argument (prompt string)
                        if !arguments.is_empty() {
                            self.load_string_argument_for_print(&arguments[0])?;
                            // Load remaining arguments normally (min, max)
                            for arg in &arguments[1..] {
                                self.load_operand(arg)?;
                            }
                        }
                    }
                    // REMOVED: Hardcoded string expansion for bridge functions
                    // Bridge functions with expand_strings=true now use wrapper functions
                    // that handle the expansion automatically. The wrapper receives original
                    // Clean Language string pointers and expands them to (ptr+4, len) pairs.
                    // No special handling needed here - just use normal load_operand.
                    Some("conditional.number") => {
                        // conditional.number(bool, f64, f64) -> f64
                        // Need to convert integer arguments to f64
                        debug_mir!(": Matched conditional.number, converting integer args to f64");
                        for (i, arg) in arguments.iter().enumerate() {
                            self.load_operand(arg)?;
                            // Convert second and third arguments (true/false values) from i32 to f64 if needed
                            if i > 0 && matches!(arg, MirOperand::Constant(MirConstant::Integer(_))) {
                                self.current_instructions.push(Instruction::F64ConvertI32S);
                            } else if i > 0 {
                                if let Some(MirOperand::Value(value_id)) = Some(arg) {
                                    if let Some(mir_type) = self.value_to_type.get(value_id) {
                                        if matches!(
                                            mir_type,
                                            MirType::I32
                                                | MirType::I8
                                                | MirType::I16
                                                | MirType::U8
                                                | MirType::U16
                                                | MirType::U32
                                        ) {
                                            self.current_instructions.push(Instruction::F64ConvertI32S);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Some(name) if name.starts_with("math.") => {
                        // CRITICAL FIX: Math functions expect f64 parameters
                        // Convert i32 (integer) arguments to f64 (number) automatically
                        for arg in arguments {
                            self.load_operand(arg)?;
                            // Check if this is an integer constant or integer value
                            // For now, assume integers need conversion (MIR should track this properly)
                            // Integer constants and values default to i32, math functions expect f64
                            if matches!(arg, MirOperand::Constant(MirConstant::Integer(_))) {
                                // Convert i32 to f64
                                self.current_instructions.push(Instruction::F64ConvertI32S);
                            } else if let Some(MirOperand::Value(value_id)) = Some(arg) {
                                // Check if the value type is an integer type
                                if let Some(mir_type) = self.value_to_type.get(value_id) {
                                    if matches!(
                                        mir_type,
                                        MirType::I32
                                            | MirType::I8
                                            | MirType::I16
                                            | MirType::U8
                                            | MirType::U16
                                            | MirType::U32
                                    ) {
                                        self.current_instructions.push(Instruction::F64ConvertI32S);
                                    }
                                }
                            }
                        }
                    }
                    Some(name)
                        if name.starts_with("number")
                            || name == "float_to_string"
                            || name == "integer.toNumber" =>
                    {
                        // CRITICAL FIX: number conversion functions expect f64 parameters
                        // number_to_string, float_to_string, etc. expect f64
                        // Convert i32 (integer) arguments to f64 (number) automatically
                        // NOTE: string.toNumber and boolean.toNumber take i32 inputs (pointer/boolean)
                        // and should NOT be converted - only integer.toNumber needs conversion
                        for arg in arguments {
                            self.load_operand(arg)?;
                            if matches!(arg, MirOperand::Constant(MirConstant::Integer(_))) {
                                self.current_instructions.push(Instruction::F64ConvertI32S);
                            } else if let Some(MirOperand::Value(value_id)) = Some(arg) {
                                if let Some(mir_type) = self.value_to_type.get(value_id) {
                                    if matches!(
                                        mir_type,
                                        MirType::I32
                                            | MirType::I8
                                            | MirType::I16
                                            | MirType::U8
                                            | MirType::U16
                                            | MirType::U32
                                    ) {
                                        self.current_instructions.push(Instruction::F64ConvertI32S);
                                    }
                                }
                            }
                        }
                    }
                    Some("string.toNumber") | Some("boolean.toNumber") => {
                        // string.toNumber takes i32 (string pointer) -> f64
                        // boolean.toNumber takes i32 (boolean value) -> f64
                        // Do NOT convert the i32 argument to f64
                        for arg in arguments {
                            self.load_operand(arg)?;
                        }
                    }
                    _ => {
                        // For user-defined functions and other built-ins, load arguments with automatic type conversion
                        // String parameters are passed as pointers to [len|content] structure
                        debug_mir!(
            "DEBUG CALL ARGS: Loading {} arguments for function {:?}",
                            arguments.len(),
                            function_name
                        );

                        // Check if we have function signature to enable automatic type conversion
                        let param_types = function_signature.as_ref().map(|sig| &sig.parameters);

                        for (i, arg) in arguments.iter().enumerate() {
                            debug_mir!(" CALL ARGS:   Arg[{}]: {:?}", i, arg);
                            self.load_operand(arg)?;

                            // Automatic type conversion: if parameter expects f64 but we have i32, convert
                            if let Some(params) = param_types {
                                if i < params.len() {
                                    let expected_param = &params[i];

                                    // Check if parameter expects f64
                                    if matches!(expected_param.param_type, MirType::F64) {
                                        // Check if argument is integer type
                                        let arg_is_int = match arg {
                                            MirOperand::Constant(MirConstant::Integer(_)) => true,
                                            MirOperand::Value(value_id) => self
                                                .value_to_type
                                                .get(value_id)
                                                .is_some_and(|t| {
                                                    matches!(
                                                        t,
                                                        MirType::I32
                                                            | MirType::I8
                                                            | MirType::I16
                                                            | MirType::U8
                                                            | MirType::U16
                                                            | MirType::U32
                                                    )
                                                }),
                                            _ => false,
                                        };

                                        if arg_is_int {
                                            debug_mir!(
            "DEBUG CALL ARGS:   Converting i32 arg[{}] to f64",
                                                i
                                            );
                                            self.current_instructions
                                                .push(Instruction::F64ConvertI32S);
                                        }
                                    }

                                    // Check if parameter expects Any type
                                    // For now, Any type accepts any i32 value (integer, boolean, pointer)
                                    // f64 values need to be converted to i32 (truncated)
                                    if matches!(expected_param.param_type, MirType::Any) {
                                        // Get the argument's actual type
                                        let arg_type = match arg {
                                            MirOperand::Constant(MirConstant::Float(_)) => Some(MirType::F64),
                                            MirOperand::Value(value_id) => self.value_to_type.get(value_id).cloned(),
                                            _ => None,
                                        };

                                        // If argument is f64, convert to i32 (truncate)
                                        // This is a limitation - proper boxing would preserve the f64
                                        if let Some(ref actual_type) = arg_type {
                                            if matches!(actual_type, MirType::F64) {
                                                debug_mir!(
                "DEBUG CALL ARGS:   Converting f64 arg[{}] to i32 for any type",
                                                    i
                                                );
                                                self.current_instructions.push(Instruction::I32TruncF64S);
                                            }
                                        }
                                    }
                                }
                            }

                            debug_mir!(" CALL ARGS:   Arg[{}] loaded successfully", i);
                        }
                        debug_mir!(
            "DEBUG CALL ARGS: Finished loading all {} arguments",
                            arguments.len()
                        );
                    }
                }

                // Generate function call (skip if already emitted for multi-arg print)
                if !call_already_emitted {
                    match function {
                        MirOperand::Function(symbol_id) => {
                            // CRITICAL FIX: Try direct SymbolId -> index lookup first
                            // This avoids name collisions for constructors/methods with same names
                            if let Some(&function_index) =
                                self.symbol_to_function_index.get(symbol_id)
                            {
                                debug_mir!(
                                    "DEBUG DIRECT LOOKUP: SymbolId({}) -> WASM index {} (DIRECT)",
                                    symbol_id.0,
                                    function_index
                                );
                                tracing::trace!(
                                    symbol_id = symbol_id.0,
                                    index = function_index,
                                    "Calling function at WASM index (direct lookup)"
                                );
                                self.current_instructions
                                    .push(Instruction::Call(function_index));
                            } else if let Some(function_name) =
                                self.get_function_name_by_symbol(*symbol_id)
                            {
                                // Fallback to name-based lookup for built-in functions
                                debug_mir!(
                                    "DEBUG LOOKUP: Looking up function '{}' in function_map",
                                    function_name
                                );

                                // Try direct lookup first
                                let function_index = if let Some(&idx) =
                                    self.wasm_generator.function_map.get(&function_name)
                                {
                                    Some(idx)
                                } else {
                                    // CRITICAL FIX: Try underscore/dot conversion first
                                    // "math_round" -> "math.round" or vice versa
                                    let alt_name = if function_name.contains('_') {
                                        function_name.replace('_', ".")
                                    } else if function_name.contains('.') {
                                        function_name.replace('.', "_")
                                    } else {
                                        String::new()
                                    };

                                    if !alt_name.is_empty() {
                                        if let Some(&idx) =
                                            self.wasm_generator.function_map.get(&alt_name)
                                        {
                                            debug_mir!(
                                                "DEBUG LOOKUP FALLBACK: Found '{}' as '{}'",
                                                function_name,
                                                alt_name
                                            );
                                            Some(idx)
                                        } else {
                                            // Try namespace-prefixed variants for builtin functions
                                            // If "min" is not found, try "math.min", "string.min", etc.
                                            let namespaces = [
                                                "math",
                                                "string",
                                                "list",
                                                "file",
                                                "http",
                                                "compare",
                                                "conditional",
                                            ];
                                            namespaces.iter().find_map(|ns| {
                                                let qualified_name =
                                                    format!("{}.{}", ns, function_name);
                                                debug_mir!(
                                                    "DEBUG LOOKUP FALLBACK: Trying '{}'",
                                                    qualified_name
                                                );
                                                self.wasm_generator
                                                    .function_map
                                                    .get(&qualified_name)
                                                    .copied()
                                            })
                                        }
                                    } else {
                                        // Try namespace-prefixed variants for builtin functions
                                        let namespaces = [
                                            "math",
                                            "string",
                                            "list",
                                            "file",
                                            "http",
                                            "compare",
                                            "conditional",
                                        ];
                                        namespaces.iter().find_map(|ns| {
                                            let qualified_name =
                                                format!("{}.{}", ns, function_name);
                                            debug_mir!(
                                                "DEBUG LOOKUP FALLBACK: Trying '{}'",
                                                qualified_name
                                            );
                                            self.wasm_generator
                                                .function_map
                                                .get(&qualified_name)
                                                .copied()
                                        })
                                    }
                                };

                                if let Some(function_index) = function_index {
                                    tracing::trace!(
                                        name = %function_name,
                                        index = function_index,
                                        "Calling function at WASM index"
                                    );
                                    self.current_instructions
                                        .push(Instruction::Call(function_index));
                                } else {
                                    // CRITICAL FIX: No more silent fallbacks to index 0
                                    // Return a proper error when function is not found in function_map
                                    debug_mir!(
                                        "DEBUG LOOKUP: Function '{}' not found in function_map!",
                                        function_name
                                    );
                                    debug_mir!(
                                        "DEBUG LOOKUP: function_map keys: {:?}",
                                        self.wasm_generator.function_map.keys().collect::<Vec<_>>()
                                    );
                                    return Err(CompilerError::Codegen {
                                    context: Box::new(crate::error::ErrorContext::new(
                                        format!(
                                            "Function '{}' (SymbolId({})) not found in function map during code generation",
                                            function_name, symbol_id.0
                                        ),
                                        None,
                                        crate::error::ErrorType::Codegen,
                                        Some(instruction.location.clone()),
                                    )),
                                });
                                }
                            } else {
                                // CRITICAL FIX: No more silent fallbacks to index 0
                                // Return a proper error when symbol ID cannot be resolved to a function name
                                return Err(CompilerError::Codegen {
                                context: Box::new(crate::error::ErrorContext::new(
                                    format!(
                                        "Cannot resolve SymbolId({}) to function name during code generation",
                                        symbol_id.0
                                    ),
                                    None,
                                    crate::error::ErrorType::Codegen,
                                    Some(instruction.location.clone()),
                                )),
                            });
                            }
                        }
                        MirOperand::NamedFunction { name, symbol_id: _ } => {
                            // CRITICAL FIX: Handle namespace functions (math.*, string.*) by looking up by name
                            debug_mir!(
                                "DEBUG NAMED FUNCTION: Looking up function '{}' in function_map",
                                name
                            );

                            // Try direct lookup first
                            let function_index =
                                if let Some(&idx) = self.wasm_generator.function_map.get(name) {
                                    Some(idx)
                                } else {
                                    // CRITICAL FIX: Try underscore/dot conversion
                                    // "input.integer" -> "input_integer" or vice versa
                                    let alt_name = if name.contains('.') {
                                        name.replace('.', "_")
                                    } else if name.contains('_') {
                                        name.replace('_', ".")
                                    } else {
                                        String::new()
                                    };

                                    if !alt_name.is_empty() {
                                        debug_mir!(
                                        "DEBUG NAMED FUNCTION FALLBACK: Trying alternate name '{}'",
                                        alt_name
                                    );
                                        self.wasm_generator.function_map.get(&alt_name).copied()
                                    } else {
                                        None
                                    }
                                };

                            if let Some(idx) = function_index {
                                debug_mir!(
                                    "DEBUG NAMED FUNCTION CALL: Calling '{}' at WASM index {}",
                                    name,
                                    idx
                                );
                                tracing::trace!(
                                    name = %name,
                                    index = idx,
                                    "Calling named function at WASM index"
                                );
                                self.current_instructions.push(Instruction::Call(idx));
                            } else {
                                // CRITICAL FIX: Return a proper error when named function is not found
                                debug_mir!(
                                "DEBUG NAMED FUNCTION: Function '{}' not found in function_map!",
                                name
                            );
                                debug_mir!(
                                    "DEBUG NAMED FUNCTION: Available functions: {:?}",
                                    self.wasm_generator.function_map.keys().collect::<Vec<_>>()
                                );
                                return Err(CompilerError::Codegen {
                                    context: Box::new(crate::error::ErrorContext::new(
                                        format!("Function '{}' not found in function map", name),
                                        None,
                                        crate::error::ErrorType::Codegen,
                                        Some(instruction.location.clone()),
                                    )),
                                });
                            }
                        }
                        _ => {
                            return Err(CompilerError::Codegen {
                                context: Box::new(crate::error::ErrorContext::new(
                                    "Indirect function calls not yet supported",
                                    None,
                                    crate::error::ErrorType::Codegen,
                                    Some(instruction.location.clone()),
                                )),
                            });
                        }
                    }
                } // End of if !call_already_emitted

                // CRITICAL FIX: Handle return values based on function signature
                debug_mir!(" CALL: Call operation completed");
                debug_mir!(
                    "DEBUG CALL: function_name={:?}, has_dest={}",
                    function_name,
                    instruction.dest.is_some()
                );

                if let Some(dest) = instruction.dest {
                    debug_mir!(" CALL DEST: Processing call with dest={:?}", dest);
                    debug_mir!(" CALL DEST: function_name={:?}", function_name);

                    // NOTE: Type conversion (F64 to I32) is handled by store_to_local_with_conversion
                    // which is called in the signature/stdlib handling below. DO NOT add redundant
                    // conversion here - it causes double truncation errors.

                    if let Some(signature) = &function_signature {
                        // CRITICAL FIX: Check if dest_type is Ptr(Void) from Unknown types
                        // If instruction has a dest, the function returns a value
                        let dest_type = self.value_to_type.get(&dest);
                        let is_ptr_void = matches!(dest_type, Some(MirType::Ptr(inner)) if matches!(**inner, MirType::Void));

                        if is_ptr_void {
                            // Ptr(Void) dest_type means Unknown type - drop the actual return values
                            // Use the ACTUAL signature return type to determine how many values to drop
                            debug_mir!(
                                " SIG VOID: Unknown type dest {:?}, signature return type: {:?}",
                                dest,
                                signature.return_type
                            );

                            // CRITICAL FIX: Check if this is a known void function by name first
                            // list.set and list.clear are void functions (they modify in-place)
                            // NOTE: list.add is NOT void - it returns the modified list per specification
                            let is_known_void_by_name = function_name.as_deref()
                                == Some("list.set")
                                || function_name.as_deref() == Some("list.clear")
                                || function_name.as_deref() == Some("list.push");

                            if is_known_void_by_name {
                                debug_mir!(
                                    "DEBUG SIG VOID: Known void function by name - no DROP needed"
                                );
                            } else {
                                match &signature.return_type {
                                    MirType::Void => {
                                        // Function truly returns nothing - no DROP needed
                                        debug_mir!(
            "DEBUG SIG VOID: Function returns Void - no drop needed"
                                        );
                                    }
                                    _ => {
                                        // CRITICAL FIX: Ptr(Void) represents Any type which CAN hold return values
                                        // Store the value to the local - Any type can store any pointer/value
                                        // Previously this was incorrectly dropping the value
                                        debug_mir!(
                                            "DEBUG SIG VOID: Storing return value (type: {:?}) to Any type dest",
                                            signature.return_type
                                        );
                                        self.store_to_local_with_conversion(
                                            dest,
                                            Some(signature.return_type.clone()),
                                        )?;
                                    }
                                }
                            }
                        } else {
                            match &signature.return_type {
                                MirType::Void => {
                                    // CRITICAL FIX: Void return type in signature means no value on stack
                                    // No DROP needed - the function truly returns nothing
                                    tracing::trace!(
                                        function_name = ?function_name,
                                        "Void function - no return value to store or drop"
                                    );
                                }
                                MirType::StringTuple => {
                                    // StringTuple functions return a SINGLE i32 pointer
                                    // The pointer references memory formatted as: [4-byte length][content bytes]
                                    // Just store the pointer directly - no Drop needed
                                    tracing::trace!(
                                        function_name = ?function_name,
                                        "Handling StringTuple return (storing single i32 pointer)"
                                    );

                                    self.store_to_local_with_conversion(
                                        dest,
                                        Some(signature.return_type.clone()),
                                    )?;

                                    tracing::trace!(
                                        "Stored StringTuple return as single i32 pointer"
                                    );
                                }
                                _ => {
                                    // Regular single-value return - with type conversion if needed
                                    // CRITICAL FIX: Pass return type for automatic f64->i32 conversion
                                    self.store_to_local_with_conversion(
                                        dest,
                                        Some(signature.return_type.clone()),
                                    )?;
                                }
                            }
                        }
                    } else {
                        // Fallback: no signature available from SymbolId lookup
                        // CRITICAL FIX: Try looking up return type by function name for stdlib functions
                        let stdlib_return_type = function_name
                            .as_ref()
                            .and_then(|name| self.get_stdlib_return_type(name));

                        if let Some(return_type) = stdlib_return_type {
                            // Found stdlib return type - use it for type conversion
                            if !matches!(return_type, MirType::Void) {
                                debug_mir!(
                                    "DEBUG STDLIB: Found return type {:?} for function {:?}",
                                    return_type,
                                    function_name
                                );
                                self.store_to_local_with_conversion(dest, Some(return_type))?;
                            }
                            // Void functions don't store anything
                        } else if let Some(dest_type) = self.value_to_type.get(&dest) {
                            debug_mir!(
                                "DEBUG VOID CHECK: dest={:?}, dest_type={:?}, function={:?}",
                                dest,
                                dest_type,
                                function_name
                            );
                            let is_ptr_void = matches!(dest_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void));

                            // CRITICAL FIX: Ptr(Void) represents Any type which can hold any value
                            // Store the return value regardless of whether dest is Ptr(Void) or not
                            if is_ptr_void {
                                // Ptr(Void) means Any type - store the value (not drop!)
                                debug_mir!(
                                    "DEBUG VOID DEST: Storing value to Any type dest {:?}",
                                    dest
                                );
                            }
                            // Both Ptr(Void) and other types should store the return value
                            self.store_to_local(dest)?;
                        } else {
                            debug_mir!(
                                "DEBUG VOID CHECK: dest={:?} not found in value_to_type",
                                dest
                            );
                            // Last resort: check if this is a known void-returning built-in function
                            if let Some(function_name) = &function_name {
                                if function_name == "testFunction"
                                    || function_name == "print"
                                    || function_name == "printl"
                                    || function_name == "println"
                                    || function_name == "list.set"
                                    || function_name == "list.clear"
                                {
                                    tracing::trace!(
                                        name = %function_name,
                                        "Skipping return value store for known void function"
                                    );
                                } else {
                                    self.store_to_local(dest)?;
                                }
                            } else {
                                self.store_to_local(dest)?;
                            }
                        }
                    }
                } else {
                    // CRITICAL FIX: Handle calls with no destination (expression statements)
                    // For non-void functions, we need to DROP the return value to clean up the stack
                    debug_mir!(" CALL NO DEST: Call has no destination, checking if return value needs to be dropped");

                    // Check if this function returns void (no cleanup needed)
                    let is_void_return = if let Some(signature) = &function_signature {
                        debug_mir!(
                            "DEBUG CALL NO DEST: Found signature, return_type={:?}",
                            signature.return_type
                        );
                        // CRITICAL FIX: Check for both Void and Ptr(Void)
                        // Ptr(Void) represents a void function (no return value)
                        matches!(signature.return_type, MirType::Void)
                            || matches!(&signature.return_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void))
                    } else {
                        debug_mir!(
                            "DEBUG CALL NO DEST: No signature found, checking fallback logic"
                        );
                        // Fallback: check known void functions by name
                        // These are builtin/stdlib functions that return nothing (modify in-place or have side effects only)
                        // NOTE: list.push is NOT void - it returns the list for chaining
                        let is_known_void_builtin = match function_name.as_deref() {
                            Some("print") | Some("printl") | Some("println") => true,
                            Some("list.set") | Some("list.clear") => true,
                            Some("mem_release") | Some("mem_retain") => true,
                            Some("mem_scope_push") | Some("mem_scope_pop") => true, // Scope-based memory management
                            _ => false,
                        };

                        if is_known_void_builtin {
                            debug_mir!(
                                " CALL NO DEST: Known void built-in function: {:?}",
                                function_name
                            );
                            true
                        } else {
                            // CRITICAL FIX: For functions without signatures called as expression statements,
                            // default to NON-VOID (add DROP) to prevent stack pollution
                            // This is safer than defaulting to void because:
                            // 1. Leaving values on stack causes WASM validation errors
                            // 2. Adding DROP for void function would cause immediate error (helps catch bugs)
                            // 3. Most builtins (like list.add) don't have registered signatures
                            debug_mir!(" CALL NO DEST: Unknown function without signature, defaulting to non-void (adding DROP for safety)");
                            false
                        }
                    };

                    if !is_void_return {
                        debug_mir!(" CALL NO DEST: Non-void function, adding DROP instruction");
                        // Function returns a value but we're not using it (expression statement)
                        // Drop the return value from the stack
                        self.current_instructions.push(Instruction::Drop);
                        tracing::trace!(
                            function_name = ?function_name,
                            "Dropped unused return value for call with no destination"
                        );
                    } else {
                        debug_mir!(" CALL NO DEST: Void function, no DROP needed");
                        tracing::trace!(
                            function_name = ?function_name,
                            "No DROP needed for void function call"
                        );
                    }
                }

                debug_mir!("DEBUG MIR: Call operation processing completed");
            }

            MirOperation::GetElementPtr {
                base,
                indices,
                is_array,
            } => {
                debug_mir!(
                    " CODEGEN GEP: base={:?}, indices={:?}, is_array={}",
                    base,
                    indices,
                    is_array
                );
                debug_mir!(
                    "DEBUG CODEGEN GEP: value_to_local map has {} entries",
                    self.value_to_local.len()
                );
                if let MirOperand::Value(vid) = base {
                    debug_mir!(
                        "DEBUG CODEGEN GEP: Looking for base ValueId({}) in value_to_local",
                        vid.0
                    );
                    if self.value_to_local.contains_key(vid) {
                        debug_mir!(
                            "DEBUG CODEGEN GEP: Base ValueId({}) FOUND in value_to_local",
                            vid.0
                        );
                    } else {
                        debug_mir!(
                            "DEBUG CODEGEN GEP: Base ValueId({}) NOT FOUND in value_to_local!",
                            vid.0
                        );
                    }
                }

                tracing::trace!(
                    base = ?base,
                    indices = ?indices,
                    is_array = is_array,
                    "Processing GetElementPtr"
                );

                // Get element pointer for array/struct access
                match self.load_operand(base) {
                    Ok(_) => debug_mir!("Base operand loaded successfully"),
                    Err(e) => {
                        debug_mir!(error = ?e, "Failed to load base operand");
                        return Err(e);
                    }
                }

                // For each index, load it and generate pointer arithmetic
                for (i, index) in indices.iter().enumerate() {
                    debug_mir!(index_num = i, index = ?index, "Processing index");
                    match self.load_operand(index) {
                        Ok(_) => {
                            debug_mir!(index_num = i, "Index loaded successfully");
                            // Calculate element address
                            if *is_array {
                                // For arrays: multiply index by 4 (element size) and add header
                                // Array elements are at array_ptr + 16 + (index * 4)
                                self.current_instructions.push(Instruction::I32Const(4));
                                self.current_instructions.push(Instruction::I32Mul);
                                self.current_instructions.push(Instruction::I32Add);
                                // Clean Language array layout:
                                //   Offset 0-3: Type marker (0)
                                //   Offset 4-7: Array length (i32)
                                //   Offset 8-11: Element size (4)
                                //   Offset 12-15: Unused
                                //   Offset 16+: Elements start here
                                self.current_instructions.push(Instruction::I32Const(16));
                                self.current_instructions.push(Instruction::I32Add);
                            } else {
                                // For class fields: the index IS the byte offset (already calculated)
                                // Just add it directly to the base pointer
                                self.current_instructions.push(Instruction::I32Add);
                            }
                        }
                        Err(e) => {
                            debug_mir!(index_num = i, error = ?e, "Failed to load index");
                            return Err(e);
                        }
                    }
                }

                // Store the calculated address to destination
                if let Some(dest) = instruction.dest {
                    debug_mir!(dest = ?dest, "Storing result to destination");
                    match self.store_to_local(dest) {
                        Ok(_) => debug_mir!("GetElementPtr completed successfully"),
                        Err(e) => {
                            debug_mir!(error = ?e, "Failed to store to destination");
                            return Err(e);
                        }
                    }
                } else {
                    debug_mir!("No destination for GetElementPtr result");
                }
            }

            MirOperation::AsyncAssign { source } => {
                debug_mir!(source = ?source, "Processing AsyncAssign");

                // For async assignments, we load the source value and store it
                // In a full async implementation, this would involve setting up async state
                // For now, we treat it as a regular assignment with future resolution semantics
                self.load_operand(source)?;

                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("AsyncAssign completed successfully");
                } else {
                    // No destination - drop the value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("AsyncAssign: No destination, dropped result");
                }
            }

            MirOperation::BoxAny {
                value,
                type_tag,
                source_type,
            } => {
                debug_mir!(?value, ?type_tag, ?source_type, "Processing BoxAny");

                // Load the value onto the stack
                self.load_operand(value)?;

                // Call the boxing helper
                self.emit_box_value(*type_tag, source_type)?;

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("BoxAny completed successfully");
                } else {
                    // No destination - drop the boxed value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("BoxAny: No destination, dropped result");
                }
            }

            MirOperation::AnyToString { value } => {
                debug_mir!(?value, "Processing AnyToString with type dispatch");

                // Load the boxed any pointer onto the stack
                self.load_operand(value)?;

                // Call the any_to_string helper which does type dispatch
                self.emit_any_to_string()?;

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("AnyToString completed successfully");
                } else {
                    // No destination - drop the string pointer to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("AnyToString: No destination, dropped result");
                }
            }

            MirOperation::UnboxAnyToI32 { value } => {
                debug_mir!(?value, "Processing UnboxAnyToI32");

                // Load the boxed any pointer onto the stack
                self.load_operand(value)?;

                // Unbox to i32
                self.emit_unbox_to_i32()?;

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("UnboxAnyToI32 completed successfully");
                } else {
                    // No destination - drop the unboxed value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("UnboxAnyToI32: No destination, dropped result");
                }
            }

            MirOperation::UnboxAnyToF64 { value } => {
                debug_mir!(?value, "Processing UnboxAnyToF64");

                // Load the boxed any pointer onto the stack
                self.load_operand(value)?;

                // Unbox to f64
                self.emit_unbox_to_f64()?;

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("UnboxAnyToF64 completed successfully");
                } else {
                    // No destination - drop the unboxed value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("UnboxAnyToF64: No destination, dropped result");
                }
            }

            MirOperation::AnyGetField { object, key } => {
                debug_mir!(?object, ?key, "Processing AnyGetField (JSON object access)");

                // Load the JSON object pointer (Any type)
                self.load_operand(object)?;

                // CRITICAL FIX: Objects are now boxed as [tag][raw_ptr][0]
                // We need to unbox by reading the raw object pointer at offset 4
                // This extracts the actual object structure pointer from the boxed any value
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                // Load the key string and expand to (content_ptr, len) format
                // __json_get_field expects: (raw_object_ptr: i32, key_ptr: i32, key_len: i32)
                self.load_string_argument_for_print(key)?;

                // Call __json_get_field(raw_object_ptr: i32, key_ptr: i32, key_len: i32) -> i32
                let json_get_field_idx = self.get_or_register_json_get_field()?;
                self.current_instructions
                    .push(Instruction::Call(json_get_field_idx));

                // Store result (Any pointer to field value or null) if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("AnyGetField completed successfully");
                } else {
                    // No destination - drop the field value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("AnyGetField: No destination, dropped result");
                }
            }

            MirOperation::AnyGetIndex { array, index } => {
                debug_mir!(?array, ?index, "Processing AnyGetIndex (JSON array access)");

                // Load the JSON array pointer (Any type)
                self.load_operand(array)?;

                // CRITICAL FIX: Arrays are now boxed as [tag][raw_ptr][0]
                // We need to unbox by reading the raw array pointer at offset 4
                // This extracts the actual array structure pointer from the boxed any value
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                // Load the integer index
                self.load_operand(index)?;

                // Call __json_get_index(raw_array_ptr: i32, index: i32) -> i32
                let json_get_index_idx = self.get_or_register_json_get_index()?;
                self.current_instructions
                    .push(Instruction::Call(json_get_index_idx));

                // Store result (Any pointer to element or null) if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("AnyGetIndex completed successfully");
                } else {
                    // No destination - drop the element pointer to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("AnyGetIndex: No destination, dropped result");
                }
            }

            MirOperation::Alloca { size, alignment: _ } => {
                debug_mir!(size = ?size, "Processing Alloca - converting to mem_alloc call");

                // Allocate heap memory by calling mem_alloc
                // mem_alloc signature: (type_id: i32, size: i32) -> i32 (pointer)
                // For class instances, we use type_id = 0 (generic object)

                // Push type_id argument (0 for generic allocation)
                self.current_instructions.push(Instruction::I32Const(0));

                // Push size argument
                self.load_operand(size)?;

                // Get mem_alloc function index from function_map
                let mem_alloc_idx = *self
                    .wasm_generator
                    .function_map
                    .get("mem_alloc")
                    .ok_or_else(|| CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "mem_alloc function not found in function_map".to_string(),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(instruction.location.clone()),
                        )),
                    })?;

                // Call mem_alloc
                self.current_instructions
                    .push(Instruction::Call(mem_alloc_idx));

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("Alloca completed successfully, stored to {:?}", dest);
                } else {
                    // No destination - drop the allocated pointer to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("Alloca: No destination, dropped result");
                }
            }

            MirOperation::Cast { value, target_type } => {
                debug_mir!(value = ?value, target_type = ?target_type, "Processing Cast");

                // Get the source type by checking value_to_type or inferring from operand
                let source_type = if let MirOperand::Value(vid) = value {
                    self.value_to_type.get(vid).cloned()
                } else {
                    None
                };

                // Load the value onto the stack
                self.load_operand(value)?;

                // Generate appropriate conversion instruction
                match (source_type.as_ref(), target_type) {
                    // Integer to Float conversions
                    (Some(MirType::I32), MirType::F64) | (None, MirType::F64) => {
                        // Convert i32 to f64 (signed conversion)
                        self.current_instructions.push(Instruction::F64ConvertI32S);
                        debug_mir!("Cast: I32 -> F64 using F64ConvertI32S");
                    }

                    // Float to Integer conversions
                    (Some(MirType::F64), MirType::I32) => {
                        // Convert f64 to i32 (truncate)
                        self.current_instructions.push(Instruction::I32TruncF64S);
                        debug_mir!("Cast: F64 -> I32 using I32TruncF64S");
                    }

                    // Same type - no conversion needed
                    (Some(MirType::I32), MirType::I32) | (Some(MirType::F64), MirType::F64) => {
                        debug_mir!("Cast: Same type, no conversion needed");
                    }

                    // Pointer casts - treat as no-op in WASM (all pointers are i32)
                    (Some(MirType::Ptr(_)), MirType::Ptr(_)) => {
                        debug_mir!("Cast: Pointer to pointer, no conversion needed");
                    }

                    // Default: log warning but don't fail
                    _ => {
                        debug_mir!(
                            source = ?source_type,
                            target = ?target_type,
                            "Cast: Unknown type conversion, treating as no-op"
                        );
                    }
                }

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("Cast completed successfully, stored to {:?}", dest);
                } else {
                    // No destination - drop the casted value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("Cast: No destination, dropped result");
                }
            }

            MirOperation::Select {
                condition,
                true_value,
                false_value,
            } => {
                debug_mir!(
                    condition = ?condition,
                    true_value = ?true_value,
                    false_value = ?false_value,
                    "Processing Select operation"
                );

                // WASM select instruction semantics:
                // Pop: condition (i32), val2, val1 (in that order from stack top)
                // Push: val1 if condition != 0, else val2
                //
                // So we push in order: true_value (val1), false_value (val2), condition
                // Result: if condition is true (non-zero), true_value is returned
                //         if condition is false (zero), false_value is returned
                self.load_operand(true_value)?;
                self.load_operand(false_value)?;
                self.load_operand(condition)?;
                self.current_instructions.push(Instruction::Select);

                // Store result if there's a destination
                if let Some(dest) = instruction.dest {
                    self.store_to_local(dest)?;
                    debug_mir!("Select completed, stored to {:?}", dest);
                } else {
                    // No destination - drop the selected value to avoid stack pollution
                    self.current_instructions.push(Instruction::Drop);
                    debug_mir!("Select: No destination, dropped result");
                }
            }

            _ => {
                // Unsupported MIR operation
                return Err(CompilerError::Codegen {
                    context: Box::new(crate::error::ErrorContext::new(
                        format!(
                            "MIR operation not yet implemented: {:?}",
                            instruction.operation
                        ),
                        None,
                        crate::error::ErrorType::Codegen,
                        Some(instruction.location.clone()),
                    )),
                });
            }
        }

        Ok(())
    }

    /// Generate WASM terminator instruction
    #[allow(dead_code)] // Used internally by generate_basic_block
    fn generate_terminator(&mut self, terminator: &MirTerminator) -> Result<(), CompilerError> {
        match terminator {
            MirTerminator::Return { value } => {
                if let Some(return_value) = value {
                    // Don't load undefined values - they represent void returns
                    if !matches!(return_value, MirOperand::Constant(MirConstant::Undefined)) {
                        // CRITICAL FIX: Removed StringTuple expansion logic
                        // Since ConcreteType::String now maps to MirType::I32, strings are single i32 pointers
                        // No expansion needed - just load the operand directly
                        self.load_operand(return_value)?;
                    }
                }
                self.current_instructions.push(Instruction::Return);
            }

            MirTerminator::Jump { target } => {
                // Fallthrough to next block (structured control flow handled by block ordering)
                debug_mir!("DEBUG MIR: Skipping Jump to {:?} (fallthrough)", target);
            }

            MirTerminator::Branch {
                condition,
                true_block,
                false_block,
            } => {
                // Evaluate condition (if/else structure handled by generate_branch_block)
                self.load_operand(condition)?;
                // Pop the condition value since we're not using it
                self.current_instructions.push(Instruction::Drop);
                debug_mir!(
                    "DEBUG MIR: Skipping Branch to {:?}/{:?} (fallthrough)",
                    true_block,
                    false_block
                );
            }

            MirTerminator::Unreachable => {
                self.current_instructions.push(Instruction::Unreachable);
            }
        }

        Ok(())
    }

    /// Load MIR operand onto WASM stack
    fn load_operand(&mut self, operand: &MirOperand) -> Result<(), CompilerError> {
        match operand {
            MirOperand::Value(value_id) => {
                if let Some(&local_index) = self.value_to_local.get(value_id) {
                    // For StringTuple, just load the single i32 pointer
                    // It will be stored as a pointer to memory with format: [4-byte length][content bytes]
                    // Functions that need (ptr, len) will handle expansion themselves (e.g., load_string_argument_for_print)
                    self.current_instructions
                        .push(Instruction::LocalGet(local_index));
                } else {
                    // CRITICAL FIX: No more silent auto-allocation of missing ValueIds
                    // Return a proper error to surface MIR builder bugs
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            format!(
                                "ValueId({:?}) not found in local variable map during load_operand. \
                                This indicates the MIR builder did not properly track this value.",
                                value_id.0
                            ),
                            None,
                            crate::error::ErrorType::Codegen,
                            None,
                        )),
                    });
                }
            }

            MirOperand::Constant(constant) => {
                self.load_constant(constant)?;
            }

            MirOperand::Function(_symbol_id) => {
                // Function reference placeholder (WASM funcref tables not yet used)
                self.current_instructions.push(Instruction::I32Const(0));
            }

            MirOperand::NamedFunction { .. } => {
                // Named function reference placeholder (resolved at call site)
                self.current_instructions.push(Instruction::I32Const(0));
            }

            MirOperand::Global(_symbol_id) => {
                // Global variable access (module globals indexed from 0)
                self.current_instructions.push(Instruction::GlobalGet(0));
            }
        }

        Ok(())
    }

    /// Load MIR constant onto WASM stack
    fn load_constant(&mut self, constant: &MirConstant) -> Result<(), CompilerError> {
        match constant {
            MirConstant::Integer(i) => {
                // Clean Language integers map to WASM i32, not i64
                self.current_instructions
                    .push(Instruction::I32Const(*i as i32));
            }
            MirConstant::Float(f) => {
                self.current_instructions.push(Instruction::F64Const(*f));
            }
            MirConstant::Boolean(b) => {
                self.current_instructions
                    .push(Instruction::I32Const(if *b { 1 } else { 0 }));
            }
            MirConstant::String(index) => {
                // CRITICAL FIX: Load the string structure base offset (not index, not content offset)
                // String format in memory: [4-byte len][content]
                // Load base offset - points to the length field
                // The generate_terminator will expand this to (content_ptr, len) if needed
                if let Some(string_pool) = &self.string_pool {
                    if let Some(string_content) = string_pool.get(*index) {
                        let base_offset = self
                            .wasm_generator
                            .get_or_create_string_offset(string_content)?;
                        tracing::trace!(
                            index = index,
                            content = %string_content,
                            base_offset = base_offset,
                            "Loading string constant at base offset (points to [len|content] structure)"
                        );
                        self.current_instructions
                            .push(Instruction::I32Const(base_offset as i32));
                    } else {
                        return Err(CompilerError::Codegen {
                            context: Box::new(
                                crate::error::ErrorContext::new(
                                    format!("String index {} not found in string pool", index),
                                    None,
                                    crate::error::ErrorType::Codegen,
                                    None,
                                )
                                .with_error_code("E007"),
                            ),
                        });
                    }
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(
                            crate::error::ErrorContext::new(
                                "No string pool available for string constant",
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )
                            .with_error_code("E007"),
                        ),
                    });
                }
            }
            MirConstant::Null => {
                self.current_instructions.push(Instruction::I32Const(0));
            }
            MirConstant::Undefined => {
                // Undefined values are represented as 0
                self.current_instructions.push(Instruction::I32Const(0));
            }
            _ => {
                return Err(CompilerError::Codegen {
                    context: Box::new(crate::error::ErrorContext::new(
                        format!("Constant type not yet implemented: {:?}", constant),
                        None,
                        crate::error::ErrorType::Codegen,
                        Some(crate::ast::SourceLocation::default()),
                    )),
                });
            }
        }

        Ok(())
    }

    /// Load string pointer only (for input functions that don't need length)
    fn load_string_pointer_only(&mut self, operand: &MirOperand) -> Result<(), CompilerError> {
        tracing::trace!(
            operand = ?operand,
            "load_string_pointer_only called"
        );
        match operand {
            MirOperand::Constant(MirConstant::String(index)) => {
                // For string constants, push the pointer to the string structure
                if let Some(string_pool) = &self.string_pool {
                    if let Some(string_content) = string_pool.get(*index) {
                        let data_offset = self
                            .wasm_generator
                            .get_or_create_string_offset(string_content)?;
                        // Push pointer to string structure (includes length prefix)
                        self.current_instructions
                            .push(Instruction::I32Const(data_offset as i32));
                    } else {
                        return Err(CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                format!("String constant {} not found in string pool", index),
                                None,
                                crate::error::ErrorType::Codegen,
                                Some(crate::ast::SourceLocation::default()),
                            )),
                        });
                    }
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "String pool not initialized for input function call".to_string(),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(crate::ast::SourceLocation::default()),
                        )),
                    });
                }
            }
            MirOperand::Value(_value_id) => {
                // CRITICAL FIX: Always load values from their local variable, never use
                // cached string constant offsets. The value_to_string_index mapping only
                // records the INITIAL assignment - if the variable was reassigned (e.g.,
                // html = html + "..."), we need to read the CURRENT value from the local.
                // This fixes the string concatenation truncation bug where reassigned
                // string variables would print their original value instead of the new one.
                self.load_operand(operand)?;
            }
            _ => {
                // For other operand types, just load normally
                self.load_operand(operand)?;
            }
        }
        Ok(())
    }

    // NOTE: Full boxing implementation for `any` type is pending.
    // The `any` type currently works with i32 values only (integers, booleans, pointers).
    // f64 values are truncated to i32 when passed to `any` parameters.
    // Proper boxing with type tags and memory allocation will be implemented in a future version.
    // See MirType::Any and AnyTypeTag for the planned memory layout.

    /// Load string argument for print functions (expands to pointer + length)
    fn load_string_argument_for_print(
        &mut self,
        operand: &MirOperand,
    ) -> Result<(), CompilerError> {
        debug_mir!(" LOAD_STRING: Called with operand: {:?}", operand);
        tracing::trace!(
            operand = ?operand,
            "load_string_argument_for_print called"
        );
        match operand {
            MirOperand::Constant(MirConstant::String(index)) => {
                debug_mir!(index = index, "Processing string constant");
                // For string constants, we need to expand to pointer + length
                if let Some(string_pool) = &self.string_pool {
                    if let Some(string_content) = string_pool.get(*index) {
                        debug_mir!(content = %string_content, "Found string content");
                        // Get the string offset in WASM memory using the underlying generator
                        let data_offset = self
                            .wasm_generator
                            .get_or_create_string_offset(string_content)?;
                        let str_len = string_content.len() as i32;
                        tracing::trace!(
                            offset = data_offset,
                            length = str_len,
                            "String offset and length"
                        );

                        // CRITICAL FIX: data_offset points to the string structure [len|content]
                        // We need to skip the 4-byte length prefix to get to the content
                        let content_offset = data_offset + 4;

                        // Push pointer to string content (skipping 4-byte length prefix)
                        self.current_instructions
                            .push(Instruction::I32Const(content_offset as i32));
                        // Push string length
                        self.current_instructions
                            .push(Instruction::I32Const(str_len));
                    } else {
                        return Err(CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                format!("String constant {} not found in string pool", index),
                                None,
                                crate::error::ErrorType::Codegen,
                                Some(crate::ast::SourceLocation::default()),
                            )),
                        });
                    }
                } else {
                    return Err(CompilerError::Codegen {
                        context: Box::new(crate::error::ErrorContext::new(
                            "String pool not initialized for print function call".to_string(),
                            None,
                            crate::error::ErrorType::Codegen,
                            Some(crate::ast::SourceLocation::default()),
                        )),
                    });
                }
            }
            MirOperand::Value(value_id) => {
                debug_mir!(" LOAD_STRING: Processing Value({:?})", value_id);

                // Check the value type to determine if we need type conversion
                let value_type = self.get_value_type(*value_id);
                debug_mir!(" LOAD_STRING: Value type is {:?}", value_type);

                // CRITICAL FIX: For non-string values, we must convert to string first
                // F64 (number type) -> float_to_string
                // I32 (integer type) -> int_to_string
                // Bool (boolean type) -> bool_to_string
                // Otherwise it's a string pointer -> expand normally

                if matches!(value_type, Some(MirType::F64)) {
                    debug_mir!(
                        " LOAD_STRING: Value is f64, converting to string via float_to_string"
                    );
                    // Load the f64 value
                    self.load_operand(operand)?;
                    // Get float_to_string function index
                    let float_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("float_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "float_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call float_to_string(f64) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(float_to_string_idx));

                    // Now we have a string pointer on the stack, expand it
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4)
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else if matches!(value_type, Some(MirType::Ptr(_))) {
                    // CRITICAL FIX: Ptr(U8) values are string pointers - just expand to (content_ptr, length)
                    // This handles the result of toString() calls which return string pointers
                    debug_mir!(
                        " LOAD_STRING: Value is Ptr (string pointer), expanding to (ptr+4, len)"
                    );

                    // Load the string pointer from the local variable
                    self.load_operand(operand)?;

                    // Allocate a temporary local to hold the pointer
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);

                    // Store pointer to temp local
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4, skipping length field)
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory at pointer location
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else if matches!(value_type, Some(MirType::I32)) {
                    // CRITICAL FIX: Integer values need to be converted to strings
                    debug_mir!(" LOAD_STRING: Value is i32 (integer), converting to string via int_to_string");
                    // Load the i32 value
                    self.load_operand(operand)?;
                    // Get int_to_string function index (stdlib function)
                    let int_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("int_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "int_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call int_to_string(i32) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(int_to_string_idx));

                    // Now we have a string pointer on the stack, expand it
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4)
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else if matches!(value_type, Some(MirType::Bool)) {
                    // CRITICAL FIX: Boolean values need to be converted to strings
                    debug_mir!(
                        " LOAD_STRING: Value is bool, converting to string via bool_to_string"
                    );
                    // Load the bool value
                    self.load_operand(operand)?;
                    // Get bool_to_string function index (stdlib function)
                    let bool_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("bool_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "bool_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call bool_to_string(i32) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(bool_to_string_idx));

                    // Now we have a string pointer on the stack, expand it
                    let temp_local = self.next_local_index;
                    self.next_local_index += 1;
                    self.temp_local_types.insert(temp_local, ValType::I32);
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4)
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                } else {
                    // String or other pointer type - load and expand normally
                    // CRITICAL FIX: Always load values from their local variable and expand
                    // to (content_ptr, length). Never use cached value_to_string_index mappings
                    // because the variable may have been reassigned (e.g., html = html + "...").
                    // The mapping only records the INITIAL assignment, not the current value.
                    // This fixes the string concatenation truncation bug.
                    tracing::trace!(
                        value_id = ?value_id.0,
                        "Expanding string pointer for ValueId (always load from local)"
                    );

                    // Load the string pointer from the local variable
                    debug_mir!(" LOAD_STRING: Loading operand to stack");
                    self.load_operand(operand)?;

                    // Allocate a temporary local to hold the pointer
                    let temp_local = self.next_local_index;
                    debug_mir!(" LOAD_STRING: Allocated temp_local={}", temp_local);
                    self.next_local_index += 1;
                    // Track type: string pointers are i32
                    self.temp_local_types.insert(temp_local, ValType::I32);

                    // Store pointer to temp local
                    debug_mir!(" LOAD_STRING: Storing pointer to temp_local");
                    self.current_instructions
                        .push(Instruction::LocalSet(temp_local));

                    // Calculate content pointer (ptr + 4, skipping length field)
                    debug_mir!(" LOAD_STRING: Calculating content_ptr = base_ptr + 4");
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions.push(Instruction::I32Const(4));
                    self.current_instructions.push(Instruction::I32Add);

                    // Load length from memory at pointer location
                    debug_mir!(" LOAD_STRING: Loading length from base_ptr");
                    self.current_instructions
                        .push(Instruction::LocalGet(temp_local));
                    self.current_instructions
                        .push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2, // 4-byte alignment for i32
                            memory_index: 0,
                        }));

                    debug_mir!(" LOAD_STRING: String pointer expansion completed - stack should have [content_ptr, length]");
                }
            }
            _ => {
                // CRITICAL FIX: For non-string operands, we need to convert them to strings first
                // and then expand to (pointer, length) format

                // Check the operand type to determine if we need type conversion
                let operand_type = self.get_operand_mir_type(operand);
                debug_mir!(" LOAD_STRING: Non-string operand type: {:?}", operand_type);

                // Load the operand onto the stack
                self.load_operand(operand)?;

                // CRITICAL: For non-string operands, we must convert to string first
                // F64 (number type) -> float_to_string
                // I32 (integer type) -> int_to_string (but not string pointers)
                // Bool (boolean type) -> bool_to_string
                if matches!(operand_type, Some(MirType::F64)) {
                    debug_mir!(" LOAD_STRING: Converting f64 to string via float_to_string");
                    // Get float_to_string function index
                    let float_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("float_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "float_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call float_to_string(f64) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(float_to_string_idx));
                } else if matches!(operand_type, Some(MirType::I32)) {
                    debug_mir!(" LOAD_STRING: Converting i32 to string via int_to_string");
                    // Get int_to_string function index
                    let int_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("int_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "int_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call int_to_string(i32) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(int_to_string_idx));
                } else if matches!(operand_type, Some(MirType::Bool)) {
                    debug_mir!(" LOAD_STRING: Converting bool to string via bool_to_string");
                    // Get bool_to_string function index
                    let bool_to_string_idx = *self
                        .wasm_generator
                        .function_map
                        .get("bool_to_string")
                        .ok_or_else(|| CompilerError::Codegen {
                            context: Box::new(crate::error::ErrorContext::new(
                                "bool_to_string function not found for print".to_string(),
                                None,
                                crate::error::ErrorType::Codegen,
                                None,
                            )),
                        })?;
                    // Call bool_to_string(i32) -> i32 (string pointer)
                    self.current_instructions
                        .push(Instruction::Call(bool_to_string_idx));
                }
                // For Ptr types and other string pointers, no conversion needed

                // The operand pushed a pointer to a string structure [len|content]
                // We need to expand this to (content_ptr, length) for printl

                // Allocate a temporary local to hold the pointer
                let temp_local = self.next_local_index;
                self.next_local_index += 1;
                // Track type: string pointers are i32
                self.temp_local_types.insert(temp_local, ValType::I32);

                // Store pointer to temp local
                self.current_instructions
                    .push(Instruction::LocalSet(temp_local));

                // Calculate content pointer (ptr + 4, skipping length field)
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions.push(Instruction::I32Const(4));
                self.current_instructions.push(Instruction::I32Add);

                // Load length from memory at pointer location
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2, // 4-byte alignment for i32
                        memory_index: 0,
                    }));
            }
        }
        debug_mir!("DEBUG MIR: load_string_argument_for_print completed successfully");
        Ok(())
    }

    /// Store value from WASM stack to local
    /// Get the type of a ValueId from the current function's locals
    fn get_value_type(&self, value_id: ValueId) -> Option<MirType> {
        self.current_function
            .as_ref()
            .and_then(|func| func.locals.get(&value_id))
            .map(|local| local.local_type.clone())
    }

    /// Get the type of a MirOperand
    fn get_operand_mir_type(&self, operand: &MirOperand) -> Option<MirType> {
        match operand {
            MirOperand::Value(vid) => self.get_value_type(*vid),
            MirOperand::Constant(constant) => Some(match constant {
                MirConstant::Integer(_) => MirType::I32,
                MirConstant::Float(_) => MirType::F64,
                MirConstant::Boolean(_) => MirType::I32,
                MirConstant::String(_) => MirType::I32, // String pointers are i32
                MirConstant::Null => MirType::I32,
                MirConstant::Undefined => MirType::I32, // Undefined is represented as i32
                MirConstant::Array(_) => MirType::I32,  // Array pointers are i32
                MirConstant::Struct(_) => MirType::I32, // Struct pointers are i32
            }),
            MirOperand::Function(_) => Some(MirType::I32), // Function pointers are i32
            MirOperand::NamedFunction { .. } => Some(MirType::I32), // Named function pointers are i32
            MirOperand::Global(_) => Some(MirType::I32), // Global variable pointers are i32
        }
    }

    /// Get the return type for stdlib functions by name
    /// This is used for namespace functions (SymbolId(0)) where signature lookup by ID fails
    fn get_stdlib_return_type(&self, function_name: &str) -> Option<MirType> {
        match function_name {
            // Math functions that return F64
            "math.abs" | "math.sqrt" | "math.sin" | "math.cos" | "math.tan" | "math.asin"
            | "math.acos" | "math.atan" | "math.atan2" | "math.sinh" | "math.cosh"
            | "math.tanh" | "math.ln" | "math.log10" | "math.log2" | "math.exp" | "math.exp2"
            | "math.floor" | "math.ceil" | "math.round" | "math.trunc" | "math.sign"
            | "math.pow" | "math.max" | "math.min" | "math.pi" | "math.e" | "math.tau" => {
                Some(MirType::F64)
            }
            // Math functions that return I32
            "math.abs.i32" => Some(MirType::I32),

            // CRITICAL FIX: Type conversion methods - these must be checked BEFORE generic patterns
            // .toNumber() methods always return F64
            "string.toNumber" | "integer.toNumber" | "boolean.toNumber" | "number.toNumber" => {
                Some(MirType::F64)
            }
            // .toInteger() methods always return I32
            "string.toInteger" | "number.toInteger" | "boolean.toInteger" => Some(MirType::I32),
            // .toBoolean() methods always return I32 (boolean is i32 in WASM)
            "string.toBoolean" | "integer.toBoolean" | "number.toBoolean" => Some(MirType::I32),
            // Matrix methods
            "matrix.determinant" => Some(MirType::F64),
            "matrix.rows" | "matrix.cols" | "matrix.size" => Some(MirType::I32),

            // String functions that return I32 (string pointer or length)
            name if name.starts_with("string.") => Some(MirType::I32),
            // List functions that return I32 (list pointer or length)
            name if name.starts_with("list.") => Some(MirType::I32),
            // HTTP functions that return I32 (string pointer)
            name if name.starts_with("http.") => Some(MirType::I32),
            // File functions that return I32 (string pointer or boolean)
            name if name.starts_with("file.") => Some(MirType::I32),
            // Void functions
            "print" | "printl" | "println" => Some(MirType::Void),
            // Default - return None to indicate unknown
            _ => None,
        }
    }

    /// Store value to local with automatic type conversion if needed
    /// This function checks if the value on the stack needs type conversion before storing
    fn store_to_local_with_conversion(
        &mut self,
        value_id: ValueId,
        source_type: Option<MirType>,
    ) -> Result<(), CompilerError> {
        if let Some(&local_index) = self.value_to_local.get(&value_id) {
            // Get destination type from function.locals
            if let Some(dest_type) = self.get_value_type(value_id) {
                // If we know the source type, check if conversion is needed
                if let Some(src_type) = source_type {
                    // CRITICAL FIX: Add type conversions when assigning between i32 and f64
                    match (&src_type, &dest_type) {
                        // f64 → i32: Truncate float to integer
                        (MirType::F64, MirType::I32) => {
                            debug_mir!(
            "DEBUG TYPE CONVERSION: Adding f64→i32 conversion for ValueId({:?})",
                                value_id.0
                            );
                            self.current_instructions.push(Instruction::I32TruncF64S);
                        }
                        // i32 → f64: Convert integer to float
                        (MirType::I32, MirType::F64) => {
                            debug_mir!(
            "DEBUG TYPE CONVERSION: Adding i32→f64 conversion for ValueId({:?})",
                                value_id.0
                            );
                            self.current_instructions.push(Instruction::F64ConvertI32S);
                        }
                        // Same types or pointer types - no conversion needed
                        _ => {}
                    }
                }
            }

            // Store to local
            self.current_instructions
                .push(Instruction::LocalSet(local_index));
            Ok(())
        } else {
            // CRITICAL FIX: No more silent auto-allocation of missing ValueIds
            // Return proper error to surface MIR builder bugs
            // All ValueIds must be properly registered in function.locals before codegen
            return Err(CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    format!(
                        "ValueId({:?}) not found in local variable map during store_to_local. \
                        This indicates the MIR builder did not properly allocate this value in function.locals. \
                        All result values must be pre-allocated before code generation.",
                        value_id.0
                    ),
                    Some("Ensure MIR builder adds all ValueIds to function.locals before generating instructions".to_string()),
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            });
        }
    }

    /// Store value to local without type conversion (backward compatibility)
    fn store_to_local(&mut self, value_id: ValueId) -> Result<(), CompilerError> {
        self.store_to_local_with_conversion(value_id, None)
    }

    /// Generate WASM binary operation (type-aware)
    fn generate_binary_operation(
        &mut self,
        op: &MirBinaryOp,
        left: &MirOperand,
        right: &MirOperand,
    ) -> Result<(), CompilerError> {
        // Determine if we're working with floats by checking operand types
        let is_float = self.is_float_operand(left) || self.is_float_operand(right);

        let instruction = match op {
            // Arithmetic operations
            MirBinaryOp::Add => {
                if is_float {
                    Instruction::F64Add
                } else {
                    Instruction::I32Add
                }
            }
            MirBinaryOp::Sub => {
                if is_float {
                    Instruction::F64Sub
                } else {
                    Instruction::I32Sub
                }
            }
            MirBinaryOp::Mul => {
                if is_float {
                    Instruction::F64Mul
                } else {
                    Instruction::I32Mul
                }
            }
            MirBinaryOp::Div => {
                if is_float {
                    Instruction::F64Div
                } else {
                    Instruction::I32DivS
                }
            }
            MirBinaryOp::Rem => {
                if is_float {
                    // F64 doesn't have remainder, use modulo semantics (not perfect but functional)
                    // For proper implementation, this should call a helper function
                    Instruction::I32RemS // Fallback - this will cause type errors on f64
                } else {
                    Instruction::I32RemS
                }
            }

            // Comparison operations
            MirBinaryOp::Eq => {
                if is_float {
                    Instruction::F64Eq
                } else {
                    Instruction::I32Eq
                }
            }
            MirBinaryOp::Ne => {
                if is_float {
                    Instruction::F64Ne
                } else {
                    Instruction::I32Ne
                }
            }
            MirBinaryOp::Lt => {
                if is_float {
                    Instruction::F64Lt
                } else {
                    Instruction::I32LtS
                }
            }
            MirBinaryOp::Le => {
                if is_float {
                    Instruction::F64Le
                } else {
                    Instruction::I32LeS
                }
            }
            MirBinaryOp::Gt => {
                if is_float {
                    Instruction::F64Gt
                } else {
                    Instruction::I32GtS
                }
            }
            MirBinaryOp::Ge => {
                if is_float {
                    Instruction::F64Ge
                } else {
                    Instruction::I32GeS
                }
            }

            // Bitwise operations (only valid for integers)
            MirBinaryOp::And => Instruction::I32And,
            MirBinaryOp::Or => Instruction::I32Or,
            MirBinaryOp::Xor => Instruction::I32Xor,
            MirBinaryOp::Shl => Instruction::I32Shl,
            MirBinaryOp::Shr => Instruction::I32ShrS,
        };

        self.current_instructions.push(instruction);
        Ok(())
    }

    /// Helper: Check if an operand is a floating-point type
    fn is_float_operand(&self, operand: &MirOperand) -> bool {
        match operand {
            MirOperand::Constant(constant) => matches!(constant, MirConstant::Float(_)),
            MirOperand::Value(value_id) => {
                if let Some(mir_type) = self.value_to_type.get(value_id) {
                    matches!(mir_type, MirType::F32 | MirType::F64)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Generate WASM unary operation
    fn generate_unary_operation(&mut self, op: &MirUnaryOp) -> Result<(), CompilerError> {
        match op {
            MirUnaryOp::Neg => {
                // Negate: 0 - x
                self.current_instructions.push(Instruction::I32Const(0));
                // Swap the order so we have: 0, x
                // Then subtract: 0 - x
                self.current_instructions.push(Instruction::I32Sub);
            }
            MirUnaryOp::Not => {
                // Logical not: x == 0
                self.current_instructions.push(Instruction::I32Const(0));
                self.current_instructions.push(Instruction::I32Eq);
            }
            MirUnaryOp::BitNot => {
                // Bitwise not: x ^ -1
                self.current_instructions.push(Instruction::I32Const(-1));
                self.current_instructions.push(Instruction::I32Xor);
            }
            // BOOK: required-operator - Required is handled specially in MirOperation::UnaryOp
            MirUnaryOp::Required => {
                // Required operator should be handled in UnaryOp match arm above
                // This should never be reached - just leave value on stack unchanged
                // (the check and trap happen in the special handling)
            }
        }

        Ok(())
    }

    /// Convert MIR function signature to WASM types
    fn convert_function_signature(
        &self,
        function: &MirFunction,
    ) -> Result<(Vec<ValType>, Vec<ValType>), CompilerError> {
        let mut param_types = Vec::new();
        let mut result_types = Vec::new();

        // Convert parameter types
        // CRITICAL FIX: String parameters AND returns are passed as single pointer to [len|content] structure
        // This matches stdlib function signatures (e.g., string.toUpperCase: i32 -> i32)
        for (i, param) in function.parameters.iter().enumerate() {
            let val_type = self.mir_type_to_wasm_type(&param.param_type)?;

            // DEBUG: Log each iteration for specific functions
            if function.name == "logMessage" || function.name == "buildUrl" {
                debug_mir!("DEBUG PARAM CONVERSION ITERATION[{}]: function='{}' param='{}' mir_type={:?} val_type={:?} param_types_len_before={}",
                    i, function.name, param.name, param.param_type, val_type, param_types.len());
            }

            param_types.push(val_type);

            // DEBUG: Log after push
            if function.name == "logMessage" || function.name == "buildUrl" {
                debug_mir!(
                    "DEBUG AFTER PUSH[{}]: function='{}' param_types_len_after={} param_types={:?}",
                    i,
                    function.name,
                    param_types.len(),
                    param_types
                );
            }
        }

        // DEBUG: Log signature conversion for specific functions
        if function.name == "logMessage" || function.name == "buildUrl" {
            tracing::debug!(
                name = %function.name,
                param_count = function.parameters.len(),
                wasm_param_count = param_types.len(),
                "DEBUG SIGNATURE CONVERSION"
            );
            for (i, param) in function.parameters.iter().enumerate() {
                tracing::debug!(
                    name = %function.name,
                    param_index = i,
                    param_name = %param.name,
                    mir_type = ?param.param_type,
                    "DEBUG SIGNATURE PARAM"
                );
            }
            tracing::debug!(
                name = %function.name,
                wasm_param_types = ?param_types,
                "DEBUG SIGNATURE WASM TYPES"
            );
        }

        // Convert return type
        tracing::debug!(
            name = %function.name,
            return_type = ?function.return_type,
            "Function MIR return type"
        );
        match &function.return_type {
            MirType::StringTuple => {
                // CRITICAL FIX: String returns are single pointer to [len|content] structure in memory
                // This matches how stdlib functions work (string.toUpperCase returns i32)
                result_types.push(ValType::I32);
                debug_mir!(
                    "Converted to WASM result_types: [I32] (string tuple as memory pointer)"
                );
            }
            MirType::Void => {
                // No return value
                debug_mir!("Converted to WASM result_types: [] (void)");
            }
            MirType::Ptr(inner) => {
                // CRITICAL FIX: Ptr(Void) should be treated as Void, not I32
                if matches!(**inner, MirType::Void) {
                    debug_mir!("Converted Ptr(Void) to WASM result_types: [] (void)");
                    // No return value for Ptr(Void)
                } else {
                    // Other pointer types are i32
                    result_types.push(ValType::I32);
                    tracing::debug!(
                        inner = ?inner,
                        "Converted Ptr to WASM result_types: [I32]"
                    );
                }
            }
            _ => {
                result_types.push(self.mir_type_to_wasm_type(&function.return_type)?);
                tracing::debug!(
                    result_types = ?result_types,
                    "Converted to WASM result_types"
                );
            }
        }

        Ok((param_types, result_types))
    }

    /// Convert MIR type to WASM ValType
    fn mir_type_to_wasm_type(&self, mir_type: &MirType) -> Result<ValType, CompilerError> {
        match mir_type {
            MirType::I8
            | MirType::I16
            | MirType::I32
            | MirType::U8
            | MirType::U16
            | MirType::U32
            | MirType::Bool => Ok(ValType::I32),

            MirType::I64 | MirType::U64 => Ok(ValType::I64),

            MirType::F32 => Ok(ValType::F32),
            MirType::F64 => Ok(ValType::F64),

            MirType::Ptr(_) => Ok(ValType::I32), // Pointers are 32-bit addresses

            MirType::StringTuple => {
                // CRITICAL FIX: StringTuple as a parameter type means pointer to string structure
                // As a return type, it uses multi-value (handled separately)
                Ok(ValType::I32)
            }

            MirType::Any => {
                // Any type is a pointer to boxed value structure: [tag:i32][value1:i32][value2:i32]
                // The pointer itself is an i32
                Ok(ValType::I32)
            }

            _ => Err(CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    format!("Cannot convert MIR type to WASM: {:?}", mir_type),
                    None,
                    crate::error::ErrorType::Codegen,
                    Some(crate::ast::SourceLocation::default()),
                )),
            }),
        }
    }

    /// Compute local variable types for WASM function
    fn compute_local_types(&self, function: &MirFunction) -> Vec<(u32, ValType)> {
        let mut local_types_map = std::collections::HashMap::new();

        // First, add explicitly declared locals from MIR function
        debug_mir!(
            function_name = %function.name,
            locals_count = function.locals.len(),
            value_to_local_entries = self.value_to_local.len(),
            "COMPUTE_TYPES: Processing function locals"
        );
        for (vid, &local_idx) in &self.value_to_local {
            debug_mir!(
                value_id = vid.0,
                local_idx = local_idx,
                "value_to_local mapping"
            );
        }

        for (value_id, local) in &function.locals {
            debug_mir!(
                value_id = value_id.0,
                mir_type = ?local.local_type,
                "Processing local type"
            );
            if let Ok(wasm_type) = self.mir_type_to_wasm_type(&local.local_type) {
                if let Some(&local_index) = self.value_to_local.get(value_id) {
                    debug_mir!(
                        local_index = local_index,
                        wasm_type = ?wasm_type,
                        "Local maps to WASM type"
                    );
                    local_types_map.insert(local_index, wasm_type);
                } else {
                    debug_mir!(value_id = value_id.0, "NOT in value_to_local map");
                }
            } else {
                debug_mir!(
                    value_id = value_id.0,
                    "Failed to convert MIR type to WASM type"
                );
            }
        }

        // Then, add auto-allocated locals that were created during code generation
        // These aren't in function.locals but are in value_to_local
        for (value_id, &local_index) in &self.value_to_local {
            if !local_types_map.contains_key(&local_index) {
                // This is an auto-allocated local
                // Try to determine its type from value_to_type
                let wasm_type = if let Some(mir_type) = self.value_to_type.get(value_id) {
                    self.mir_type_to_wasm_type(mir_type).unwrap_or(ValType::I32)
                } else {
                    // Default to i32 if type is unknown
                    ValType::I32
                };
                tracing::trace!(
                    local_index = local_index,
                    value_id = ?value_id,
                    wasm_type = ?wasm_type,
                    "Auto-allocated local type"
                );
                local_types_map.insert(local_index, wasm_type);
            }
        }

        // CRITICAL FIX: Add tracked temporary locals created during code generation
        // (e.g., for string expansion in load_string_argument_for_print)
        // Use temp_local_types to get the correct type instead of defaulting to i32
        for (&local_index, &wasm_type) in &self.temp_local_types {
            if !local_types_map.contains_key(&local_index) {
                debug_mir!(
                    "DEBUG MIR: Adding temporary local {} with tracked type {:?}",
                    local_index,
                    wasm_type
                );
                local_types_map.insert(local_index, wasm_type);
            }
        }

        // Convert map to vec of (count, type) pairs
        // CRITICAL FIX: Only return locals AFTER parameters
        // In WASM, parameters are part of the local index space, but WasmFunction::new()
        // expects only the additional locals, not the parameters (which are in the signature)
        let num_params = function.parameters.len() as u32;
        let mut locals = Vec::new();
        debug_mir!(
            "DEBUG LOCAL TYPES: Computing final local types (next_local_index={}, num_params={})",
            self.next_local_index,
            num_params
        );
        debug_mir!(
            "DEBUG LOCAL TYPES: Only returning locals starting from index {}",
            num_params
        );
        for i in num_params..self.next_local_index {
            if let Some(&wasm_type) = local_types_map.get(&i) {
                debug_mir!(" LOCAL TYPES:   Local {} -> {:?}", i, wasm_type);
                locals.push((1, wasm_type));
            }
        }
        debug_mir!(
            "DEBUG LOCAL TYPES: Final locals vec has {} entries (excluding {} parameters)",
            locals.len(),
            num_params
        );

        debug_mir!("DEBUG MIR: Computed {} local types total", locals.len());
        locals
    }

    /// Compute basic block order for code generation
    #[allow(dead_code)] // Reserved for future optimization passes
    fn compute_block_order(&self, function: &MirFunction) -> Vec<BasicBlockId> {
        // For now, use a simple ordering starting with entry block
        let mut order = vec![function.entry_block];

        for &block_id in function.blocks.keys() {
            if block_id != function.entry_block {
                order.push(block_id);
            }
        }

        order
    }

    /// Resolve namespace function SymbolId to WASM function name
    #[allow(dead_code)]
    fn resolve_namespace_function(&self, symbol_id: SymbolId) -> Option<String> {
        // Based on registration order in symbol_table.rs, map SymbolIds to function names
        // These correspond to the math namespace functions registered at lines 748-772
        match symbol_id.0 {
            35 => Some("math_sin".to_string()),
            36 => Some("math_cos".to_string()),
            37 => Some("math_tan".to_string()),
            38 => Some("math_abs".to_string()),
            39 => Some("math_floor".to_string()),
            40 => Some("math_ceil".to_string()),
            41 => Some("math_round".to_string()),
            42 => Some("math_sqrt".to_string()),
            43 => Some("math_trunc".to_string()),
            44 => Some("math_pi".to_string()),
            45 => Some("math_pow".to_string()),
            46 => Some("math_max".to_string()),
            47 => Some("math_min".to_string()),
            // String namespace functions (use dot notation to match stdlib registration)
            48 => Some("string.length".to_string()),
            49 => Some("string.substring".to_string()),
            50 => Some("string.toUpperCase".to_string()),
            51 => Some("string.toLowerCase".to_string()),
            52 => Some("string.contains".to_string()),
            // List namespace functions
            53 => Some("list.size".to_string()),
            54 => Some("list.push".to_string()),
            55 => Some("list.pop".to_string()),
            56 => Some("list.get".to_string()),
            // Additional math functions (MUST use different IDs to avoid shadowing)
            70 => Some("math.ln".to_string()),
            71 => Some("math.log10".to_string()),
            72 => Some("math.log2".to_string()),
            73 => Some("math.exp".to_string()),
            74 => Some("math.exp2".to_string()),
            75 => Some("math.asin".to_string()),
            76 => Some("math.acos".to_string()),
            77 => Some("math.atan".to_string()),
            78 => Some("math.atan2".to_string()),
            79 => Some("math.sinh".to_string()),
            80 => Some("math.cosh".to_string()),
            81 => Some("math.tanh".to_string()),
            // Type method calls like toString (SymbolId 60-69) - use dot notation
            60 => Some("number.toString".to_string()), // number.toString
            61 => Some("math.max".to_string()),        // CRITICAL FIX: math.max not number.toString
            62 => Some("integer.toString".to_string()), // integer.toString
            63 => Some("boolean.toString".to_string()), // boolean.toString
            64 => Some("string.length".to_string()),   // string.length (alt mapping)
            65 => Some("string.substring".to_string()), // string.substring (alt mapping)
            66 => Some("string.contains".to_string()), // string.contains (alt mapping)
            67 => Some("string.contains".to_string()), // string.contains (alt2)
            68 => Some("string.length".to_string()),   // string.length (alt2)
            69 => Some("string.isEmpty".to_string()),  // string.isEmpty - FIXED
            _ => {
                tracing::debug!(
                    symbol_id = symbol_id.0,
                    "Unknown namespace function SymbolId"
                );
                None
            }
        }
    }

    /// Get function name by symbol ID using pure dynamic resolution
    /// CRITICAL FIX: Completely eliminated hardcoded SymbolId mappings
    /// All symbols (builtins + user-defined) are resolved from the symbol table
    fn get_function_name_by_symbol(&self, symbol_id: SymbolId) -> Option<String> {
        debug_mir!(
            "DEBUG SYMBOL MAP LOOKUP: Looking up SymbolId({}) in function_symbol_map",
            symbol_id.0
        );
        debug_mir!(
            "DEBUG SYMBOL MAP LOOKUP: Map has {} entries",
            self.function_symbol_map.len()
        );

        if let Some(function_name) = self.function_symbol_map.get(&symbol_id) {
            debug_mir!(
                "DEBUG SYMBOL MAP LOOKUP: Found SymbolId({}) -> '{}'",
                symbol_id.0,
                function_name
            );
            tracing::debug!(
                symbol_id = symbol_id.0,
                name = %function_name,
                "Resolved SymbolId to function name dynamically"
            );
            Some(function_name.clone())
        } else {
            debug_mir!(
                "DEBUG SYMBOL MAP LOOKUP: SymbolId({}) NOT FOUND in map!",
                symbol_id.0
            );
            debug_mir!(
                "DEBUG SYMBOL MAP LOOKUP: Map contents (first 10): {:?}",
                self.function_symbol_map.iter().take(10).collect::<Vec<_>>()
            );
            tracing::warn!(
                symbol_id = symbol_id.0,
                "Unknown function SymbolId - not found in function map"
            );
            None
        }
    }

    /// Register builtin function signatures
    /// This allows the codegen to know which builtin functions return void vs values
    fn register_builtin_function_signatures(&mut self) {
        use crate::mir::mir_types::{BasicBlockId, MirFunction, MirFunctionAttributes, MirType};
        use std::collections::HashMap;

        // Helper to create a fake MirFunction for a builtin with given return type
        let create_builtin_signature =
            |name: &str, symbol_id: usize, return_type: MirType| MirFunction {
                symbol_id: SymbolId(symbol_id),
                name: name.to_string(),
                parameters: vec![],
                return_type,
                blocks: HashMap::new(),
                entry_block: BasicBlockId(0),
                locals: HashMap::new(),
                next_value_id: 0,
                next_block_id: 0,
                attributes: MirFunctionAttributes {
                    inline: false,
                    pure: false,
                    entry_point: false,
                    exported: false,
                },
                location: Default::default(),
            };

        // Register void-returning functions
        self.function_signatures.insert(
            SymbolId(0),
            create_builtin_signature("print", 0, MirType::Void),
        );
        self.function_signatures.insert(
            SymbolId(1),
            create_builtin_signature("printl", 1, MirType::Void),
        );
        self.function_signatures.insert(
            SymbolId(2),
            create_builtin_signature("println", 2, MirType::Void),
        );

        // Register value-returning type conversion functions
        self.function_signatures.insert(
            SymbolId(5),
            create_builtin_signature("int_to_string", 5, MirType::Ptr(Box::new(MirType::I32))),
        );
        self.function_signatures.insert(
            SymbolId(6),
            create_builtin_signature("float_to_string", 6, MirType::Ptr(Box::new(MirType::I32))),
        );
        self.function_signatures.insert(
            SymbolId(7),
            create_builtin_signature("bool_to_string", 7, MirType::Ptr(Box::new(MirType::I32))),
        );
        self.function_signatures.insert(
            SymbolId(8),
            create_builtin_signature("string_to_int", 8, MirType::I32),
        );
        self.function_signatures.insert(
            SymbolId(9),
            create_builtin_signature("string_to_float", 9, MirType::F64),
        );

        tracing::debug!("Registered builtin function signatures");
    }

    /// Set up memory section
    fn setup_memory_section(&mut self) -> Result<(), CompilerError> {
        debug_mir!("DEBUG MIR: Setting up memory section with 16 pages (1MB) minimum");
        self.wasm_generator
            .memory_section
            .memory(wasm_encoder::MemoryType {
                minimum: 16,       // 16 pages = 1MB initial memory
                maximum: Some(64), // Limit to 64 pages (4MB) for safety
                memory64: false,
                shared: false,
            });
        debug_mir!("DEBUG MIR: Memory section configured");
        Ok(())
    }

    /// Set up string pool in WASM module
    fn setup_string_pool(&mut self, string_pool: &[String]) -> Result<(), CompilerError> {
        debug_mir!(
            "DEBUG MIR: Setting up string pool with {} strings:",
            string_pool.len()
        );
        for (i, s) in string_pool.iter().enumerate() {
            debug_mir!("DEBUG MIR:   String {}: '{}'", i, s);
        }

        // Store the string pool for use during code generation
        self.string_pool = Some(string_pool.to_vec());

        // Pre-register all strings in the underlying WASM generator's string pool
        // This ensures they get proper data section offsets
        for string_content in string_pool {
            let offset = self
                .wasm_generator
                .get_or_create_string_offset(string_content)?;
            debug_mir!(
                "DEBUG MIR: Registered string '{}' at offset {}",
                string_content,
                offset
            );
        }

        Ok(())
    }

    /// Add function to WASM module
    fn add_function_to_module(
        &mut self,
        name: String,
        wasm_function: WasmFunction,
        signature: (Vec<ValType>, Vec<ValType>),
    ) -> Result<(), CompilerError> {
        // Convert signature to the format expected by CodeGenerator
        let (param_types, return_types) = signature;

        // Convert all return types (supports multi-value returns)
        let return_wasm_types: Vec<_> = return_types
            .iter()
            .map(|vt| self.val_type_to_wasm_type(vt))
            .collect::<Result<Vec<_>, _>>()?;

        let param_wasm_types: Vec<_> = param_types
            .iter()
            .map(|vt| self.val_type_to_wasm_type(vt))
            .collect::<Result<Vec<_>, _>>()?;

        // Log function registration
        tracing::debug!(name = %name, "Registering function");

        // CRITICAL FIX: Use the pre-registered index from function_map instead of function_count
        // The index was already assigned during pre-registration phase
        let function_index = self
            .wasm_generator
            .function_map
            .get(&name)
            .copied()
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    format!(
                        "Function '{}' not found in function_map during generation. \
                         This should never happen as all functions are pre-registered.",
                        name
                    ),
                    None,
                    None,
                )
            })?;

        // Add function type signature
        let type_index = self.wasm_generator.add_function_type(
            &param_wasm_types,
            if return_wasm_types.is_empty() {
                None
            } else {
                Some(return_wasm_types[0])
            },
        )?;

        // Add function to function section
        self.wasm_generator.function_section.function(type_index);

        // Add function code to code section
        self.wasm_generator.code_section.function(&wasm_function);

        // Update function tracking
        self.wasm_generator.function_names.push(name.clone());
        // NOTE: function_map already has the correct index from pre-registration,
        // so we don't insert again. function_count will be updated after all functions are generated.

        let function_index = function_index;

        tracing::debug!(
            name = %name,
            index = function_index,
            "Function registered with pre-assigned index"
        );
        tracing::debug!(
            entries = self.wasm_generator.function_map.len(),
            "Function map after registration"
        );
        // Verify the function was actually added
        if let Some(&idx) = self.wasm_generator.function_map.get(&name) {
            tracing::trace!(name = %name, index = idx, "Verified function is in map");
        } else {
            tracing::error!(name = %name, "Function was NOT added to function map");
        }

        Ok(())
    }

    /// Convert ValType to WasmType
    fn val_type_to_wasm_type(
        &self,
        val_type: &ValType,
    ) -> Result<crate::codegen::WasmType, CompilerError> {
        use crate::codegen::WasmType;
        match val_type {
            ValType::I32 => Ok(WasmType::I32),
            ValType::I64 => Ok(WasmType::I64),
            ValType::F32 => Ok(WasmType::F32),
            ValType::F64 => Ok(WasmType::F64),
            _ => Err(CompilerError::codegen_error(
                format!("Unsupported WASM value type: {:?}", val_type),
                None,
                None,
            )),
        }
    }

    /// Generate start function export for the entry point
    fn generate_start_function_export(
        &mut self,
        entry_symbol_id: SymbolId,
    ) -> Result<(), CompilerError> {
        // Log all functions in function map
        tracing::debug!("Function map contents:");
        for (name, index) in &self.wasm_generator.function_map {
            tracing::trace!(name = %name, index = index, "Function in map");
        }
        tracing::debug!(
            entries = self.wasm_generator.function_map.len(),
            symbol_id = entry_symbol_id.0,
            "Looking for entry function by SymbolId"
        );

        // CRITICAL FIX: Use SymbolId -> index mapping instead of function name
        // Function names can collide (e.g., top-level start() and Vehicle.start() method)
        // but SymbolIds are unique
        if let Some(entry_function_index) = self.symbol_to_function_index.get(&entry_symbol_id) {
            // Create a _start function that calls the entry function
            let type_index = self
                .wasm_generator
                .type_manager
                .add_function_type_single(&[], None)?;
            self.wasm_generator.function_section.function(type_index);

            let mut instructions = Vec::new();
            // Call the start function
            // NOTE: entry_function_index from function_map is ALREADY absolute (includes imports)
            // because function_count is incremented for both imports and user functions
            instructions.push(Instruction::Call(*entry_function_index));
            // CRITICAL FIX: Only drop return value if the function actually returns something
            // The start function is void, so there's nothing to drop
            // instructions.push(Instruction::Drop);  // Removed - causes stack underflow for void functions
            // End function
            instructions.push(Instruction::End);

            // Create the start function
            let mut start_function = WasmFunction::new(vec![]);
            for instruction in instructions {
                start_function.instruction(&instruction);
            }

            // Add to code section
            self.wasm_generator.code_section.function(&start_function);

            // Export as _start
            // CRITICAL FIX: _start is the NEXT function after all existing functions
            // Use function_count which tracks all functions (imports + defined functions)
            let start_func_index = self.wasm_generator.function_count;
            self.wasm_generator.export_section.export(
                "_start",
                wasm_encoder::ExportKind::Func,
                start_func_index,
            );

            // Note: Memory export moved to finalize_module() for all modules

            // Update function tracking
            self.wasm_generator
                .function_names
                .push("_start".to_string());
            self.wasm_generator.function_count += 1;
        } else {
            return Err(CompilerError::codegen_error(
                "Entry point function 'start' not found in function map".to_string(),
                None,
                None,
            ));
        }

        Ok(())
    }

    /// Emit code to box a value into an `any` type
    ///
    /// Boxing allocates 12 bytes: [tag:i32][value1:i32][value2:i32]
    /// - tag: type discriminator from AnyTypeTag
    /// - value1: primary value (integer, boolean, string pointer, or f64 low bits)
    /// - value2: secondary value (f64 high bits, or 0 for other types)
    ///
    /// After this function, the boxed pointer is on the stack
    fn emit_box_value(
        &mut self,
        tag: AnyTypeTag,
        source_type: &MirType,
    ) -> Result<(), CompilerError> {
        debug_mir!(?tag, ?source_type, "Boxing value to any type");

        // Get mem_alloc function index
        let mem_alloc_idx = *self
            .wasm_generator
            .function_map
            .get("mem_alloc")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "mem_alloc function not found in function_map for boxing".to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        // The value to box is already on the stack
        // We need to save it to a temporary local first

        // Create a temporary local for the value
        let temp_local = self.next_local_index;
        self.next_local_index += 1;

        // Store the value to the temp local
        match source_type {
            MirType::F64 => {
                self.current_instructions
                    .push(Instruction::LocalSet(temp_local));
                self.temp_local_types.insert(temp_local, ValType::F64);
            }
            _ => {
                self.current_instructions
                    .push(Instruction::LocalSet(temp_local));
                self.temp_local_types.insert(temp_local, ValType::I32);
            }
        }

        // Allocate 12 bytes for the boxed structure
        // mem_alloc(type_id=0, size=12)
        self.current_instructions.push(Instruction::I32Const(0)); // type_id
        self.current_instructions.push(Instruction::I32Const(12)); // size
        self.current_instructions
            .push(Instruction::Call(mem_alloc_idx));

        // Save the pointer to another temp local
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);
        self.current_instructions
            .push(Instruction::LocalTee(ptr_local));

        // Store the tag at offset 0
        // Stack: [ptr]
        self.current_instructions
            .push(Instruction::I32Const(tag.as_i32()));
        self.current_instructions
            .push(Instruction::I32Store(wasm_encoder::MemArg {
                offset: 0,
                align: 2, // 4-byte alignment
                memory_index: 0,
            }));

        // Store value1 at offset 4
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));

        match source_type {
            MirType::F64 => {
                // For f64, we need to reinterpret as i64 and split into two i32s
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I64ReinterpretF64);
                // Store low 32 bits
                self.current_instructions.push(Instruction::I32WrapI64);
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                // Store value2 (high 32 bits) at offset 8
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I64ReinterpretF64);
                self.current_instructions.push(Instruction::I64Const(32));
                self.current_instructions.push(Instruction::I64ShrU);
                self.current_instructions.push(Instruction::I32WrapI64);
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 8,
                        align: 2,
                        memory_index: 0,
                    }));
            }
            _ => {
                // For i32 types (integer, boolean, string pointer, etc.)
                self.current_instructions
                    .push(Instruction::LocalGet(temp_local));
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));

                // Store 0 in value2 at offset 8
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.current_instructions.push(Instruction::I32Const(0));
                self.current_instructions
                    .push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 8,
                        align: 2,
                        memory_index: 0,
                    }));
            }
        }

        // Push the boxed pointer onto the stack as the result
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));

        debug_mir!(?tag, "Boxing complete, pointer on stack");
        Ok(())
    }

    /// Emit code to unbox a value from an `any` type
    ///
    /// The boxed pointer is on the stack. This function:
    /// 1. Reads the tag to determine the type
    /// 2. Reads and reconstructs the value
    ///
    /// Returns the type tag so the caller can dispatch appropriately
    fn emit_unbox_to_i32(&mut self) -> Result<(), CompilerError> {
        debug_mir!("Unboxing any value to i32");

        // The boxed pointer is on the stack
        // Need to check type tag and handle both integer (tag 1) and number (tag 3) cases

        // Save pointer to a temp local so we can read both tag and value
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);

        self.current_instructions
            .push(Instruction::LocalSet(ptr_local));

        // Read the type tag at offset 0
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));

        // Check if type tag is 3 (f64 number from JSON parsing)
        self.current_instructions.push(Instruction::I32Const(3));
        self.current_instructions.push(Instruction::I32Eq);
        self.current_instructions
            .push(Instruction::If(wasm_encoder::BlockType::Result(
                ValType::I32,
            )));

        // Type tag is 3 (Number): Read f64 at offset 4 and convert to i32
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::F64Load(wasm_encoder::MemArg {
                offset: 4,
                align: 3,
                memory_index: 0,
            }));
        // Convert f64 to i32 (truncate)
        self.current_instructions.push(Instruction::I32TruncF64S);

        self.current_instructions.push(Instruction::Else);

        // Type tag is not 3: Read i32 at offset 4 directly (type tag 1 = Integer)
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));

        self.current_instructions.push(Instruction::End);

        Ok(())
    }

    /// Emit code to read the type tag from a boxed any value
    /// The boxed pointer should be on the stack
    /// After this, the tag (i32) is on the stack
    fn emit_read_any_tag(&mut self) -> Result<(), CompilerError> {
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));
        Ok(())
    }

    /// Emit code to unbox a value to f64
    /// The boxed pointer should be on the stack
    fn emit_unbox_to_f64(&mut self) -> Result<(), CompilerError> {
        debug_mir!("Unboxing any value to f64");

        // Save pointer to temp
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);
        self.current_instructions
            .push(Instruction::LocalSet(ptr_local));

        // Read low bits from offset 4
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions.push(Instruction::I64ExtendI32U);

        // Read high bits from offset 8
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.current_instructions
            .push(Instruction::I32Load(wasm_encoder::MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }));
        self.current_instructions.push(Instruction::I64ExtendI32U);
        self.current_instructions.push(Instruction::I64Const(32));
        self.current_instructions.push(Instruction::I64Shl);

        // Combine: high | low
        self.current_instructions.push(Instruction::I64Or);

        // Reinterpret as f64
        self.current_instructions
            .push(Instruction::F64ReinterpretI64);

        Ok(())
    }

    /// Emit code to convert an any value to string with proper type dispatch
    /// The boxed pointer should be on the stack
    fn emit_any_to_string(&mut self) -> Result<(), CompilerError> {
        debug_mir!("Converting any value to string with type dispatch");

        // Get function indices for conversion functions
        let int_to_string_idx = *self
            .wasm_generator
            .function_map
            .get("int_to_string")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "int_to_string function not found".to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        let float_to_string_idx = *self
            .wasm_generator
            .function_map
            .get("float_to_string")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "float_to_string function not found".to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        let bool_to_string_idx = *self
            .wasm_generator
            .function_map
            .get("bool_to_string")
            .ok_or_else(|| CompilerError::Codegen {
                context: Box::new(crate::error::ErrorContext::new(
                    "bool_to_string function not found".to_string(),
                    None,
                    crate::error::ErrorType::Codegen,
                    None,
                )),
            })?;

        // Save the boxed pointer to a local
        let ptr_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(ptr_local, ValType::I32);
        self.current_instructions
            .push(Instruction::LocalSet(ptr_local));

        // Read the tag
        self.current_instructions
            .push(Instruction::LocalGet(ptr_local));
        self.emit_read_any_tag()?;

        // Create a result local
        let result_local = self.next_local_index;
        self.next_local_index += 1;
        self.temp_local_types.insert(result_local, ValType::I32);

        // Dispatch based on tag using if-else chain
        // if tag == 1 (Integer) -> int_to_string
        // else if tag == 2 (Boolean) -> bool_to_string
        // else if tag == 3 (Number) -> float_to_string
        // else if tag == 4 (String) -> return value directly
        // else -> int_to_string as fallback

        // Check for Integer (tag == 1)
        self.current_instructions
            .push(Instruction::I32Const(AnyTypeTag::Integer.as_i32()));
        self.current_instructions.push(Instruction::I32Eq);
        self.current_instructions
            .push(Instruction::If(BlockType::Empty));
        {
            // Integer case: call int_to_string
            self.current_instructions
                .push(Instruction::LocalGet(ptr_local));
            self.emit_unbox_to_i32()?;
            self.current_instructions
                .push(Instruction::Call(int_to_string_idx));
            self.current_instructions
                .push(Instruction::LocalSet(result_local));
        }
        self.current_instructions.push(Instruction::Else);
        {
            // Check for Boolean (tag == 2)
            self.current_instructions
                .push(Instruction::LocalGet(ptr_local));
            self.emit_read_any_tag()?;
            self.current_instructions
                .push(Instruction::I32Const(AnyTypeTag::Boolean.as_i32()));
            self.current_instructions.push(Instruction::I32Eq);
            self.current_instructions
                .push(Instruction::If(BlockType::Empty));
            {
                // Boolean case: call bool_to_string
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.emit_unbox_to_i32()?;
                self.current_instructions
                    .push(Instruction::Call(bool_to_string_idx));
                self.current_instructions
                    .push(Instruction::LocalSet(result_local));
            }
            self.current_instructions.push(Instruction::Else);
            {
                // Check for Number (tag == 3)
                self.current_instructions
                    .push(Instruction::LocalGet(ptr_local));
                self.emit_read_any_tag()?;
                self.current_instructions
                    .push(Instruction::I32Const(AnyTypeTag::Number.as_i32()));
                self.current_instructions.push(Instruction::I32Eq);
                self.current_instructions
                    .push(Instruction::If(BlockType::Empty));
                {
                    // Number case: call float_to_string
                    self.current_instructions
                        .push(Instruction::LocalGet(ptr_local));
                    self.emit_unbox_to_f64()?;
                    self.current_instructions
                        .push(Instruction::Call(float_to_string_idx));
                    self.current_instructions
                        .push(Instruction::LocalSet(result_local));
                }
                self.current_instructions.push(Instruction::Else);
                {
                    // Check for String (tag == 4)
                    self.current_instructions
                        .push(Instruction::LocalGet(ptr_local));
                    self.emit_read_any_tag()?;
                    self.current_instructions
                        .push(Instruction::I32Const(AnyTypeTag::String.as_i32()));
                    self.current_instructions.push(Instruction::I32Eq);
                    self.current_instructions
                        .push(Instruction::If(BlockType::Empty));
                    {
                        // String case: return value directly (it's already a string pointer)
                        self.current_instructions
                            .push(Instruction::LocalGet(ptr_local));
                        self.emit_unbox_to_i32()?;
                        self.current_instructions
                            .push(Instruction::LocalSet(result_local));
                    }
                    self.current_instructions.push(Instruction::Else);
                    {
                        // Default case: treat as integer
                        self.current_instructions
                            .push(Instruction::LocalGet(ptr_local));
                        self.emit_unbox_to_i32()?;
                        self.current_instructions
                            .push(Instruction::Call(int_to_string_idx));
                        self.current_instructions
                            .push(Instruction::LocalSet(result_local));
                    }
                    self.current_instructions.push(Instruction::End); // End String if
                }
                self.current_instructions.push(Instruction::End); // End Number if
            }
            self.current_instructions.push(Instruction::End); // End Boolean if
        }
        self.current_instructions.push(Instruction::End); // End Integer if

        // Push the result onto the stack
        self.current_instructions
            .push(Instruction::LocalGet(result_local));

        debug_mir!("any.toString() dispatch complete");
        Ok(())
    }

    /// Get or register the __json_get_field function index
    /// Returns the function index for accessing JSON object fields by string key
    fn get_or_register_json_get_field(&mut self) -> Result<u32, CompilerError> {
        // First check if it's already registered
        if let Some(&idx) = self.wasm_generator.function_map.get("__json_get_field") {
            return Ok(idx);
        }

        // The function should have been registered by JsonClass::register_access_operations
        // If not found, return an error
        Err(CompilerError::Codegen {
            context: Box::new(crate::error::ErrorContext::new(
                "__json_get_field function not found. Ensure JSON module is properly initialized."
                    .to_string(),
                None,
                crate::error::ErrorType::Codegen,
                None,
            )),
        })
    }

    /// Get or register the __json_get_index function index
    /// Returns the function index for accessing JSON array elements by integer index
    fn get_or_register_json_get_index(&mut self) -> Result<u32, CompilerError> {
        // First check if it's already registered
        if let Some(&idx) = self.wasm_generator.function_map.get("__json_get_index") {
            return Ok(idx);
        }

        // The function should have been registered by JsonClass::register_access_operations
        // If not found, return an error
        Err(CompilerError::Codegen {
            context: Box::new(crate::error::ErrorContext::new(
                "__json_get_index function not found. Ensure JSON module is properly initialized."
                    .to_string(),
                None,
                crate::error::ErrorType::Codegen,
                None,
            )),
        })
    }

    /// Finalize WASM module and return bytecode
    fn finalize_module(&mut self) -> Result<Vec<u8>, CompilerError> {
        use wasm_encoder::Module;

        let mut module = Module::new();

        // Add sections from the internal CodeGenerator
        // Note: We clone sections as they're being added to the module

        // 1. Add type section
        let type_section = self.wasm_generator.type_manager.clone_type_section();
        module.section(&type_section);

        // 2. Add import section - clone it
        let import_section = self.wasm_generator.import_section.clone();
        module.section(&import_section);

        // 3. Add function section - clone it
        let function_section = self.wasm_generator.function_section.clone();
        module.section(&function_section);

        // 4. Add memory section - clone it
        let memory_section = self.wasm_generator.memory_section.clone();
        module.section(&memory_section);

        // 4.5. Add global section for heap pointer and other globals
        // This must come after Memory section and before Export section per WASM spec
        // CRITICAL FIX: Set heap pointer to AFTER all string constants to avoid overwriting them
        // The string_offset_counter tracks the next free address after all strings
        let heap_start = {
            let string_end = self.wasm_generator.string_offset_counter;
            // Align to 8 bytes for safety
            (string_end + 7) & !7
        };
        let mut global_section = wasm_encoder::GlobalSection::new();
        global_section.global(
            wasm_encoder::GlobalType {
                val_type: wasm_encoder::ValType::I32,
                mutable: true,
            },
            &wasm_encoder::ConstExpr::i32_const(heap_start as i32),
        );
        module.section(&global_section);

        // Export the heap pointer global so host can read it for debugging
        self.wasm_generator.export_section.export(
            "__heap_ptr",
            wasm_encoder::ExportKind::Global,
            0,
        );

        // CRITICAL FIX: Always export memory for WASM host interop
        // Memory must be exported for plugins and any host that needs to read/write WASM memory
        self.wasm_generator
            .export_section
            .export("memory", wasm_encoder::ExportKind::Memory, 0);

        // Export all user-defined functions (from functions: block)
        // These are needed for plugins and library modules
        for (name, &index) in &self.wasm_generator.function_map {
            // Skip internal functions (starting with __) EXCEPT route handlers
            // Route handlers (__route_handler_N) MUST be exported for frame-runtime
            let is_route_handler = name.starts_with("__route_handler_");
            if is_route_handler || (!name.starts_with("__") && !name.starts_with("_")) {
                self.wasm_generator.export_section.export(
                    name,
                    wasm_encoder::ExportKind::Func,
                    index,
                );
            }
        }

        // 5. Add export section - clone it
        let export_section = self.wasm_generator.export_section.clone();
        module.section(&export_section);

        // 6. Add code section - clone it
        let code_section = self.wasm_generator.code_section.clone();
        module.section(&code_section);

        // 7. Add data section (contains string literals)
        let data_section = self.wasm_generator.memory_utils.get_data_section();
        module.section(data_section);

        Ok(module.finish())
    }

    /// Register plugin bridge functions as WASM imports
    ///
    /// Bridge functions from plugin.toml [bridge] sections are registered as WASM imports
    /// so the runtime can provide their implementations.
    ///
    /// For functions with `expand_strings=true`, we:
    /// 1. Register the raw import with expanded signature (strings as ptr,len pairs)
    /// 2. Store wrapper info for LATER registration (after ALL imports are done)
    ///
    /// CRITICAL: This function ONLY registers imports. Wrapper functions are stored in
    /// `pending_bridge_wrappers` and must be registered later by calling
    /// `register_pending_bridge_wrappers()` AFTER all stdlib imports are complete.
    fn register_plugin_bridge_imports(&mut self) -> Result<(), CompilerError> {
        use crate::builtins::registry::BuiltinType;
        use crate::types::WasmType;

        for func in &self.bridge_functions.clone() {
            let param_types = func.get_param_types();
            let return_type = func.get_return_type();
            let module = &func.module;

            // Check if any string params need expansion
            let needs_wrapper =
                func.expand_strings && param_types.iter().any(|t| matches!(t, BuiltinType::String));

            if needs_wrapper {
                // For expand_strings=true: register raw import now, defer wrapper

                // Build expanded signature for raw import
                let mut raw_wasm_params = Vec::new();
                for param_type in &param_types {
                    if matches!(param_type, BuiltinType::String) {
                        raw_wasm_params.push(WasmType::I32); // ptr (content start)
                        raw_wasm_params.push(WasmType::I32); // len
                    } else {
                        raw_wasm_params.push(Self::builtin_type_to_wasm_type(param_type));
                    }
                }

                let wasm_return = match &return_type {
                    BuiltinType::Void => None,
                    _ => Some(Self::builtin_type_to_wasm_type(&return_type)),
                };

                // Register import with expanded signature (NO __raw suffix)
                // The runtime provides functions with their original names
                tracing::debug!(
                    name = %func.name,
                    module = %module,
                    params = ?raw_wasm_params,
                    returns = ?wasm_return,
                    "Registering plugin bridge import (expand_strings)"
                );

                let raw_func_index = self.wasm_generator.register_import_function(
                    module,
                    &func.name,
                    &raw_wasm_params,
                    wasm_return,
                )?;

                // Store wrapper info for later registration (AFTER all imports)
                let wrapper_params: Vec<WasmType> = param_types
                    .iter()
                    .map(|t| Self::builtin_type_to_wasm_type(t))
                    .collect();

                tracing::debug!(
                    name = %func.name,
                    raw_func_index = raw_func_index,
                    "Deferring wrapper function registration until after all imports"
                );

                self.pending_bridge_wrappers.push(PendingBridgeWrapper {
                    name: func.name.clone(),
                    params: wrapper_params,
                    wasm_return,
                    raw_func_index,
                    param_types: param_types.clone(),
                });
            } else {
                // No expansion needed: register import directly
                let wasm_params: Vec<WasmType> = param_types
                    .iter()
                    .map(|t| Self::builtin_type_to_wasm_type(t))
                    .collect();

                let wasm_return = match &return_type {
                    BuiltinType::Void => None,
                    _ => Some(Self::builtin_type_to_wasm_type(&return_type)),
                };

                tracing::debug!(
                    name = %func.name,
                    module = %module,
                    params = ?wasm_params,
                    returns = ?wasm_return,
                    "Registering plugin bridge function as direct WASM import"
                );

                self.wasm_generator.register_import_function(
                    module,
                    &func.name,
                    &wasm_params,
                    wasm_return,
                )?;
            }
        }

        Ok(())
    }

    /// Register pending bridge wrapper functions
    ///
    /// CRITICAL: This MUST be called AFTER all imports are registered (including stdlib imports)
    /// Wrapper functions are internal WASM functions, not imports, so they must be registered
    /// after the import section is complete to avoid function index collisions.
    fn register_pending_bridge_wrappers(&mut self) -> Result<(), CompilerError> {
        use crate::builtins::registry::BuiltinType;
        use wasm_encoder::{Instruction, MemArg};

        let wrappers = std::mem::take(&mut self.pending_bridge_wrappers);

        for wrapper in wrappers {
            // Build wrapper instructions
            let mut wrapper_instructions = Vec::new();
            let mut local_idx = 0u32;

            for param_type in wrapper.param_types.iter() {
                if matches!(param_type, BuiltinType::String) {
                    // For strings: expand to (ptr+4, len)
                    // Clean strings are length-prefixed: [4-byte len][content]
                    // ptr+4 = content start, load from ptr = length

                    // Push content pointer (ptr + 4)
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    wrapper_instructions.push(Instruction::I32Const(4));
                    wrapper_instructions.push(Instruction::I32Add);

                    // Push length (load i32 from ptr)
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    wrapper_instructions.push(Instruction::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                    local_idx += 1;
                } else {
                    // Non-string: pass through
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    local_idx += 1;
                }
            }

            // Call the raw import
            wrapper_instructions.push(Instruction::Call(wrapper.raw_func_index));
            // Note: register_function adds End instruction automatically

            // Register wrapper function
            tracing::debug!(
                name = %wrapper.name,
                params = ?wrapper.params,
                returns = ?wrapper.wasm_return,
                raw_func_index = wrapper.raw_func_index,
                function_count = self.wasm_generator.function_count,
                "Registering wrapper function for expand_strings bridge (after all imports)"
            );

            self.wasm_generator.register_function(
                &wrapper.name,
                &wrapper.params,
                wrapper.wasm_return,
                &wrapper_instructions,
            )?;
        }

        Ok(())
    }

    /// Register HTTP server wrapper functions for string expansion
    ///
    /// HTTP server functions like `_http_route` and `_req_param` need wrapper functions
    /// to expand Clean Language strings (ptr to [len][content]) to raw format (ptr+4, len).
    ///
    /// CRITICAL: This MUST be called AFTER all imports are registered AND after
    /// `register_pending_bridge_wrappers()` to avoid function index collisions.
    fn register_http_server_wrappers(&mut self) -> Result<(), CompilerError> {
        use crate::builtins::registry::BuiltinType;
        use crate::types::WasmType;
        use wasm_encoder::{Instruction, MemArg};

        // Define HTTP server functions that need wrappers
        // These have expand_strings behavior: strings passed as (ptr, len) pairs to host
        let http_server_functions = [
            // _http_route: (string method, string path, integer handler_idx) -> i32
            (
                "_http_route",
                vec![
                    BuiltinType::String,
                    BuiltinType::String,
                    BuiltinType::Integer,
                ],
                Some(WasmType::I32),
            ),
            // _req_param: (string param_name) -> string (pointer)
            ("_req_param", vec![BuiltinType::String], Some(WasmType::I32)),
        ];

        for (func_name, param_types, wasm_return) in http_server_functions {
            // Get the raw import index from http_import_indices
            let raw_func_index = match self.wasm_generator.http_import_indices.get(func_name) {
                Some(&idx) => idx,
                None => {
                    // Function wasn't registered as an import - skip
                    tracing::debug!(
                        name = %func_name,
                        "HTTP server function not registered as import, skipping wrapper"
                    );
                    continue;
                }
            };

            // Build wrapper parameters (Clean Language types)
            let wrapper_params: Vec<WasmType> = param_types
                .iter()
                .map(|t| Self::builtin_type_to_wasm_type(t))
                .collect();

            // Build wrapper instructions
            let mut wrapper_instructions = Vec::new();
            let mut local_idx = 0u32;

            for param_type in param_types.iter() {
                if matches!(param_type, BuiltinType::String) {
                    // For strings: expand to (ptr+4, len)
                    // Clean strings are length-prefixed: [4-byte len][content]
                    // ptr+4 = content start, load from ptr = length

                    // Push content pointer (ptr + 4)
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    wrapper_instructions.push(Instruction::I32Const(4));
                    wrapper_instructions.push(Instruction::I32Add);

                    // Push length (load i32 from ptr)
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    wrapper_instructions.push(Instruction::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                    local_idx += 1;
                } else {
                    // Non-string: pass through
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    local_idx += 1;
                }
            }

            // Call the raw import
            wrapper_instructions.push(Instruction::Call(raw_func_index));

            // Create a wrapper name that will be used for lookups
            let wrapper_name = format!("{}_wrapper", func_name);

            tracing::debug!(
                name = %func_name,
                wrapper_name = %wrapper_name,
                params = ?wrapper_params,
                returns = ?wasm_return,
                raw_func_index = raw_func_index,
                function_count = self.wasm_generator.function_count,
                "Registering HTTP server wrapper function for string expansion"
            );

            // Register wrapper function
            let wrapper_index = self.wasm_generator.register_function(
                &wrapper_name,
                &wrapper_params,
                wasm_return,
                &wrapper_instructions,
            )?;

            // CRITICAL: Map the original function name to the wrapper index
            // This ensures that calls to _http_route use the wrapper, not the raw import
            self.wasm_generator
                .function_map
                .insert(func_name.to_string(), wrapper_index);

            // CRITICAL: Also update http_import_indices to point to wrapper
            // MIR codegen uses http_import_indices for HTTP function lookups
            self.wasm_generator
                .http_import_indices
                .insert(func_name.to_string(), wrapper_index);

            tracing::debug!(
                name = %func_name,
                wrapper_index = wrapper_index,
                "Mapped {} to wrapper function index {} (both function_map and http_import_indices)",
                func_name,
                wrapper_index
            );
        }

        Ok(())
    }

    /// Convert BuiltinType to WASM type
    fn builtin_type_to_wasm_type(
        bt: &crate::builtins::registry::BuiltinType,
    ) -> crate::types::WasmType {
        use crate::builtins::registry::BuiltinType;
        use crate::types::WasmType;

        match bt {
            BuiltinType::Integer => WasmType::I32,
            BuiltinType::Number => WasmType::F64,
            BuiltinType::String => WasmType::I32, // String pointer
            BuiltinType::Boolean => WasmType::I32,
            BuiltinType::Void => WasmType::I32, // Fallback, shouldn't be used
            BuiltinType::List(_) => WasmType::I32, // List pointer
            BuiltinType::Matrix(_) => WasmType::I32, // Matrix pointer
            BuiltinType::Pairs(_, _) => WasmType::I32, // Pairs pointer
            BuiltinType::Namespace => WasmType::I32, // Namespace as i32
            BuiltinType::Any => WasmType::I32,  // Any as i32 pointer
        }
    }
}

/// Statistics for function generation
#[derive(Debug, Default)]
struct FunctionStats {
    blocks_generated: usize,
    instructions_generated: usize,
}

impl<'a> Default for MirCodeGenerator<'a> {
    fn default() -> Self {
        Self::new()
    }
}
