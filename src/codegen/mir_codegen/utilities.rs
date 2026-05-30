//! Utility helpers for MIR code generation.
//!
//! Contains:
//! * Type helpers: `get_value_type`, `get_operand_mir_type`, `get_stdlib_return_type`
//! * Function signature conversion: `convert_function_signature`, `mir_type_to_wasm_type`
//! * Local/block computation: `compute_local_types`, `compute_block_order`
//! * Function resolution: `resolve_namespace_function`, `get_function_name_by_symbol`
//! * Module setup: `register_builtin_function_signatures`, `setup_memory_section`,
//!   `setup_string_pool`, `add_function_to_module`, `val_type_to_wasm_type`
//! * Export helpers: `generate_start_function_export`
//! * JSON helpers: `get_or_register_json_get_field`, `get_or_register_json_get_index`
//! * Module finalisation: `finalize_module`
//! * Bridge/import registration: `collect_used_function_names_from_mir`,
//!   `register_plugin_bridge_imports`, `register_pending_bridge_wrappers`,
//!   `register_http_server_wrappers`, `register_external_function_imports`
//! * Type converters: `builtin_type_to_wasm_type`, `mir_type_to_wasm_type_for_import`

use super::*;
use wasm_encoder::{Function as WasmFunction, Instruction, ValType};

