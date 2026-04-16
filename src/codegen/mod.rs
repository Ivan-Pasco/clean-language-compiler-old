//! Module for WebAssembly code generation.

use wasm_encoder::{
    CodeSection, ConstExpr, EntityType, ExportSection, FunctionSection, GlobalSection, GlobalType,
    ImportSection, MemorySection, ValType,
};

use crate::ast::{Class, Function as AstFunction, Type};
use crate::error::{CompilationErrorKind, CompilerError, EnhancedErrorCollector};

use crate::types::WasmType;
use std::collections::{HashMap, HashSet};

// Declare the modules
mod binaryen_optimizer;
// NOTE: builtin_generator.rs contains additional impl blocks for CodeGenerator but is NOT
// compiled as a module. The critical native functions (lastIndexOf, startsWith, etc.) are
// registered via register_native_string_operations() below instead.
pub mod bridge_generator;
pub mod const_eval;
mod instruction_generator;
mod memory;
pub mod mir_codegen;
pub mod native_stdlib;
pub mod optimizations;
mod stdlib_generator;
mod type_conversion;
mod type_manager;
// Legacy wasm_generator module removed - use mir_codegen instead
mod codegen_generation;
mod codegen_module_builder;
mod codegen_registration;
mod codegen_utilities;
mod wasm_module_builder;

#[cfg(test)]
mod tests;

// Import the StringPool struct
use self::memory::MemoryUtils;
use binaryen_optimizer::BinaryenOptimizer;
use instruction_generator::{InstructionGenerator, LocalVarInfo};
pub use mir_codegen::{MirCodeGenerator, MirCodegenResult};
use type_conversion::TypeConverter;
use type_manager::TypeManager;
use wasm_module_builder::WasmModuleBuilder;

// Add these constants for memory type IDs
pub const INTEGER_TYPE_ID: u32 = 1;
pub const FLOAT_TYPE_ID: u32 = 2;
pub const STRING_TYPE_ID: u32 = 3;
pub const LIST_TYPE_ID: u32 = 4;
pub const MATRIX_TYPE_ID: u32 = 5;
pub const PAIRS_TYPE_ID: u32 = 6;

// Memory constants
pub const PAGE_SIZE: u32 = 65536;
pub const HEADER_SIZE: u32 = 16; // 16-byte header for memory blocks
pub const MIN_ALLOCATION: u32 = 16;
pub const HEAP_START: usize = 1024; // Start heap at 1KB, leaving room for static data
pub const DEFAULT_MAX_MEMORY_PAGES: u64 = 1024; // 64MB max - generous for SSR, no cost until used

/// Code generator for Clean Language
pub struct CodeGenerator {
    function_section: FunctionSection,
    export_section: ExportSection,
    code_section: CodeSection,
    memory_section: MemorySection,
    global_section: GlobalSection,

    import_section: ImportSection,
    type_manager: TypeManager,
    instruction_generator: InstructionGenerator,
    enhanced_error_collector: EnhancedErrorCollector,
    variable_map: HashMap<String, LocalVarInfo>,
    memory_utils: MemoryUtils,
    function_count: u32,
    current_function_params: Vec<LocalVarInfo>, // Parameters (indices 0, 1, 2...)
    current_function_locals: Vec<LocalVarInfo>, // Actual locals (indices param_count+0, param_count+1...)
    current_function_param_count: u32,          // Track parameter count for proper local indexing
    function_map: HashMap<String, u32>,
    function_names: Vec<String>,
    function_definitions: HashMap<String, AstFunction>, // Store function definitions for default parameter handling
    file_import_indices: HashMap<String, u32>,
    http_import_indices: HashMap<String, u32>,

    // Class and inheritance support
    current_class_context: Option<String>,
    class_field_map: HashMap<String, HashMap<String, (Type, u32)>>, // class_name -> (field_name -> (type, offset))
    class_table: HashMap<String, Class>,

    // String management for imports
    string_offset_counter: u32,
    string_pool: HashMap<String, u32>,

    // Variable type tracking for automatic toString() conversion
    variable_types: HashMap<String, Type>, // Track original Clean Language types

    // Add missing fields
    label_counter: u32,

    // Loop depth tracking for break/continue statements
    // Stores the label depth of the current loop's outer block (for break)
    // and loop block (for continue). Stack for nested loops.
    loop_break_labels: Vec<u32>,    // Stack of break target labels
    loop_continue_labels: Vec<u32>, // Stack of continue target labels
    current_block_depth: u32,       // Current nested block depth

    // Variable tracking for automatic getter generation
    start_function_variables: HashMap<String, (Type, i32)>, // variable_name -> (type, constant_value)

    // Configuration for runtime imports
    include_runtime_imports: bool,

    // Reachability-based tree-shaking for Layer 2/3 external-I/O imports.
    //
    // None = no filtering (legacy behaviour; emit every registered import).
    // Some(set) = filter mode: imports whose field name is "reachability-gated"
    // (see `is_reachability_gated_import`) are only emitted if the set contains
    // the field name. Internal runtime imports (print, math, memory, string
    // ops, type conversion, list ops, JSON) always emit regardless.
    //
    // The MIR codegen populates this from the call-graph before any
    // register_*_imports() / register_*_operations() call.
    reachable_imports: Option<HashSet<String>>,

    // Track imported function names to avoid exporting them
    imported_functions: HashSet<String>,

    // WASM module builder for assembling final module
    module_builder: WasmModuleBuilder,

    // Type converter
    type_converter: TypeConverter,

    // Binaryen optimizer for WebAssembly optimization
    binaryen_optimizer: Option<BinaryenOptimizer>,

    // Serialised plugin registrations to embed in the WASM custom section.
    // Set via `set_plugin_registrations` before calling `generate`.
    plugin_registrations: Option<crate::plugins::PluginRegistrations>,
}

impl Default for CodeGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGenerator {
    /// Create a new code generator with full runtime imports
    pub fn new() -> Self {
        Self::new_with_config(true)
    }

    /// Create a new code generator for testing without runtime imports
    pub fn new_minimal() -> Self {
        Self::new_with_config(false)
    }