impl MirCodeGenerator<'_> {
    // -------------------------------------------------------------------------
    // Type helpers
    // -------------------------------------------------------------------------

    /// Get the MIR type of a `ValueId` from the current function's locals.
    pub(super) fn get_value_type(&self, value_id: ValueId) -> Option<MirType> {
        self.current_function
            .as_ref()
            .and_then(|func| func.locals.get(&value_id))
            .map(|local| local.local_type.clone())
    }

    /// Get the MIR type of a `MirOperand`.
    pub(super) fn get_operand_mir_type(&self, operand: &MirOperand) -> Option<MirType> {
        match operand {
            MirOperand::Value(vid) => self.get_value_type(*vid),
            MirOperand::Constant(constant) => Some(match constant {
                MirConstant::Integer(_) => MirType::I32,
                MirConstant::Float(_) => MirType::F64,
                MirConstant::Boolean(_) => MirType::I32,
                MirConstant::String(_) => MirType::I32, // String pointers are i32
                MirConstant::Null => MirType::I32,
                MirConstant::Undefined => MirType::I32,
                MirConstant::Array(_) => MirType::I32, // Array pointers are i32
                MirConstant::Struct(_) => MirType::I32, // Struct pointers are i32
            }),
            MirOperand::Function(_) => Some(MirType::I32),
            MirOperand::NamedFunction { .. } => Some(MirType::I32),
            MirOperand::Global(_) => Some(MirType::I32),
        }
    }

    /// Get the return type for stdlib functions by name.
    ///
    /// Used for namespace functions (SymbolId(0)) where signature lookup by ID fails.
    pub(super) fn get_stdlib_return_type(&self, function_name: &str) -> Option<MirType> {
        // First check the registered return type registry (populated during init)
        if let Some(return_type) = self.function_return_types.get(function_name) {
            return Some(return_type.clone());
        }

        // Fallback: namespace-based heuristics for functions not explicitly registered
        match function_name {
            name if name.starts_with("string.") => Some(MirType::I32),
            name if name.starts_with("list.") => Some(MirType::I32),
            name if name.starts_with("http.") => Some(MirType::I32),
            name if name.starts_with("file.") => Some(MirType::I32),
            _ => None,
        }
    }

    // -------------------------------------------------------------------------
    // Function signature conversion
    // -------------------------------------------------------------------------

    /// Convert a MIR function signature to WASM `(params, results)` types.
    pub(super) fn convert_function_signature(
        &self,
        function: &MirFunction,
    ) -> Result<(Vec<ValType>, Vec<ValType>), CompilerError> {
        let mut param_types = Vec::new();
        let mut result_types = Vec::new();

        for (i, param) in function.parameters.iter().enumerate() {
            let val_type = self.mir_type_to_wasm_type(&param.param_type)?;

            if function.name == "logMessage" || function.name == "buildUrl" {
                debug_mir!(
                    "DEBUG PARAM CONVERSION ITERATION[{}]: function='{}' param='{}' mir_type={:?} val_type={:?} param_types_len_before={}",
                    i, function.name, param.name, param.param_type, val_type, param_types.len()
                );
            }

            param_types.push(val_type);

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
                // String returns are single pointer to [len|content] structure in memory
                result_types.push(ValType::I32);
                debug_mir!(
                    "Converted to WASM result_types: [I32] (string tuple as memory pointer)"
                );
            }
            MirType::Void => {
                debug_mir!("Converted to WASM result_types: [] (void)");
            }
            MirType::Any => {
                result_types.push(ValType::I32);
                debug_mir!("Converted Any to WASM result_types: [I32] (boxed value pointer)");
            }
            MirType::Ptr(inner) => {
                if matches!(**inner, MirType::Void) {
                    debug_mir!("Converted Ptr(Void) to WASM result_types: [] (void)");
                } else {
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

    /// Convert a `MirType` to a WASM `ValType`.
    pub(super) fn mir_type_to_wasm_type(
        &self,
        mir_type: &MirType,
    ) -> Result<ValType, CompilerError> {
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

            MirType::Ptr(_) => Ok(ValType::I32),

            MirType::StringTuple => {
                // As a parameter type, StringTuple is a pointer to the string structure
                Ok(ValType::I32)
            }

            MirType::Any => {
                // Any type is a pointer to boxed value: [tag:i32][value1:i32][value2:i32]
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

    // -------------------------------------------------------------------------
    // Local / block computation
    // -------------------------------------------------------------------------

    /// Compute WASM local variable types for a function.
    ///
    /// Returns `(count, ValType)` pairs for locals AFTER the parameters.
    pub(super) fn compute_local_types(&self, function: &MirFunction) -> Vec<(u32, ValType)> {
        let mut local_types_map = std::collections::HashMap::new();

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

        // Auto-allocated locals that were created during code generation
        for (value_id, &local_index) in &self.value_to_local {
            local_types_map.entry(local_index).or_insert_with(|| {
                let wasm_type = if let Some(mir_type) = self.value_to_type.get(value_id) {
                    self.mir_type_to_wasm_type(mir_type).unwrap_or(ValType::I32)
                } else {
                    ValType::I32
                };
                tracing::trace!(
                    local_index = local_index,
                    value_id = ?value_id,
                    wasm_type = ?wasm_type,
                    "Auto-allocated local type"
                );
                wasm_type
            });
        }

        // Tracked temporary locals (e.g., string expansion in load_string_argument_for_print)
        for (&local_index, &wasm_type) in &self.temp_local_types {
            local_types_map.entry(local_index).or_insert_with(|| {
                debug_mir!(
                    "DEBUG MIR: Adding temporary local {} with tracked type {:?}",
                    local_index,
                    wasm_type
                );
                wasm_type
            });
        }

        // Build vec of (count, type) pairs — only locals AFTER parameters
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

    /// Compute basic block ordering for code generation.
    ///
    /// Reserved for future optimisation passes; currently produces entry-block-first order.
    #[allow(dead_code)] // Block ordering utility — not yet called from the main codegen loop
    pub(super) fn compute_block_order(&self, function: &MirFunction) -> Vec<BasicBlockId> {
        let mut order = vec![function.entry_block];
        for &block_id in function.blocks.keys() {
            if block_id != function.entry_block {
                order.push(block_id);
            }
        }
        order
    }

    // -------------------------------------------------------------------------
    // Function resolution
    // -------------------------------------------------------------------------

    /// Resolve a namespace function `SymbolId` to a WASM function name.
    #[allow(dead_code)] // Namespace-by-ID resolver — superseded by string-keyed lookup in mod.rs
    pub(super) fn resolve_namespace_function(&self, symbol_id: SymbolId) -> Option<String> {
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
            48 => Some("string.length".to_string()),
            49 => Some("string.substring".to_string()),
            50 => Some("string.toUpperCase".to_string()),
            51 => Some("string.toLowerCase".to_string()),
            52 => Some("string.contains".to_string()),
            53 => Some("list.size".to_string()),
            54 => Some("list.push".to_string()),
            55 => Some("list.pop".to_string()),
            56 => Some("list.get".to_string()),
            60 => Some("number.toString".to_string()),
            61 => Some("math.max".to_string()),
            62 => Some("integer.toString".to_string()),
            63 => Some("boolean.toString".to_string()),
            64 => Some("string.length".to_string()),
            65 => Some("string.substring".to_string()),
            66 => Some("string.contains".to_string()),
            67 => Some("string.contains".to_string()),
            68 => Some("string.length".to_string()),
            69 => Some("string.isEmpty".to_string()),
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
            _ => {
                tracing::debug!(
                    symbol_id = symbol_id.0,
                    "Unknown namespace function SymbolId"
                );
                None
            }
        }
    }

    /// Get a function name by `SymbolId` using pure dynamic resolution.
    ///
    /// All symbols (builtins + user-defined) are resolved from `function_symbol_map`
    /// which is populated during the pre-registration phase of `generate()`.
    pub(super) fn get_function_name_by_symbol(&self, symbol_id: SymbolId) -> Option<String> {
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

    // -------------------------------------------------------------------------
    // Module setup
    // -------------------------------------------------------------------------

    /// Register built-in function signatures so the codegen knows return types.
    pub(super) fn register_builtin_function_signatures(&mut self) {
        use crate::mir::mir_types::{BasicBlockId, MirFunction, MirFunctionAttributes, MirType};
        use std::collections::HashMap;

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

        // Void-returning functions
        self.function_signatures.insert(
            SymbolId(0),
            create_builtin_signature("print", 0, MirType::Void),
        );
        self.function_signatures.insert(
            SymbolId(1),
            create_builtin_signature("printl", 1, MirType::Void),
        );

        // Value-returning type conversion functions
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

        // Populate name-based return type registry for stdlib/builtin functions

        // Void functions
        for name in &["print", "printl", "list.set", "list.clear", "list.push"] {
            self.function_return_types
                .insert(name.to_string(), MirType::Void);
        }

        // Math functions returning F64
        for name in &[
            "math.abs",
            "math.sqrt",
            "math.sin",
            "math.cos",
            "math.tan",
            "math.asin",
            "math.acos",
            "math.atan",
            "math.atan2",
            "math.sinh",
            "math.cosh",
            "math.tanh",
            "math.ln",
            "math.log10",
            "math.log2",
            "math.exp",
            "math.exp2",
            "math.floor",
            "math.ceil",
            "math.round",
            "math.trunc",
            "math.sign",
            "math.pow",
            "math.max",
            "math.min",
            "math.pi",
            "math.e",
            "math.tau",
            "matrix.determinant",
            "string.toNumber",
            "integer.toNumber",
            "boolean.toNumber",
            "number.toNumber",
        ] {
            self.function_return_types
                .insert(name.to_string(), MirType::F64);
        }

        // Functions returning I32
        for name in &[
            "math.abs.i32",
            "string.toInteger",
            "number.toInteger",
            "boolean.toInteger",
            "string.toBoolean",
            "integer.toBoolean",
            "number.toBoolean",
            "matrix.rows",
            "matrix.cols",
            "matrix.size",
        ] {
            self.function_return_types
                .insert(name.to_string(), MirType::I32);
        }

        tracing::debug!(
            count = self.function_return_types.len(),
            "Registered builtin function signatures and return types"
        );
    }

    /// Set up the WASM memory section using the configured memory tier.
    pub(super) fn setup_memory_section(&mut self) -> Result<(), CompilerError> {
        let initial = self.memory_tier.initial_pages();
        let max = self.memory_tier.max_pages();
        debug_mir!(
            "DEBUG MIR: Setting up memory section: tier={}, initial={} pages ({}KB), max={} pages ({}KB)",
            self.memory_tier.name(), initial, initial * 64, max, max * 64
        );
        self.wasm_generator
            .memory_section
            .memory(wasm_encoder::MemoryType {
                minimum: initial,
                maximum: Some(max),
                memory64: false,
                shared: false,
            });
        debug_mir!("DEBUG MIR: Memory section configured");
        Ok(())
    }

    /// Register all strings from `string_pool` into the WASM data section.
    pub(super) fn setup_string_pool(
        &mut self,
        string_pool: &[String],
    ) -> Result<(), CompilerError> {
        debug_mir!(
            "DEBUG MIR: Setting up string pool with {} strings:",
            string_pool.len()
        );
        for (i, s) in string_pool.iter().enumerate() {
            debug_mir!("DEBUG MIR:   String {}: '{}'", i, s);
        }

        self.string_pool = Some(string_pool.to_vec());

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

    /// Add a fully-generated `WasmFunction` to the WASM module.
    pub(super) fn add_function_to_module(
        &mut self,
        name: String,
        wasm_function: WasmFunction,
        signature: (Vec<ValType>, Vec<ValType>),
    ) -> Result<(), CompilerError> {
        let (param_types, return_types) = signature;

        let return_wasm_types: Vec<_> = return_types
            .iter()
            .map(|vt| self.val_type_to_wasm_type(vt))
            .collect::<Result<Vec<_>, _>>()?;

        let param_wasm_types: Vec<_> = param_types
            .iter()
            .map(|vt| self.val_type_to_wasm_type(vt))
            .collect::<Result<Vec<_>, _>>()?;

        tracing::debug!(name = %name, "Registering function");

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

        let type_index = self.wasm_generator.add_function_type(
            &param_wasm_types,
            if return_wasm_types.is_empty() {
                None
            } else {
                Some(return_wasm_types[0])
            },
        )?;

        self.wasm_generator.function_section.function(type_index);
        self.wasm_generator.code_section.function(&wasm_function);
        self.wasm_generator.function_names.push(name.clone());

        tracing::debug!(
            name = %name,
            index = function_index,
            "Function registered with pre-assigned index"
        );
        tracing::debug!(
            entries = self.wasm_generator.function_map.len(),
            "Function map after registration"
        );
        if let Some(&idx) = self.wasm_generator.function_map.get(&name) {
            tracing::trace!(name = %name, index = idx, "Verified function is in map");
        } else {
            tracing::error!(name = %name, "Function was NOT added to function map");
        }

        Ok(())
    }

    /// Convert a WASM `ValType` to the project's `WasmType` enum.
    pub(super) fn val_type_to_wasm_type(
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

    // -------------------------------------------------------------------------
    // Export helpers
    // -------------------------------------------------------------------------

    /// Generate and export a `_start` wrapper that calls the program entry point.
    pub(super) fn generate_start_function_export(
        &mut self,
        entry_symbol_id: SymbolId,
    ) -> Result<(), CompilerError> {
        tracing::debug!("Function map contents:");
        for (name, index) in &self.wasm_generator.function_map {
            tracing::trace!(name = %name, index = index, "Function in map");
        }
        tracing::debug!(
            entries = self.wasm_generator.function_map.len(),
            symbol_id = entry_symbol_id.0,
            "Looking for entry function by SymbolId"
        );

        if let Some(entry_function_index) = self.symbol_to_function_index.get(&entry_symbol_id) {
            let type_index = self
                .wasm_generator
                .type_manager
                .add_function_type_single(&[], None)?;
            self.wasm_generator.function_section.function(type_index);

            // Determine whether the entry function has a non-void return type.
            // If so, `_start` (which is void) must drop the return value so the
            // WASM operand stack is empty at the `end` instruction.
            //
            // Both `MirType::Void` and `MirType::Ptr(Void)` map to an empty WASM
            // result list (see `convert_function_signature`). A `drop` after a call
            // to such a function would attempt to pop from an empty stack, causing
            // WASM validation error E007 ("expected a type but nothing on stack").
            let entry_returns_value = self
                .function_signatures
                .get(&entry_symbol_id)
                .map(|sig| {
                    !matches!(sig.return_type, MirType::Void)
                        && !matches!(&sig.return_type, MirType::Ptr(inner) if matches!(**inner, MirType::Void))
                })
                .unwrap_or(false);

            let mut instructions = Vec::new();
            instructions.push(Instruction::Call(*entry_function_index));
            if entry_returns_value {
                instructions.push(Instruction::Drop);
            }
            instructions.push(Instruction::End);

            let mut start_function = WasmFunction::new(vec![]);
            for instruction in instructions {
                start_function.instruction(&instruction);
            }

            self.wasm_generator.code_section.function(&start_function);

            let start_func_index = self.wasm_generator.function_count;
            self.wasm_generator.export_section.export(
                "_start",
                wasm_encoder::ExportKind::Func,
                start_func_index,
            );

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

    // -------------------------------------------------------------------------
    // Test runner export
    // -------------------------------------------------------------------------

    /// Generate and export a `_run_tests` WASM function.
    ///
    /// The function calls each `__test_*` function in order, prints
    /// `PASS: <name>` or `FAIL: <name>` for each result, and returns
    /// the total failure count as an i32.
    ///
    /// This is a raw WASM instruction emitter, similar to
    /// `generate_start_function_export`, but builds a real function body.
    pub(super) fn generate_run_tests_export(
        &mut self,
        test_functions: &[(crate::resolver::SymbolId, String)],
    ) -> Result<(), CompilerError> {
        if test_functions.is_empty() {
            return Ok(());
        }

        // Look up the `printl` host import index.  The import must have been
        // registered already (happens at the start of `generate()`).
        let printl_idx = match self.wasm_generator.function_map.get("printl").copied() {
            Some(idx) => idx,
            None => {
                return Err(CompilerError::codegen_error(
                    "_run_tests: 'printl' host import not found in function_map".to_string(),
                    None,
                    None,
                ));
            }
        };

        // Pre-compute PASS/FAIL string offsets for each test.
        // String format expected by `printl`: (content_ptr: i32, length: i32)
        // The `get_or_create_string_offset` stores [4-byte-len][content] and
        // returns the base offset.  We pass `base + 4` as content_ptr.
        struct TestEntry {
            func_index: u32,
            pass_content_ptr: i32,
            pass_len: i32,
            fail_content_ptr: i32,
            fail_len: i32,
        }

        let mut entries: Vec<TestEntry> = Vec::with_capacity(test_functions.len());

        for (symbol_id, test_name) in test_functions {
            // Look up the pre-registered function index for this test.
            let func_index = match self.symbol_to_function_index.get(symbol_id).copied() {
                Some(idx) => idx,
                None => {
                    tracing::warn!(
                        test_name = %test_name,
                        symbol_id = symbol_id.0,
                        "Test function not found in symbol_to_function_index, skipping"
                    );
                    continue;
                }
            };

            let pass_str = format!("PASS: {}", test_name);
            let fail_str = format!("FAIL: {}", test_name);

            let pass_base = self.wasm_generator.get_or_create_string_offset(&pass_str)? as i32;
            let fail_base = self.wasm_generator.get_or_create_string_offset(&fail_str)? as i32;

            entries.push(TestEntry {
                func_index,
                pass_content_ptr: pass_base + 4,
                pass_len: pass_str.len() as i32,
                fail_content_ptr: fail_base + 4,
                fail_len: fail_str.len() as i32,
            });
        }

        if entries.is_empty() {
            return Ok(());
        }

        // ----------------------------------------------------------------
        // Build the WASM function body for `_run_tests`.
        //
        // Locals:
        //   local[0] = failure_count (i32)
        //   local[1] = test_result   (i32, receives boolean from __test_*)
        //
        // Pseudo-code:
        //   let failure_count = 0
        //   for each test:
        //       test_result = __test_N()
        //       if test_result != 0:
        //           printl(pass_content_ptr, pass_len)
        //       else:
        //           printl(fail_content_ptr, fail_len)
        //           failure_count += 1
        //   return failure_count
        // ----------------------------------------------------------------

        let mut f = WasmFunction::new(vec![
            (2_u32, ValType::I32), // 2 locals of type i32 (failure_count, test_result)
        ]);

        // local[0] = 0 (failure_count starts at 0)
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::LocalSet(0));

        for entry in &entries {
            // Call the test function -> pushes i32 result on stack
            f.instruction(&Instruction::Call(entry.func_index));
            // Store result in local[1]
            f.instruction(&Instruction::LocalSet(1));

            // if local[1] != 0 { printl(pass); } else { printl(fail); failure_count++ }
            f.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
            f.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));

            // Check if result is zero (test failed)
            f.instruction(&Instruction::LocalGet(1));
            f.instruction(&Instruction::I32Eqz);
            // If zero (failed), break to the outer block (else branch)
            f.instruction(&Instruction::BrIf(0));

            // PASS branch: call printl(pass_content_ptr, pass_len)
            f.instruction(&Instruction::I32Const(entry.pass_content_ptr));
            f.instruction(&Instruction::I32Const(entry.pass_len));
            f.instruction(&Instruction::Call(printl_idx));
            f.instruction(&Instruction::Br(1)); // jump past the outer block

            f.instruction(&Instruction::End); // end inner block

            // FAIL branch: call printl(fail_content_ptr, fail_len) + increment failure_count
            f.instruction(&Instruction::I32Const(entry.fail_content_ptr));
            f.instruction(&Instruction::I32Const(entry.fail_len));
            f.instruction(&Instruction::Call(printl_idx));
            // failure_count += 1
            f.instruction(&Instruction::LocalGet(0));
            f.instruction(&Instruction::I32Const(1));
            f.instruction(&Instruction::I32Add);
            f.instruction(&Instruction::LocalSet(0));

            f.instruction(&Instruction::End); // end outer block
        }

        // Return failure_count
        f.instruction(&Instruction::LocalGet(0));
        f.instruction(&Instruction::End);

        // Register the function type: () -> i32
        let type_index = self
            .wasm_generator
            .type_manager
            .add_function_type_single(&[], Some(crate::types::WasmType::I32))?;

        self.wasm_generator.function_section.function(type_index);
        self.wasm_generator.code_section.function(&f);

        let run_tests_index = self.wasm_generator.function_count;
        self.wasm_generator
            .function_map
            .insert("_run_tests".to_string(), run_tests_index);
        self.wasm_generator
            .function_names
            .push("_run_tests".to_string());
        self.wasm_generator.function_count += 1;

        tracing::debug!(
            func_index = run_tests_index,
            test_count = entries.len(),
            "_run_tests function generated and registered"
        );

        Ok(())
    }

    /// Generate a `_start` WASM function that calls `_run_tests` and drops the result.
    ///
    /// Used when a file has only a `tests:` block and no `start:` function.  The
    /// clean-runner always invokes `_start`; this wrapper makes test-only files runnable.
    pub(super) fn generate_test_start_export(&mut self) -> Result<(), CompilerError> {
        let run_tests_idx = match self.wasm_generator.function_map.get("_run_tests").copied() {
            Some(idx) => idx,
            None => {
                return Err(CompilerError::codegen_error(
                    "generate_test_start_export: '_run_tests' not found in function_map"
                        .to_string(),
                    None,
                    None,
                ));
            }
        };

        let type_index = self
            .wasm_generator
            .type_manager
            .add_function_type_single(&[], None)?;
        self.wasm_generator.function_section.function(type_index);

        let mut f = WasmFunction::new(vec![]);
        f.instruction(&Instruction::Call(run_tests_idx));
        f.instruction(&Instruction::Drop);
        f.instruction(&Instruction::End);
        self.wasm_generator.code_section.function(&f);

        let start_index = self.wasm_generator.function_count;
        self.wasm_generator.export_section.export(
            "_start",
            wasm_encoder::ExportKind::Func,
            start_index,
        );
        self.wasm_generator
            .function_names
            .push("_start".to_string());
        self.wasm_generator.function_count += 1;

        Ok(())
    }

    // -------------------------------------------------------------------------
    // JSON helpers
    // -------------------------------------------------------------------------

    /// Get the function index for `__json_get_field` (JSON object field access).
    pub(super) fn get_or_register_json_get_field(&mut self) -> Result<u32, CompilerError> {
        if let Some(&idx) = self.wasm_generator.function_map.get("__json_get_field") {
            return Ok(idx);
        }
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

    /// Get the function index for `__json_get_index` (JSON array element access).
    pub(super) fn get_or_register_json_get_index(&mut self) -> Result<u32, CompilerError> {
        if let Some(&idx) = self.wasm_generator.function_map.get("__json_get_index") {
            return Ok(idx);
        }
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

    // -------------------------------------------------------------------------
    // Module finalisation
    // -------------------------------------------------------------------------

    /// Finalise the WASM module and return the encoded bytecode.
    pub(super) fn finalize_module(&mut self) -> Result<Vec<u8>, CompilerError> {
        use wasm_encoder::Module;

        let mut module = Module::new();

        // 1. Type section
        let type_section = self.wasm_generator.type_manager.clone_type_section();
        module.section(&type_section);

        // 2. Import section
        let import_section = self.wasm_generator.import_section.clone();
        module.section(&import_section);

        // 3. Function section
        let function_section = self.wasm_generator.function_section.clone();
        module.section(&function_section);

        // 3.5 Table section — emitted only when the module uses first-class
        // function references (MirOperand::Function / NamedFunction as values).
        // The table holds user-function indices so hosts can dispatch them via
        // `call_indirect` using the i32 handle the compiler emitted at the
        // reference site. Exported below as `__indirect_function_table`.
        let emit_function_table = !self.referenced_function_indices.is_empty();
        let mut sorted_referenced_funcs: Vec<u32> =
            self.referenced_function_indices.iter().copied().collect();
        sorted_referenced_funcs.sort_unstable();
        let table_size = sorted_referenced_funcs.last().map(|&n| n + 1).unwrap_or(0);
        if emit_function_table {
            let mut table_section = wasm_encoder::TableSection::new();
            table_section.table(wasm_encoder::TableType {
                element_type: wasm_encoder::RefType::FUNCREF,
                minimum: table_size,
                maximum: Some(table_size),
            });
            module.section(&table_section);
        }

        // 4. Memory section
        let memory_section = self.wasm_generator.memory_section.clone();
        module.section(&memory_section);

        // 4.5 Global section (heap pointer + state variables)
        let heap_start = {
            let string_end = self.wasm_generator.string_offset_counter;
            (string_end + 7) & !7
        };
        let mut global_section = wasm_encoder::GlobalSection::new();
        // Global 0: heap pointer
        global_section.global(
            wasm_encoder::GlobalType {
                val_type: wasm_encoder::ValType::I32,
                mutable: true,
            },
            &wasm_encoder::ConstExpr::i32_const(heap_start as i32),
        );

        // State variable globals (indices start at 1)
        for (symbol_id, name, mir_type, initializer) in &self.state_globals {
            let (val_type, init_expr) = match (mir_type, initializer) {
                (MirType::I32, Some(MirConstant::Integer(n))) => (
                    wasm_encoder::ValType::I32,
                    wasm_encoder::ConstExpr::i32_const(*n as i32),
                ),
                (MirType::F64, Some(MirConstant::Float(f))) => (
                    wasm_encoder::ValType::F64,
                    wasm_encoder::ConstExpr::f64_const(*f),
                ),
                (MirType::Bool, Some(MirConstant::Integer(n))) => (
                    wasm_encoder::ValType::I32,
                    wasm_encoder::ConstExpr::i32_const(*n as i32),
                ),
                (MirType::I32 | MirType::Bool, _) => (
                    wasm_encoder::ValType::I32,
                    wasm_encoder::ConstExpr::i32_const(0),
                ),
                (MirType::F64, _) => (
                    wasm_encoder::ValType::F64,
                    wasm_encoder::ConstExpr::f64_const(0.0),
                ),
                _ => (
                    wasm_encoder::ValType::I32,
                    wasm_encoder::ConstExpr::i32_const(0),
                ),
            };
            global_section.global(
                wasm_encoder::GlobalType {
                    val_type,
                    mutable: true,
                },
                &init_expr,
            );
            debug_mir!(
                name = %name,
                symbol_id = ?symbol_id,
                val_type = ?val_type,
                initializer = ?initializer,
                "Added state variable global to WASM module"
            );
        }

        module.section(&global_section);

        // Export heap pointer global
        self.wasm_generator.export_section.export(
            "__heap_ptr",
            wasm_encoder::ExportKind::Global,
            0,
        );

        // Always export memory for WASM host interop
        self.wasm_generator
            .export_section
            .export("memory", wasm_encoder::ExportKind::Memory, 0);

        // Export all user-defined functions.
        //
        // Defensive invariants:
        // 1. Skip the `u32::MAX` tree-shake sentinel (Import Minimality Rule).
        // 2. Skip indices that are out of bounds w.r.t. the module's function
        //    count. A bad index here causes wasmparser to reject the module
        //    with "unknown function N: exported function index out of bounds".
        //    When that happens it's a registration bug upstream — log and
        //    skip rather than silently emitting invalid bytes.
        let total_functions = self.wasm_generator.function_count;
        for (name, &index) in &self.wasm_generator.function_map {
            if index == u32::MAX {
                continue;
            }
            if index >= total_functions {
                tracing::error!(
                    name = %name,
                    index = index,
                    total_functions = total_functions,
                    "Refusing to emit export with out-of-bounds function index \
                     (upstream registration bug — report via `report_error`)"
                );
                continue;
            }
            // Export rule:
            //   - Route handlers (double-underscore __route_handler_N) — exported by this path
            //     so the host can dispatch to them by stable index.
            //   - Single-underscore callbacks (_frame_callback, _on_pointer_down, _on_pointer_move,
            //     _on_pointer_up, and any future plugin-defined callbacks): starts with "_" but NOT
            //     with "__".  These are the plugin-facing ABI surface; the host calls them directly
            //     by name so they must all be exported.
            //   - Regular user functions (no leading underscore): always exported.
            //   - Internal compiler helpers (double underscore, not __route_handler_): NOT exported.
            let is_route_handler = name.starts_with("__route_handler_");
            let is_single_underscore_callback = name.starts_with('_') && !name.starts_with("__");
            let is_regular_function = !name.starts_with('_');
            if is_route_handler || is_single_underscore_callback || is_regular_function {
                self.wasm_generator.export_section.export(
                    name,
                    wasm_encoder::ExportKind::Func,
                    index,
                );
            }
        }

        // Export handler functions as handle_event_N for runtime callback dispatch
        for (handler_name, &handler_index) in &self.handler_indices {
            if let Some(&func_index) = self.wasm_generator.function_map.get(handler_name) {
                let export_name = format!("handle_event_{}", handler_index);
                tracing::debug!(
                    handler = %handler_name,
                    handler_index = handler_index,
                    wasm_func_index = func_index,
                    export_name = %export_name,
                    "Exporting handler function for runtime callback dispatch"
                );
                self.wasm_generator.export_section.export(
                    &export_name,
                    wasm_encoder::ExportKind::Func,
                    func_index,
                );
            } else {
                tracing::warn!(
                    handler = %handler_name,
                    "Handler function not found in function map — skipping export"
                );
            }
        }

        // Export the indirect function table so hosts can call_indirect on it.
        if emit_function_table {
            self.wasm_generator.export_section.export(
                "__indirect_function_table",
                wasm_encoder::ExportKind::Table,
                0,
            );
        }

        // 5. Export section
        let export_section = self.wasm_generator.export_section.clone();
        module.section(&export_section);

        // 5.5 Element section — populate `__indirect_function_table` with the
        // WASM function indices referenced as first-class values. Each entry
        // sits at its own slot (slot N holds function N), so the i32 value
        // emitted at the reference site is a valid call_indirect index.
        if emit_function_table {
            let mut element_section = wasm_encoder::ElementSection::new();
            for func_idx in &sorted_referenced_funcs {
                let funcs = [*func_idx];
                element_section.active(
                    Some(0),
                    &wasm_encoder::ConstExpr::i32_const(*func_idx as i32),
                    wasm_encoder::Elements::Functions(&funcs),
                );
            }
            module.section(&element_section);
        }

        // 6. Code section
        let code_section = self.wasm_generator.code_section.clone();
        module.section(&code_section);

        // 7. Data section (string literals)
        let data_section = self.wasm_generator.memory_utils.get_data_section();
        module.section(data_section);

        // 8. clean:build custom section — compiler provenance for diagnostics.
        // Read by host runtimes (e.g. clean-server) when reporting WASM parse
        // failures so bugs can be attributed to a specific compiler version.
        let build_info = serde_json::json!({
            "compiler_version": crate::VERSION,
            "build_profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        });
        if let Ok(json_bytes) = serde_json::to_vec(&build_info) {
            let custom = wasm_encoder::CustomSection {
                name: std::borrow::Cow::Borrowed("clean:build"),
                data: std::borrow::Cow::Borrowed(&json_bytes),
            };
            module.section(&custom);
        }

        // 9. clean:memory custom section — memory tier metadata for host runtimes.
        // Allows hosts (clean-server, JS loaders) to read the module's intended
        // memory configuration without CLI flags.  See MEMORY_POLICY.md section 3.
        let memory_info = serde_json::json!({
            "tier": self.memory_tier.name(),
            "initial_pages": self.memory_tier.initial_pages(),
            "max_pages": self.memory_tier.max_pages(),
        });
        if let Ok(json_bytes) = serde_json::to_vec(&memory_info) {
            let custom = wasm_encoder::CustomSection {
                name: std::borrow::Cow::Borrowed("clean:memory"),
                data: std::borrow::Cow::Borrowed(&json_bytes),
            };
            module.section(&custom);
        }

        Ok(module.finish())
    }

    // -------------------------------------------------------------------------
    // Bridge / import registration
    // -------------------------------------------------------------------------

    /// Walk the MIR call graph and collect every called function name.
    ///
    /// This is a general-purpose reachability scan used to drive the
    /// Import Minimality Rule (see foundation/platform-architecture/EXECUTION_LAYERS.md).
    /// Returns the set of names that appear as call targets anywhere in the
    /// MIR program, expanded with common import-name aliases so that
    /// language-level names (`http.get`) match WASM import field names
    /// (`http_get`).
    pub(super) fn collect_all_called_names_from_mir(
        &self,
        mir_program: &MirProgram,
    ) -> HashSet<String> {
        use crate::mir::mir_types::{MirBinaryOp, MirOperand, MirOperation, MirType};

        let mut names: HashSet<String> = HashSet::new();

        /// Return true if this MIR type is a string (pointer-to-char or pointer-to-byte).
        fn is_string_type(t: &MirType) -> bool {
            matches!(
                t,
                MirType::Ptr(inner)
                    if matches!(inner.as_ref(), MirType::I8 | MirType::U8)
            )
        }

        // Helper: insert a function name AND expand language aliases to bridge names so
        // that bridge imports whose names start with `_res_`, `_req_`, etc. are not
        // incorrectly tree-shaken when only the alias appears in the MIR call graph.
        let insert_name = |names: &mut HashSet<String>, name: &str| {
            names.insert(name.to_string());
            if let Some(bridge_name) = self.language_to_bridge_map.get(name) {
                names.insert(bridge_name.clone());
            }
        };

        for function in mir_program.functions.values() {
            for block in function.blocks.values() {
                for instruction in &block.instructions {
                    match &instruction.operation {
                        MirOperation::Call { function, .. } => match function {
                            MirOperand::NamedFunction { name, .. } => {
                                insert_name(&mut names, name);
                            }
                            MirOperand::Function(symbol_id) => {
                                if let Some(name) = mir_program.symbol_name_map.get(symbol_id) {
                                    insert_name(&mut names, name);
                                }
                            }
                            _ => {}
                        },
                        // Detect string equality / inequality comparisons.
                        // These do NOT emit an explicit MIR Call — instead the
                        // codegen injects a `string_compare` call at BinaryOp
                        // code-gen time. We must mark `string_compare` reachable
                        // here so the import is not tree-shaken.
                        MirOperation::AsyncFireCall { .. } => {
                            names.insert("_async_fire".to_string());
                        }
                        MirOperation::AsyncAwaitCall { .. } => {
                            names.insert("_async_await".to_string());
                        }
                        MirOperation::BinaryOp {
                            op: MirBinaryOp::Eq | MirBinaryOp::Ne,
                            left,
                            right,
                        } => {
                            let left_is_string = match left {
                                MirOperand::Value(vid) => function
                                    .locals
                                    .get(vid)
                                    .map(|l| is_string_type(&l.local_type))
                                    .unwrap_or(false),
                                _ => false,
                            };
                            let right_is_string = match right {
                                MirOperand::Value(vid) => function
                                    .locals
                                    .get(vid)
                                    .map(|l| is_string_type(&l.local_type))
                                    .unwrap_or(false),
                                _ => false,
                            };
                            if left_is_string || right_is_string {
                                names.insert("string_compare".to_string());
                                names.insert("string.compare".to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Implicit dependency: JSON stringify operations (json.dataToText,
        // json.prettyDataToText) generate native WASM that calls string.concat
        // internally. If any json.* function is reachable, ensure string.concat
        // is also in the reachable set so the import is not tree-shaken.
        if names
            .iter()
            .any(|n| n.starts_with("json.") || n.starts_with("json_"))
        {
            names.insert("string.concat".to_string());
        }

        // Expand language-level names to the WASM import field names used by
        // host bridges. A call to `http.get` in the program maps to the
        // import `http_get` on the WASM module. Plugin bridge functions use
        // underscore-prefixed names like `_http_set_cache`.
        let expansions: Vec<String> = names
            .iter()
            .flat_map(|n| {
                let mut out = Vec::new();
                if n.contains('.') {
                    // "http.set_cache" → "http_set_cache"
                    let underscored = n.replace('.', "_");
                    out.push(underscored.clone());
                    // "http.set_cache" → "_http_set_cache" (plugin bridge name)
                    out.push(format!("_{}", underscored));
                }
                if n.contains('_') && !n.starts_with('_') {
                    // "http_get" → "http.get"
                    out.push(n.replacen('_', ".", 1));
                }
                out
            })
            .collect();
        names.extend(expansions);

        // Language names whose dot→underscore expansion does NOT match the
        // actual host bridge import field name (different naming convention or
        // "get" prefix in the import name). Without this table those imports
        // are tree-shaken even when the language-level name is in the MIR call
        // graph, and the wrapper that registers the alias is never created.
        let explicit_reachable: &[(&str, &str)] = &[
            // camelCase crypto: "crypto.randomHex" → "crypto_randomHex" (wrong)
            //                   actual import:       "crypto_random_hex"
            ("crypto.randomHex", "crypto_random_hex"),
            ("crypto.randomBytes", "crypto_random_bytes"),
            // http response accessors: "http.responseCode" → "http_responseCode" (wrong)
            //                          actual import:        "http_get_response_code"
            ("http.responseCode", "http_get_response_code"),
            ("http.getResponseCode", "http_get_response_code"),
            ("http.responseBody", "http_get_response_body"),
            ("http.getResponseBody", "http_get_response_body"),
            // camelCase http: dot→underscore expansion produces wrong snake_case import name
            ("http.postWithHeaders", "http_post_with_headers"),
            ("http.getWithHeaders", "http_get_with_headers"),
            ("http.postJson", "http_post_json"),
            ("http.putJson", "http_put_json"),
            ("http.patchJson", "http_patch_json"),
            ("http.postForm", "http_post_form"),
            ("http.encodeUrl", "http_encode_url"),
            ("http.decodeUrl", "http_decode_url"),
            ("http.buildQuery", "http_build_query"),
        ];
        for (lang, import_field) in explicit_reachable {
            if names.contains(*lang) {
                names.insert(import_field.to_string());
            }
        }

        tracing::debug!(
            total_called_names = names.len(),
            "Collected reachable call names from MIR"
        );

        names
    }

    /// Scan the MIR program and record which bridge functions are actually called.
    ///
    /// Used for selective imports — only functions referenced in the code will be
    /// registered as WASM imports.
    pub(super) fn collect_used_function_names_from_mir(&mut self, mir_program: &MirProgram) {
        use crate::mir::mir_types::{MirOperand, MirOperation};

        let bridge_function_names: HashSet<String> = self
            .bridge_functions
            .iter()
            .map(|f| f.name.clone())
            .collect();

        let resolve_to_bridge = |name: &str| -> Option<String> {
            if bridge_function_names.contains(name) {
                return Some(name.to_string());
            }
            if let Some(bridge_name) = self.language_to_bridge_map.get(name) {
                if bridge_function_names.contains(bridge_name.as_str()) {
                    return Some(bridge_name.clone());
                }
            }
            None
        };

        for function in mir_program.functions.values() {
            for block in function.blocks.values() {
                for instruction in &block.instructions {
                    if let MirOperation::Call { function, .. } = &instruction.operation {
                        match function {
                            MirOperand::NamedFunction { name, .. } => {
                                if let Some(bridge_name) = resolve_to_bridge(name) {
                                    self.used_bridge_function_names.insert(bridge_name);
                                }
                            }
                            MirOperand::Function(symbol_id) => {
                                if let Some(name) = mir_program.symbol_name_map.get(symbol_id) {
                                    if let Some(bridge_name) = resolve_to_bridge(name) {
                                        self.used_bridge_function_names.insert(bridge_name);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        tracing::debug!(
            total_bridge_functions = self.bridge_functions.len(),
            used_bridge_functions = self.used_bridge_function_names.len(),
            used_names = ?self.used_bridge_function_names,
            "Collected used bridge function names from MIR"
        );
    }

    /// Register plugin bridge functions as WASM imports.
    ///
    /// For functions with `expand_strings=true`, registers the raw import and
    /// defers wrapper registration to `register_pending_bridge_wrappers()`.
    ///
    /// CRITICAL: Only imports functions that are actually used in the code.
    pub(super) fn register_plugin_bridge_imports(&mut self) -> Result<(), CompilerError> {
        use crate::builtins::registry::BuiltinType;
        use crate::types::WasmType;

        let used_functions: Vec<_> = self
            .bridge_functions
            .iter()
            .filter(|f| self.used_bridge_function_names.contains(&f.name))
            .cloned()
            .collect();

        if used_functions.is_empty() {
            tracing::debug!("No bridge functions are used in the code, skipping imports");
            return Ok(());
        }

        tracing::debug!(
            total = self.bridge_functions.len(),
            used = used_functions.len(),
            "Registering only used bridge function imports"
        );

        for func in &used_functions {
            let param_types = func.get_param_types();
            let return_type = func.get_return_type();
            let module = &func.module;

            let needs_wrapper =
                func.expand_strings && param_types.iter().any(|t| matches!(t, BuiltinType::String));

            if needs_wrapper {
                // Build expanded signature for raw import (strings → ptr, len pairs)
                let mut raw_wasm_params = Vec::new();
                for param_type in &param_types {
                    if matches!(param_type, BuiltinType::String) {
                        raw_wasm_params.push(WasmType::I32); // ptr
                        raw_wasm_params.push(WasmType::I32); // len
                    } else {
                        raw_wasm_params.push(Self::builtin_type_to_wasm_type(param_type));
                    }
                }

                let wasm_return = match &return_type {
                    BuiltinType::Void => None,
                    _ => Some(Self::builtin_type_to_wasm_type(&return_type)),
                };

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

                // `register_import_function` returns `u32::MAX` as a sentinel
                // when the import is tree-shaken out (Import Minimality Rule,
                // foundation/platform-architecture/EXECUTION_LAYERS.md). If that happens,
                // there is no real host function to wrap, so skip the wrapper
                // entirely — otherwise the wrapper's body would emit
                // `Call(u32::MAX)` and the wrapper would end up in the export
                // section with a bogus index.
                if raw_func_index == u32::MAX {
                    tracing::debug!(
                        name = %func.name,
                        "Skipping wrapper for tree-shaken bridge import"
                    );
                    continue;
                }

                let wrapper_params: Vec<WasmType> = param_types
                    .iter()
                    .map(Self::builtin_type_to_wasm_type)
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
                let wasm_params: Vec<WasmType> = param_types
                    .iter()
                    .map(Self::builtin_type_to_wasm_type)
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

                let import_index = self.wasm_generator.register_import_function(
                    module,
                    &func.name,
                    &wasm_params,
                    wasm_return,
                )?;

                // Tree-shaken imports return `u32::MAX` (see
                // EXECUTION_LAYERS.md Import Minimality Rule). Never attach
                // that sentinel to a language-level alias — aliases end up in
                // `function_map` and anything reading from there (including
                // the export-section emitter) must only see valid indices.
                if import_index == u32::MAX {
                    continue;
                }

                // Register language-name aliases
                for (lang_name, bridge_name) in &self.language_to_bridge_map {
                    if bridge_name == &func.name {
                        tracing::debug!(
                            lang_name = %lang_name,
                            bridge_name = %bridge_name,
                            wasm_index = import_index,
                            "Registering language-name alias for direct bridge import"
                        );
                        self.wasm_generator
                            .function_map
                            .insert(lang_name.clone(), import_index);
                    }
                }
            }
        }

        Ok(())
    }

    // Registers the dot-notation call-site alias (e.g. "db.query") in function_map
    // pointing to the wrapper index. The canonical import ("_db_query") was already added by
    // register_plugin_bridge_imports(). Only the canonical name appears as a WASM import.
    /// Register pending bridge wrapper functions.
    ///
    /// CRITICAL: Must be called AFTER all imports are registered to avoid function
    /// index collisions between imports and internal wrapper functions.
    pub(super) fn register_pending_bridge_wrappers(&mut self) -> Result<(), CompilerError> {
        use crate::builtins::registry::BuiltinType;
        use wasm_encoder::{Instruction, MemArg};

        let wrappers = std::mem::take(&mut self.pending_bridge_wrappers);

        for wrapper in wrappers {
            let mut wrapper_instructions = Vec::new();
            let mut local_idx = 0u32;

            for param_type in wrapper.param_types.iter() {
                if matches!(param_type, BuiltinType::String) {
                    // Expand Clean string (ptr → [len][content]) to (ptr+4, len)
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    wrapper_instructions.push(Instruction::I32Const(4));
                    wrapper_instructions.push(Instruction::I32Add);

                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    wrapper_instructions.push(Instruction::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                    local_idx += 1;
                } else {
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    local_idx += 1;
                }
            }

            wrapper_instructions.push(Instruction::Call(wrapper.raw_func_index));

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

            // Register language-name aliases for the wrapper
            if let Some(&wrapper_index) = self.wasm_generator.function_map.get(&wrapper.name) {
                for (lang_name, bridge_name) in &self.language_to_bridge_map {
                    if bridge_name == &wrapper.name {
                        tracing::debug!(
                            lang_name = %lang_name,
                            bridge_name = %bridge_name,
                            wasm_index = wrapper_index,
                            "Registering language-name alias for expand_strings bridge wrapper"
                        );
                        self.wasm_generator
                            .function_map
                            .insert(lang_name.clone(), wrapper_index);
                    }
                }
            }
        }

        Ok(())
    }

    /// Register HTTP server wrapper functions for string expansion.
    ///
    /// HTTP server functions like `_http_route` and `_req_param` need wrapper
    /// functions that expand Clean Language strings to raw (ptr+4, len) format.
    ///
    /// CRITICAL: Must be called AFTER all imports AND `register_pending_bridge_wrappers()`.
    pub(super) fn register_http_server_wrappers(&mut self) -> Result<(), CompilerError> {
        use crate::builtins::registry::BuiltinType;
        use crate::types::WasmType;
        use wasm_encoder::{Instruction, MemArg};

        let http_server_functions = [
            (
                "_http_route",
                vec![
                    BuiltinType::String,
                    BuiltinType::String,
                    BuiltinType::Integer,
                ],
                Some(WasmType::I32),
            ),
            (
                "_http_route_protected",
                vec![
                    BuiltinType::String,
                    BuiltinType::String,
                    BuiltinType::Integer,
                    BuiltinType::String,
                ],
                Some(WasmType::I32),
            ),
            ("_req_param", vec![BuiltinType::String], Some(WasmType::I32)),
            ("_req_query", vec![BuiltinType::String], Some(WasmType::I32)),
            (
                "_req_header",
                vec![BuiltinType::String],
                Some(WasmType::I32),
            ),
            (
                "_req_cookie",
                vec![BuiltinType::String],
                Some(WasmType::I32),
            ),
            (
                "_session_store",
                vec![
                    BuiltinType::Integer,
                    BuiltinType::String,
                    BuiltinType::String,
                ],
                Some(WasmType::I32),
            ),
            (
                "_http_set_cookie",
                vec![BuiltinType::String],
                Some(WasmType::I32),
            ),
            (
                "_auth_require_role",
                vec![BuiltinType::String],
                Some(WasmType::I32),
            ),
            ("_auth_can", vec![BuiltinType::String], Some(WasmType::I32)),
            (
                "_auth_has_any_role",
                vec![BuiltinType::String],
                Some(WasmType::I32),
            ),
            (
                "_res_redirect",
                vec![BuiltinType::String, BuiltinType::Integer],
                Some(WasmType::I32),
            ),
            (
                "_res_set_header",
                vec![BuiltinType::String, BuiltinType::String],
                Some(WasmType::I32),
            ),
            ("_res_status", vec![BuiltinType::Integer], None),
            (
                "_http_respond",
                vec![
                    BuiltinType::Integer,
                    BuiltinType::String,
                    BuiltinType::String,
                ],
                Some(WasmType::I32),
            ),
            (
                "_http_redirect",
                vec![BuiltinType::Integer, BuiltinType::String],
                Some(WasmType::I32),
            ),
        ];

        for (func_name, param_types, wasm_return) in http_server_functions {
            let raw_func_index = match self.wasm_generator.http_import_indices.get(func_name) {
                Some(&idx) => idx,
                None => {
                    tracing::debug!(
                        name = %func_name,
                        "HTTP server function not registered as import, skipping wrapper"
                    );
                    continue;
                }
            };

            let wrapper_params: Vec<WasmType> = param_types
                .iter()
                .map(Self::builtin_type_to_wasm_type)
                .collect();

            let mut wrapper_instructions = Vec::new();
            let mut local_idx = 0u32;

            for param_type in param_types.iter() {
                if matches!(param_type, BuiltinType::String) {
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    wrapper_instructions.push(Instruction::I32Const(4));
                    wrapper_instructions.push(Instruction::I32Add);

                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    wrapper_instructions.push(Instruction::I32Load(MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));

                    local_idx += 1;
                } else {
                    wrapper_instructions.push(Instruction::LocalGet(local_idx));
                    local_idx += 1;
                }
            }

            wrapper_instructions.push(Instruction::Call(raw_func_index));

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

            let wrapper_index = self.wasm_generator.register_function(
                &wrapper_name,
                &wrapper_params,
                wasm_return,
                &wrapper_instructions,
            )?;

            // Map the original function name to the wrapper index
            self.wasm_generator
                .function_map
                .insert(func_name.to_string(), wrapper_index);

            // Update http_import_indices to point to wrapper
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

    // -------------------------------------------------------------------------
    // Type converters (static)
    // -------------------------------------------------------------------------

    /// Convert a `BuiltinType` to the project's `WasmType` enum.
    pub(super) fn builtin_type_to_wasm_type(
        bt: &crate::builtins::registry::BuiltinType,
    ) -> crate::types::WasmType {
        use crate::builtins::registry::BuiltinType;
        use crate::types::WasmType;

        match bt {
            BuiltinType::Integer => WasmType::I32,
            BuiltinType::Number => WasmType::F64,
            BuiltinType::String => WasmType::I32,
            BuiltinType::Boolean => WasmType::I32,
            BuiltinType::Void => WasmType::I32,
            BuiltinType::List(_) => WasmType::I32,
            BuiltinType::Matrix(_) => WasmType::I32,
            BuiltinType::Pairs(_, _) => WasmType::I32,
            BuiltinType::Namespace => WasmType::I32,
            BuiltinType::Any => WasmType::I32,
            BuiltinType::Handler => WasmType::I32,
        }
    }

    /// Convert a `MirType` to `WasmType` for external function import signatures.
    pub(super) fn mir_type_to_wasm_type_for_import(mir_type: &MirType) -> crate::types::WasmType {
        use crate::types::WasmType;

        match mir_type {
            MirType::I8
            | MirType::I16
            | MirType::I32
            | MirType::U8
            | MirType::U16
            | MirType::U32
            | MirType::Bool => WasmType::I32,

            MirType::I64 | MirType::U64 => WasmType::I64,

            MirType::F32 => WasmType::F32,
            MirType::F64 => WasmType::F64,

            MirType::Ptr(_) => WasmType::I32,
            MirType::StringTuple => WasmType::I32,
            MirType::Any => WasmType::I32,
            MirType::Array(_, _) => WasmType::I32,
            MirType::Function { .. } => WasmType::I32,
            MirType::Void => WasmType::I32,
            MirType::Struct(_) => WasmType::I32,
        }
    }

    /// Register external functions declared via `external:` blocks as WASM imports.
    ///
    /// CRITICAL: Must be called AFTER plugin bridge imports and BEFORE any internal
    /// functions are registered, because WASM requires all imports to precede definitions.
    pub(super) fn register_external_function_imports(
        &mut self,
        externals: &[crate::mir::mir_types::MirExternalFunction],
    ) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        for external in externals {
            // Skip language-name aliases (dot-notation names like "db.query",
            // "req.param"). These exist in ast.externals so the HIR validator
            // recognises their namespace prefix, but they are NOT separate WASM
            // imports. The canonical underscore bridge function ("_db_query") is
            // registered by register_plugin_bridge_imports(), and the dot-alias
            // is added to function_map by register_pending_bridge_wrappers().
            // Emitting a second import for the alias causes dual-emission — the
            // host would have to satisfy an orphaned "db.query" import that is
            // never actually called from WASM bytecode.
            if self.language_to_bridge_map.contains_key(&external.name) {
                tracing::debug!(
                    name = %external.name,
                    "Skipping language-alias external (handled by bridge registration)"
                );
                continue;
            }

            let wasm_params: Vec<WasmType> = external
                .parameters
                .iter()
                .map(|p| Self::mir_type_to_wasm_type_for_import(&p.param_type))
                .collect();

            let wasm_return = match &external.return_type {
                MirType::Void => None,
                rt => Some(Self::mir_type_to_wasm_type_for_import(rt)),
            };

            tracing::debug!(
                name = %external.name,
                module = %external.module,
                params = ?wasm_params,
                returns = ?wasm_return,
                "Registering external function as WASM import"
            );

            let func_index = self.wasm_generator.register_import_function(
                &external.module,
                &external.name,
                &wasm_params,
                wasm_return,
            )?;

            self.external_function_indices
                .insert(external.name.clone(), func_index);

            self.wasm_generator
                .function_map
                .insert(external.name.clone(), func_index);

            tracing::debug!(
                name = %external.name,
                func_index = func_index,
                "External function registered with WASM index"
            );
        }

        Ok(())
    }
}