    /// Create a new code generator with configurable runtime imports
    fn new_with_config(include_runtime_imports: bool) -> Self {
        let type_manager = TypeManager::new();
        let instruction_generator = InstructionGenerator::new(type_manager.clone());
        let _stdlib_type_manager = type_manager.clone();
        let _stdlib_instruction_generator = InstructionGenerator::new(type_manager.clone());

        // Create global section with heap pointer global at index 0
        // HEAP_PTR_GLOBAL (index 0) is a mutable i32 initialized to HEAP_START
        let mut global_section = GlobalSection::new();
        global_section.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: true,
            },
            &ConstExpr::i32_const(native_stdlib::HEAP_START as i32),
        );

        Self {
            function_section: FunctionSection::new(),
            export_section: ExportSection::new(),
            code_section: CodeSection::new(),
            memory_section: MemorySection::new(),
            global_section,

            import_section: ImportSection::new(),
            type_manager,
            instruction_generator,
            enhanced_error_collector: EnhancedErrorCollector::new(),
            variable_map: HashMap::new(),
            memory_utils: MemoryUtils::new(HEAP_START), // Start at proper heap location (64KB)
            function_count: 0,
            current_function_params: Vec::new(),
            current_function_locals: Vec::new(),
            current_function_param_count: 0,
            function_map: HashMap::new(),
            function_names: Vec::new(),
            function_definitions: HashMap::new(),
            file_import_indices: HashMap::new(),
            http_import_indices: HashMap::new(),

            // Class and inheritance support
            current_class_context: None,
            class_field_map: HashMap::new(),
            class_table: HashMap::new(),

            // String management for imports
            string_offset_counter: 4096, // Start at 4KB to avoid conflicts
            string_pool: HashMap::new(),

            // Variable type tracking for automatic toString() conversion
            variable_types: HashMap::new(),

            // Add missing fields
            label_counter: 0,

            // Loop depth tracking for break/continue
            loop_break_labels: Vec::new(),
            loop_continue_labels: Vec::new(),
            current_block_depth: 0,

            // Result tracking for get_result function generation
            // Variable tracking for automatic getter generation
            start_function_variables: HashMap::new(),

            // Configuration for runtime imports
            include_runtime_imports,

            // Tree-shaking disabled by default; MirCodegen opts in.
            reachable_imports: None,

            // Track imported function names to avoid exporting them
            imported_functions: HashSet::new(),
            module_builder: WasmModuleBuilder::new(include_runtime_imports),
            type_converter: TypeConverter::new(),
            binaryen_optimizer: None, // Will be configured based on optimization level
            plugin_registrations: None,
        }
    }

    /// Attach plugin lifecycle registrations so they are embedded as a custom
    /// section in the generated WASM binary.
    ///
    /// Call this before invoking `generate` / `assemble_module`.  The section
    /// is only written when there is at least one non-empty registration list,
    /// so calling this with an all-default `PluginRegistrations` is a no-op.
    pub fn set_plugin_registrations(&mut self, registrations: crate::plugins::PluginRegistrations) {
        self.plugin_registrations = Some(registrations);
    }

    /// Enable production-level optimization using Binaryen
    pub fn enable_production_optimization(&mut self) {
        self.binaryen_optimizer = Some(BinaryenOptimizer::for_production());
    }

    /// Enable size-optimized compilation for web deployment
    pub fn enable_size_optimization(&mut self) {
        self.binaryen_optimizer = Some(BinaryenOptimizer::for_size_optimization());
    }

    /// Enable speed-optimized compilation for maximum performance
    pub fn enable_speed_optimization(&mut self) {
        self.binaryen_optimizer = Some(BinaryenOptimizer::for_speed_optimization());
    }

    /// Enable development mode with debugging support
    pub fn enable_development_mode(&mut self) {
        self.binaryen_optimizer = Some(BinaryenOptimizer::for_development());
    }

    /// Disable all optimizations
    pub fn disable_optimization(&mut self) {
        self.binaryen_optimizer = None;
    }

    /// Set custom Binaryen optimizer
    pub fn set_optimizer(&mut self, optimizer: BinaryenOptimizer) {
        self.binaryen_optimizer = Some(optimizer);
    }

    /// Create a new CodeGenerator with imports registered for testing
    #[cfg(test)]
    pub fn new_for_testing() -> Result<Self, CompilerError> {
        let mut codegen = Self::new();

        // Register imports needed for testing
        codegen.register_print_imports()?;
        codegen.register_console_imports()?;
        codegen.register_file_imports()?;
        codegen.register_http_imports(&HashSet::new(), false)?;
        codegen.register_type_conversion_imports()?;
        codegen.register_method_style_imports()?;
        // Register native memory operations (includes native int_to_string, bool_to_string, etc.)
        codegen.register_memory_operations()?;

        Ok(codegen)
    }

    /// Configure the memory section with standard settings
    fn setup_memory_section(&mut self) {
        self.memory_section.memory(wasm_encoder::MemoryType {
            minimum: 32, // 32 pages = 2MB initial memory (1MB data section + 1MB heap)
            maximum: Some(DEFAULT_MAX_MEMORY_PAGES), // 64MB max - physical memory only committed on grow
            memory64: false,
            shared: false,
        });
    }

    /// Helper method for tests to set up memory and exports
    pub fn setup_for_testing(&mut self) -> Result<(), CompilerError> {
        // Register imports FIRST (they get indices 0-13) - just like in generate()
        self.register_print_imports()?;
        // File and HTTP imports temporarily disabled for debugging stack validation issues
        // self.register_file_imports()?;
        // self.register_http_imports()?;
        // Enable type conversion imports - CRITICAL for runtime functionality
        self.register_type_conversion_imports()?;
        // Enable method-style function imports - CRITICAL for method calls
        self.register_method_style_imports()?;

        // Set up memory section
        self.setup_memory_section();

        // Export all registered functions
        for (func_name, &func_index) in &self.function_map.clone() {
            self.export_section
                .export(func_name, wasm_encoder::ExportKind::Func, func_index);
        }
        self.export_section
            .export("memory", wasm_encoder::ExportKind::Memory, 0);

        Ok(())
    }

    /// Helper method for tests to generate complete WASM module
    pub fn generate_test_module(&mut self) -> Result<Vec<u8>, CompilerError> {
        self.setup_for_testing()?;
        self.assemble_module()
    }

    /// Prepare function type information without generating code
    fn prepare_function_type(&mut self, function: &AstFunction) -> Result<(), CompilerError> {
        // Convert parameter types to WebAssembly types
        let param_types: Vec<WasmType> = function
            .parameters
            .iter()
            .map(|p| self.ast_type_to_wasm_type(&p.type_))
            .collect::<Result<Vec<WasmType>, CompilerError>>()?;

        // Convert return type to WebAssembly type
        let return_type = if function.return_type == Type::Void {
            None
        } else {
            Some(self.ast_type_to_wasm_type(&function.return_type)?)
        };

        // Add function type to type section
        let type_index = self.add_function_type(&param_types, return_type)?;

        // Add function to function section
        self.function_section.function(type_index);

        // Store function information
        self.function_map
            .insert(function.name.clone(), self.function_count);
        self.function_names.push(function.name.clone());
        // Store function type information for later use
        // Note: We'll use our own FuncType struct instead of wasmparser's

        self.function_count += 1;
        Ok(())
    }

    fn ast_type_to_wasm_type(&self, ast_type: &Type) -> Result<WasmType, CompilerError> {
        // Delegate to the extracted TypeConverter
        self.type_converter.ast_type_to_wasm_type(ast_type)
    }

    fn types_compatible(&self, from: &WasmType, to: &WasmType) -> bool {
        // Delegate to the extracted TypeConverter
        self.type_converter.types_compatible(from, to)
    }

    /// Assemble the final WebAssembly module
    fn assemble_module(&mut self) -> Result<Vec<u8>, CompilerError> {
        // Use the extracted WasmModuleBuilder to assemble the module
        let base_wasm = self.module_builder.assemble_module(
            &self.type_manager,
            &self.import_section,
            &self.function_section,
            &self.memory_section,
            &self.global_section,
            &self.export_section,
            &self.code_section,
            self.memory_utils.get_data_section(),
            self.function_count,
        )?;

        // Apply Binaryen optimization if configured
        let mut wasm = if let Some(ref optimizer) = self.binaryen_optimizer {
            // Check if wasm-opt is available before attempting optimization
            if !BinaryenOptimizer::is_available() {
                log::info!("Binaryen wasm-opt not available. Consider installing Binaryen for optimization.");
                log::info!("Install instructions: https://github.com/WebAssembly/binaryen");
                base_wasm
            } else {
                match optimizer.optimize(&base_wasm) {
                    Ok((optimized_wasm, stats)) => {
                        log::info!("Binaryen optimization completed:");
                        log::info!("  Original size: {} bytes", stats.original_size);
                        log::info!("  Optimized size: {} bytes", stats.optimized_size);
                        log::info!("  Size reduction: {:.2}%", stats.size_reduction_percent);
                        log::info!("  Optimization time: {}ms", stats.optimization_time_ms);
                        optimized_wasm
                    }
                    Err(e) => {
                        log::warn!(
                            "Binaryen optimization failed: {}, using unoptimized WASM",
                            e
                        );
                        base_wasm
                    }
                }
            }
        } else {
            base_wasm
        };

        // Append a `clean_plugin_registrations` custom section when there are
        // lifecycle registrations to embed.  Custom sections are ignored by all
        // WASM runtimes (they never affect execution) but can be read by tools
        // and frameworks that need to know which lifecycle hooks are active.
        if let Some(ref registrations) = self.plugin_registrations {
            let has_any = !registrations.server.is_empty()
                || !registrations.cli.is_empty()
                || !registrations.data.is_empty()
                || !registrations.build.is_empty();

            if has_any {
                match serde_json::to_vec(registrations) {
                    Ok(json_bytes) => {
                        // Encode the custom section using wasm_encoder and append
                        // the raw bytes to the assembled module.
                        let custom = wasm_encoder::CustomSection {
                            name: std::borrow::Cow::Borrowed("clean_plugin_registrations"),
                            data: std::borrow::Cow::Borrowed(&json_bytes),
                        };
                        use wasm_encoder::Encode;
                        // Section ID 0 (custom) must be written first, then the
                        // encoded payload produced by CustomSection::encode().
                        let mut section_bytes: Vec<u8> = Vec::new();
                        section_bytes.push(0u8); // custom section ID
                        custom.encode(&mut section_bytes);
                        wasm.extend_from_slice(&section_bytes);
                        log::debug!(
                            "Embedded clean_plugin_registrations custom section ({} bytes)",
                            json_bytes.len()
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to serialise plugin registrations for WASM custom section: {}",
                            e
                        );
                    }
                }
            }
        }

        Ok(wasm)
    }

    fn add_function_type(
        &mut self,
        params: &[WasmType],
        return_type: Option<WasmType>,
    ) -> Result<u32, CompilerError> {
        // Use the type manager to add the function type (single return value)
        self.type_manager
            .add_function_type_single(params, return_type)
    }

    fn add_function_type_multi(
        &mut self,
        params: &[WasmType],
        return_types: &[WasmType],
    ) -> Result<u32, CompilerError> {
        // Use the type manager to add the function type (multi-value returns)
        self.type_manager.add_function_type(params, return_types)
    }

    pub fn register_import_function(
        &mut self,
        module: &str,
        field: &str,
        params: &[WasmType],
        return_type: Option<WasmType>,
    ) -> Result<u32, CompilerError> {
        // NOTE: Check if function is already registered to prevent duplicates
        // This allows multiple registration calls to be idempotent
        if let Some(&existing_index) = self.function_map.get(field) {
            tracing::debug!(
                function = field,
                existing_index = existing_index,
                "Function already registered, returning existing index"
            );
            return Ok(existing_index);
        }

        // Reachability-gated tree-shaking (Import Minimality Rule,
        // platform-architecture/EXECUTION_LAYERS.md).
        //
        // Skip Layer 2/3 external-I/O imports that are not reachable from any
        // MIR call. Internal runtime imports (print, math, memory, string ops,
        // type conversion, list ops, JSON) are never filtered here because
        // they are invoked from synthesized codegen without a 1:1 MIR call
        // name.
        if let Some(reachable) = &self.reachable_imports {
            if is_reachability_gated_import(field) && !reachable.contains(field) {
                tracing::debug!(
                    function = field,
                    "Skipping unused reachability-gated import (tree-shake)"
                );
                // Return a sentinel index. Nothing calls an unreachable
                // import, so this value is never consumed.
                return Ok(u32::MAX);
            }
        }

        let type_index = self.add_function_type(params, return_type)?;
        self.import_section
            .import(module, field, EntityType::Function(type_index));
        let func_index = self.function_count;
        self.function_map.insert(field.to_string(), func_index);
        self.function_names.push(field.to_string());

        // Also store the function type information in the instruction generator
        let wasm_params: Vec<wasm_encoder::ValType> = params.iter().map(|t| (*t).into()).collect();
        let wasm_results: Vec<wasm_encoder::ValType> =
            return_type.map_or_else(Vec::new, |t| vec![t.into()]);
        self.instruction_generator
            .add_function_type(func_index, wasm_params, wasm_results);

        self.function_count += 1;

        tracing::debug!(
            function = field,
            index = func_index,
            "Registered new import function"
        );

        Ok(func_index)
    }

    /// Get function index from function map (safe alternative to hardcoded indices)
    pub fn get_function_index(&self, function_name: &str) -> Option<u32> {
        self.function_map.get(function_name).copied()
    }

    /// Get the next function index that will be assigned
    /// This is useful for forward references in mutually recursive functions
    pub fn get_next_function_index(&self) -> u32 {
        self.function_count
    }

    /// Get function index from function map with error handling
    pub fn get_function_index_or_error(&self, function_name: &str) -> Result<u32, CompilerError> {
        self.function_map
            .get(function_name)
            .copied()
            .ok_or_else(|| {
                let error = self
                    .enhanced_error_collector
                    .create_compilation_error(
                        CompilationErrorKind::FunctionNotFound,
                        format!("Function '{}' not found in function map", function_name),
                        None,
                    )
                    .with_help(format!(
                        "Available functions: {:?}",
                        self.function_map.keys().collect::<Vec<_>>()
                    ))
                    .with_suggestion(format!(
                        "Define the function '{}' before calling it",
                        function_name
                    ))
                    .build();
                error.into_compiler_error()
            })
    }
    /// Enable reachability-based filtering of Layer 2/3 external-I/O imports.
    ///
    /// After this is set, `register_import_function` will skip any gated
    /// import (see `is_reachability_gated_import`) whose field name is not
    /// present in `reachable`.
    pub fn set_reachable_imports(&mut self, reachable: HashSet<String>) {
        self.reachable_imports = Some(reachable);
    }
} // impl CodeGenerator (constructors and basic setup)

/// Returns true if the WASM import `field` name is a Layer 3 server-only
/// function whose emission should be gated on reachability.
///
/// Only Layer 3 server imports are gated here: HTTP server routing, request
/// context, response, session, and auth. A client-only Clean program (e.g.
/// `plugins: frame.ui`) does not reference any of these, and the host it
/// runs on (e.g. a browser) cannot provide them — so emitting them would
/// force every host to stub ~30 server functions that will never be called.
///
/// Layer 2 categories (http client, file I/O, crypto, database) are NOT
/// gated here because Clean's stdlib classes (`HttpClass`, `FileClass`,
/// `JsonClass`, etc.) generate WASM wrapper functions that reference these
/// imports at registration time regardless of user-program usage. Filtering
/// them would require refactoring the stdlib class registration to be lazy.
///
/// Internal imports (print, math, memory, string ops, type conversion,
/// list ops, JSON) are never filtered — they are invoked from synthesized
/// codegen paths without a 1:1 MIR call name.
///
/// Spec: platform-architecture/EXECUTION_LAYERS.md — Import Minimality Rule.
pub(crate) fn is_reachability_gated_import(field: &str) -> bool {
    // HTTP server / request / response (Layer 3)
    if field.starts_with("_http_") || field.starts_with("_req_") || field.starts_with("_res_") {
        return true;
    }
    // Session / auth (Layer 3)
    if field.starts_with("_session_") || field.starts_with("_auth_") {
        return true;
    }
    false
}

/// Generate WebAssembly from MIR with minimal runtime (for testing)
pub fn generate_wasm_from_mir_minimal(
    mir_program: crate::mir::MirProgram,
) -> Result<Vec<u8>, CompilerError> {
    let mut mir_codegen = MirCodeGenerator::new_minimal();

    match mir_codegen.generate(mir_program) {
        Ok(result) => Ok(result.wasm_bytes),
        Err(errors) => {
            // Return the first error for now
            if let Some(error) = errors.into_iter().next() {
                Err(error)
            } else {
                Err(CompilerError::Codegen {
                    context: Box::new(crate::error::ErrorContext::new(
                        "Unknown error during MIR code generation",
                        None,
                        crate::error::ErrorType::Codegen,
                        Some(crate::ast::SourceLocation::default()),
                    )),
                })
            }
        }
    }
}
