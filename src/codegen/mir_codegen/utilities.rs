//! Utility helpers for MIR code generation.
//!
//! Contains:
//! * Type helpers: `get_value_type`, `get_operand_mir_type`, `get_stdlib_return_type`
//! * Function signature conversion: `convert_function_signature`, `mir_type_to_wasm_type`
//! * Local/block computation: `compute_local_types`, `compute_block_order`
//! * Function resolution: `get_function_name_by_symbol`
//! * Module setup: `register_builtin_function_signatures`, `setup_memory_section`,
//!   `setup_string_pool`, `add_function_to_module`, `val_type_to_wasm_type`
//! * Export helpers: `generate_start_function_export`
//! * JSON helpers: `get_or_register_json_get_field`, `get_or_register_json_get_index`
//! * Module finalisation: `finalize_module`
//! * Bridge/import registration: `collect_used_function_names_from_mir`,
//!   `register_plugin_bridge_imports`, `register_pending_bridge_wrappers`,
//!   `register_external_function_imports`
//! * Type converters: `builtin_type_to_wasm_type`, `mir_type_to_wasm_type_for_import`

use super::*;
use wasm_encoder::{Function as WasmFunction, Instruction, ValType};

/// Return true if `host_class` is set and `bridge_hosts` excludes both `"all"`
/// and the active host class. Used by `register_plugin_bridge_imports` to
/// substitute a local no-op stub for bridges that won't be provided by the
/// active host (CLIENT_BUILD_ENTRY_LEAK).
///
/// Bridges with no `hosts` field are treated as cross-host (Phase C compat):
/// see `foundation/spec/plugins/contracts/bridge-host-classes.md` §6.2.
fn bridge_is_host_mismatched(host_class: Option<&str>, bridge_hosts: Option<&[String]>) -> bool {
    let host_class = match host_class {
        Some(h) => h,
        None => return false,
    };
    let hosts = match bridge_hosts {
        Some(h) => h,
        None => return false,
    };
    !hosts.iter().any(|h| h == "all" || h == host_class)
}

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
                MirConstant::Integer64(_) => MirType::I64,
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

        // Void functions.
        // `list.push` is NOT void — it returns the (possibly reallocated)
        // list pointer for chaining. ArrayLiteral codegen relies on this
        // return value: it emits a Copy after each push to fold the new
        // pointer back into the variable's local. Listing it here caused
        // the codegen to drop the call result, leaving the post-call
        // Copy reading an uninitialized local (= 0). With multiple list
        // literals in the same program this looked "fine" only because
        // every list happened to share address 0 thanks to a separate
        // bug in the list bump allocator; once that allocator was fixed,
        // the Copy bug surfaced as cross-list corruption.
        for name in &["print", "printl", "list.set", "list.clear"] {
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

        let wasm_return = if return_wasm_types.is_empty() {
            None
        } else {
            Some(return_wasm_types[0])
        };

        let type_index = self
            .wasm_generator
            .add_function_type(&param_wasm_types, wasm_return)?;

        self.wasm_generator.function_section.function(type_index);
        self.wasm_generator.code_section.function(&wasm_function);
        self.wasm_generator.function_names.push(name.clone());

        // Defense in depth for CODEGEN_STACK_REMAINING (fp ab7f9b3f): the
        // call-site void detection in `wasm_function_is_void` queries this
        // registry as a last resort. Previously only `register_function` /
        // `register_function_multi` / `register_import_function` populated
        // it — MIR-emitted user functions (the ones that go through this
        // `add_function_to_module` path) were missing, so any user-class
        // void method called as an expression-statement could not be
        // proven void by name and a spurious `drop` was emitted.
        self.wasm_generator
            .record_wasm_return_type(&name, wasm_return);

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

        // Globals 1-3: __json_get parse-result cache. Avoids re-parsing the
        // same source string when the SSR shape calls json.get(j, path) in a
        // loop with a stable `j`. Without this each iteration re-runs the
        // recursive descent parser, allocating a fresh tree on the bump
        // heap — drives O(n × tree_size) cumulative leak that hit
        // CMP-SSR-MALLOC-OOM-CONDITIONAL-HELPER (e4c682d19d00) for n≈30
        // with large items. See json.get shim in src/stdlib/json_class.rs.
        //
        // Global 1: cached source string ptr (the `str_ptr` extracted from
        //           the boxed Any wrapper passed to json.get). 0 = empty.
        // Global 2: cached parsed-tree boxed Any ptr.
        // Global 3: heap floor — the __heap_ptr value immediately after the
        //           parse landed the tree. On subsequent json.get calls we
        //           require __heap_ptr >= Global 3, otherwise some
        //           mem_scope_pop has reclaimed the tree and we must re-parse.
        //
        // Globals 4-5: transient arena (see native_stdlib::transient_arena).
        // Global 4 = TRANSIENT_BASE_GLOBAL (pool base ptr; 0 = uninit).
        // Global 5 = TRANSIENT_PTR_GLOBAL  (bump pointer within the pool).
        // Both start at 0; the pool is lazily __malloc-allocated on the
        // first __transient_scope_enter call. The matching scope_exit
        // restores TRANSIENT_PTR — the mechanism that replaces the
        // rolled-back string_builder_reclaim (see ARCHITECTURE in
        // transient_arena.rs).
        for _ in 0..(crate::codegen::native_stdlib::RESERVED_GLOBAL_COUNT - 1) {
            global_section.global(
                wasm_encoder::GlobalType {
                    val_type: wasm_encoder::ValType::I32,
                    mutable: true,
                },
                &wasm_encoder::ConstExpr::i32_const(0),
            );
        }

        // State variable globals (indices start at 4, after heap pointer + 3 json cache slots)
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
            //   - Page handlers (double-underscore __page_handler_*) — synthetic handlers
            //     generated by the multi-file compiler for page companion .cln files.
            //     The host dispatches to them by name via _http_route registration.
            //   - Single-underscore callbacks (_frame_callback, _on_pointer_down, _on_pointer_move,
            //     _on_pointer_up, and any future plugin-defined callbacks): starts with "_" but NOT
            //     with "__".  These are the plugin-facing ABI surface; the host calls them directly
            //     by name so they must all be exported.
            //   - Regular user functions (no leading underscore): always exported.
            //   - Plugin-generated functions in user_defined_function_names: always exported,
            //     including those with double-underscore prefix (e.g. __redirect_0 from expand_block).
            //   - Internal compiler helpers (double underscore, not __route_handler_ or
            //     __page_handler_, not in user_defined_function_names): NOT exported.
            let is_route_handler = name.starts_with("__route_handler_");
            let is_page_handler = name.starts_with("__page_handler_");
            let is_single_underscore_callback = name.starts_with('_') && !name.starts_with("__");
            let is_regular_function = !name.starts_with('_');
            let is_user_defined = self.user_defined_function_names.contains(name.as_str());
            if is_route_handler
                || is_page_handler
                || is_single_underscore_callback
                || is_regular_function
                || is_user_defined
            {
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

        // 10. clean.abi_version custom section — Plugin Contracts v2 Phase B.
        // Emitted only for plugin builds (when abi_version was supplied via
        // `set_abi_version`). Payload is the raw UTF-8 version string
        // ("1.0.0"), with no length prefix or JSON wrapping — the loader
        // (Phase B cycle 2) reads it directly as &str. Section name uses
        // dotted form `clean.abi_version` per
        // foundation/spec/plugins/contracts/runtime-abi.md §4.
        if let Some(ref ver) = self.abi_version {
            let custom = wasm_encoder::CustomSection {
                name: std::borrow::Cow::Borrowed("clean.abi_version"),
                data: std::borrow::Cow::Borrowed(ver.as_bytes()),
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
    /// Plugin Contracts v2 — check every reachable bridge call against the
    /// active build's host class. Returns one diagnostic per bridge function
    /// whose declared `hosts` does not include the host class.
    ///
    /// Returns an empty Vec when:
    /// - `self.host_class` is None (enforcement disabled — e.g. plugin builds),
    /// - no bridge function in the reachable set has a `hosts` field, or
    /// - every reachable bridge accepts `"all"` or the current host class.
    ///
    /// Bridges with no `hosts` field are skipped (Phase C compatibility per
    /// `foundation/spec/plugins/contracts/bridge-host-classes.md` §6.2 — third
    /// party plugins haven't migrated yet). When Phase D lands, missing
    /// `hosts` will become a deprecation warning at plugin load.
    pub(super) fn check_bridge_host_classes(
        &self,
        reachable_calls: &HashSet<String>,
    ) -> Vec<crate::error::CompilerError> {
        use crate::error::{CompilerError, ErrorContext, ErrorType};

        let host_class = match self.host_class.as_deref() {
            Some(h) => h,
            None => return Vec::new(),
        };

        let mut diagnostics = Vec::new();
        for bridge in &self.bridge_functions {
            let hosts = match bridge.hosts.as_deref() {
                Some(h) => h,
                None => continue,
            };
            // A bridge call must reach the BFS reachable set to be relevant.
            // Also accept dot-form aliases (e.g. `db.query` for `_db_query`).
            let reachable = reachable_calls.contains(&bridge.name)
                || self
                    .language_to_bridge_map
                    .iter()
                    .any(|(lang, bridge_name)| {
                        bridge_name == &bridge.name && reachable_calls.contains(lang)
                    });
            if !reachable {
                continue;
            }
            let allowed = hosts.iter().any(|h| h == "all" || h == host_class);
            if allowed {
                continue;
            }
            let message = format!(
                "BRIDGE-HOST-MISMATCH: bridge function `{}` is declared for hosts {:?}, \
                 but this build targets host class `{}`. \
                 Calling this function on the wrong host will fail at runtime. \
                 See foundation/spec/plugins/contracts/bridge-host-classes.md §6.",
                bridge.name, hosts, host_class
            );
            diagnostics.push(CompilerError::Validation {
                context: Box::new(
                    ErrorContext::new(message, None, ErrorType::Validation, None)
                        .with_error_code("BRIDGE-HOST-MISMATCH"),
                ),
            });
        }
        diagnostics
    }

    /// This is a general-purpose reachability scan used to drive the
    /// Import Minimality Rule (see foundation/platform-architecture/EXECUTION_LAYERS.md).
    /// Returns the set of names that appear as call targets anywhere in the
    /// MIR program, expanded with common import-name aliases so that
    /// language-level names (`http.get`) match WASM import field names
    /// (`http_get`).
    /// Infer the bridge host class from the reachable bridge functions.
    ///
    /// Fixes 270f8fc643db: when the user did not pass an explicit `--target`,
    /// the compiler used to default to `server`, which fails for client entry
    /// files that only call browser-declared bridges (e.g. `ui.observeVisible`).
    ///
    /// Rule: if every reachable bridge with a `hosts` field accepts `browser`
    /// (either via `"all"` or by naming `"browser"`), and at least one
    /// reachable bridge is browser-restricted (does NOT accept `server`),
    /// return `Some("browser")`. Otherwise return `None` so the caller can
    /// fall back to the historical `server` default. Bridges without a
    /// `hosts` field are treated as unconstrained (Phase C compatibility per
    /// `foundation/spec/plugins/contracts/bridge-host-classes.md` §6.2).
    pub(crate) fn infer_host_class_from_mir(&self, mir_program: &MirProgram) -> Option<String> {
        let reachable = self.collect_all_called_names_from_mir(mir_program);

        let mut any_browser_restricted = false;

        for bridge in &self.bridge_functions {
            let hosts = match bridge.hosts.as_deref() {
                Some(h) => h,
                None => continue,
            };
            // A bridge counts only if the MIR actually calls it (direct or via
            // a language alias like `ui.observeVisible` → `_ui_observe_visible`).
            let bridge_reachable = reachable.contains(&bridge.name)
                || self
                    .language_to_bridge_map
                    .iter()
                    .any(|(lang, bridge_name)| {
                        bridge_name == &bridge.name && reachable.contains(lang)
                    });
            if !bridge_reachable {
                continue;
            }

            let accepts_browser = hosts.iter().any(|h| h == "all" || h == "browser");
            let accepts_server = hosts.iter().any(|h| h == "all" || h == "server");

            if !accepts_browser {
                // Some reachable bridge is server-only — inference cannot pick
                // `browser` safely.
                return None;
            }
            if !accepts_server {
                // At least one bridge is browser-restricted, so `server` is
                // wrong; inference has evidence to pick `browser`.
                any_browser_restricted = true;
            }
            // else: bridge accepts both browser+server or is `"all"` — no signal.
        }

        if any_browser_restricted {
            Some("browser".to_string())
        } else {
            None
        }
    }

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

        // Build a name→SymbolId reverse map so NamedFunction calls can be
        // resolved to their MIR body even when symbol_id is the shared
        // SymbolId(0) placeholder used for all stdlib namespace functions.
        let name_to_symbol: std::collections::HashMap<String, crate::resolver::SymbolId> =
            mir_program
                .functions
                .iter()
                .map(|(sym, f)| (f.name.clone(), *sym))
                .collect();

        // Helper: insert a function name AND expand language aliases to bridge names so
        // that bridge imports whose names start with `_res_`, `_req_`, etc. are not
        // incorrectly tree-shaken when only the alias appears in the MIR call graph.
        //
        // Also expand plugin-emitted helper aliases: when a language name resolves
        // via `language_to_helper_map` (e.g. `auth.jwt.sign` → `jwt_sign`) the
        // helper function was appended to `program.functions` during framework
        // block expansion. Insert its name so bridge imports called from the
        // helper's body are not tree-shaken. The helper's SymbolId is added to
        // the BFS worklist so its body is walked (bug c96f15c65a23 —
        // FRAME-AUTH-JWT-HELPERS-UNREACHABLE follow-up).
        let insert_name = |names: &mut HashSet<String>,
                           worklist: &mut Vec<crate::resolver::SymbolId>,
                           visited: &mut HashSet<crate::resolver::SymbolId>,
                           name: &str| {
            names.insert(name.to_string());
            if let Some(bridge_name) = self.language_to_bridge_map.get(name) {
                names.insert(bridge_name.clone());
            }
            if let Some(helper_name) = self.language_to_helper_map.get(name) {
                names.insert(helper_name.clone());
                if let Some(&helper_sym) = name_to_symbol.get(helper_name.as_str()) {
                    if visited.insert(helper_sym) {
                        worklist.push(helper_sym);
                    }
                }
            }
        };

        // Layer-3 (server-only) bridge names sourced from plugin manifests.
        // Replaces the previous hardcoded `_http_*`, `_req_*`, … prefix list
        // (BUILTIN-NAMESPACE-OVERREACH). A bridge is server-only when its
        // `hosts` field is set and excludes both `"all"` and `"browser"`.
        // Calls to these names appearing inside dead code are intentionally
        // excluded from the reachable-names set so server imports are not
        // pulled into builds whose start: never invokes them (Import
        // Minimality Rule, GEN003).
        let server_only_bridge_names: HashSet<String> = self
            .bridge_functions
            .iter()
            .filter(|bf| {
                bf.hosts
                    .as_deref()
                    .is_some_and(|hosts| !hosts.iter().any(|h| h == "browser" || h == "all"))
            })
            .map(|bf| bf.name.clone())
            .collect();

        // Seed the BFS worklist with the program entry point and every
        // exported function (route handlers, page handlers, _run_tests wrapper).
        // Only functions reachable from these roots contribute bridge imports.
        let mut visited: HashSet<crate::resolver::SymbolId> = HashSet::new();
        let mut worklist: Vec<crate::resolver::SymbolId> = Vec::new();

        {
            let mut seed = |sym: crate::resolver::SymbolId| {
                if visited.insert(sym) {
                    worklist.push(sym);
                }
            };

            if let Some(ep) = mir_program.entry_point {
                seed(ep);
            }
            for (sym, f) in &mir_program.functions {
                let is_exported_by_attr = f.attributes.entry_point || f.attributes.exported;
                // Route handlers (__route_handler_*) and page handlers (__page_handler_*)
                // are called by the HTTP server runtime when requests arrive — they are
                // never invoked from start:, so BFS from start: alone never reaches
                // bridge functions used only inside a route handler → helper chain.
                // Single-underscore callbacks (_on_event, _frame_callback, etc.) face
                // the same problem: called by plugin hosts, not from start:.
                // These naming patterns mirror the WASM export-section rules in
                // utilities.rs — the same functions that are exported so hosts can
                // dispatch to them must also be seeded so their bridge imports are
                // registered. (GEN003 follow-up)
                //
                // In client mode (browser build), route/page handlers are server-only
                // and must NOT be seeded — seeding them causes their _db_query /
                // _http_respond imports to appear in frontend.wasm (CLIENT_MODULE_LEAK).
                let is_server_handler =
                    f.name.starts_with("__route_handler_") || f.name.starts_with("__page_handler_");
                let is_single_underscore_callback =
                    f.name.starts_with('_') && !f.name.starts_with("__");
                let is_external_call_target =
                    (!self.client_mode && is_server_handler) || is_single_underscore_callback;
                // User-defined functions are called directly by the HTTP server runtime
                // when they are registered as endpoint handlers (e.g. via frame.server
                // endpoints:). The plugin uses integer table indices to register them,
                // so the BFS cannot follow the call chain from start: through the
                // registration call. Instead we seed user-defined functions directly
                // so their bridge imports (Layer2/3) are found reachable.
                // Plugin-generated preamble functions have location.file == "<plugin-output>";
                // we deliberately exclude them here to preserve the Import Minimality Rule
                // — preamble helpers like resDownload must not drag in _res_download unless
                // user code actually calls them.
                //
                // In client mode, user-defined functions are NOT seeded as roots.
                // Only _start (the entry point) and explicitly exported functions are
                // roots; everything else is found by BFS reachability from those roots.
                // This prevents server-only functions (e.g. find_user calling _db_query)
                // from leaking into frontend.wasm (CLIENT_MODULE_LEAK).
                let is_user_defined = !self.client_mode
                    && !f.location.file.is_empty()
                    && f.location.file != crate::ast::PLUGIN_OUTPUT_MARKER
                    && f.location.file != crate::ast::PLUGIN_OUTPUT_V2_ROOT_MARKER;
                // PLUGIN_OUTPUT_V2_ROOT_MARKER is used by `expander.rs::synthesize_event_handler_shims`
                // for synthesized event-handler shims that dispatch to component
                // class methods. The browser loader calls these shims via
                // `instance.exports[handlerName]()` JS — no static call site
                // exists in MIR — so they must be BFS roots, otherwise their
                // bridge imports (`_ui_*` helpers etc.) get tree-shaken.
                //
                // Plugin v2 module_helpers helpers deliberately do NOT carry
                // this marker even when `module_helpers_are_roots = true`.
                // The BFS already seeds `__route_handler_*` / `__page_handler_*`
                // shims as roots and finds module_helpers naturally through
                // them — auto-rooting every helper used to leak its bridge
                // imports unconditionally (GEN003 fingerprint `a2375b1158b2`).
                let is_v2_module_helper_root =
                    f.location.file == crate::ast::PLUGIN_OUTPUT_V2_ROOT_MARKER;
                if is_exported_by_attr
                    || is_external_call_target
                    || is_user_defined
                    || is_v2_module_helper_root
                {
                    seed(*sym);
                }
                // Page handlers call json.encode(data) to serialize load() return values.
                // json.encode is a pure WASM function that requires string.concat to be
                // registered (register_stringify_operations gates on string.concat). Force
                // string.concat into the reachable set whenever a page handler is present
                // so the pure WASM path is always available (GEN004).
                if !self.client_mode && f.name.starts_with("__page_handler_") {
                    names.insert("string.concat".to_string());
                }
            }
        } // seed closure dropped here, releasing borrows on visited/worklist

        // Plugin source files have no `start:` entry point and their exported
        // functions are not yet marked `exported: true` in MIR attributes (only
        // `start` gets that flag — see mir_builder/functions.rs). When the seed
        // is empty we fall back to scanning ALL functions, which preserves the
        // pre-GEN003 behaviour and ensures internal helpers like `string.concat`
        // are not incorrectly tree-shaken when compiling plugin code.
        if worklist.is_empty() {
            for sym in mir_program.functions.keys() {
                if visited.insert(*sym) {
                    worklist.push(*sym);
                }
            }
        }

        while let Some(sym) = worklist.pop() {
            let current_func = match mir_program.functions.get(&sym) {
                Some(f) => f,
                None => {
                    continue;
                }
            };
            for block in current_func.blocks.values() {
                for instruction in &block.instructions {
                    match &instruction.operation {
                        MirOperation::Call { function, .. } => match function {
                            MirOperand::NamedFunction { name, symbol_id } => {
                                insert_name(&mut names, &mut worklist, &mut visited, name);
                                // symbol_id is SymbolId(0) for all stdlib/namespace
                                // functions; resolve by name for user-defined callees.
                                let callee = if symbol_id.0 != 0 {
                                    Some(*symbol_id)
                                } else {
                                    name_to_symbol.get(name.as_str()).copied()
                                };
                                if let Some(s) = callee {
                                    if visited.insert(s) {
                                        worklist.push(s);
                                    }
                                }
                            }
                            MirOperand::Function(callee_sym) => {
                                if let Some(name) = mir_program.symbol_name_map.get(callee_sym) {
                                    insert_name(&mut names, &mut worklist, &mut visited, name);
                                }
                                if visited.insert(*callee_sym) {
                                    worklist.push(*callee_sym);
                                }
                            }
                            _ => {}
                        },
                        // Detect string equality / inequality comparisons.
                        // These do NOT emit an explicit MIR Call — instead the
                        // codegen injects a `string_compare` call at BinaryOp
                        // code-gen time. We must mark `string_compare` reachable
                        // here so the import is not tree-shaken.
                        // Function references used as handler arguments (e.g. passed to
                        // _http_route). The MIR builder emits:
                        //   Copy { source: Function(symbol_id) }
                        // with a local named "funcref_<name>". The BFS must follow these
                        // just like direct calls — otherwise bridge imports used inside
                        // the referenced function are not found reachable and the import
                        // is stripped. (GEN003: route handler → helper → bridge import)
                        MirOperation::Copy {
                            source: MirOperand::Function(callee_sym),
                        } => {
                            if let Some(name) = mir_program.symbol_name_map.get(callee_sym) {
                                insert_name(&mut names, &mut worklist, &mut visited, name);
                            }
                            if visited.insert(*callee_sym) {
                                worklist.push(*callee_sym);
                            }
                        }
                        MirOperation::AsyncFireCall { fn_name, .. } => {
                            names.insert("_async_fire".to_string());
                            if let Some(s) = name_to_symbol.get(fn_name.as_str()) {
                                if visited.insert(*s) {
                                    worklist.push(*s);
                                }
                            }
                        }
                        MirOperation::AsyncAwaitCall { fn_name, .. } => {
                            names.insert("_async_await".to_string());
                            if let Some(s) = name_to_symbol.get(fn_name.as_str()) {
                                if visited.insert(*s) {
                                    worklist.push(*s);
                                }
                            }
                        }
                        MirOperation::BinaryOp {
                            op: MirBinaryOp::Eq | MirBinaryOp::Ne,
                            left,
                            right,
                        } => {
                            let left_is_string = match left {
                                MirOperand::Value(vid) => current_func
                                    .locals
                                    .get(vid)
                                    .map(|l| is_string_type(&l.local_type))
                                    .unwrap_or(false),
                                _ => false,
                            };
                            let right_is_string = match right {
                                MirOperand::Value(vid) => current_func
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
                        // `a and b` / `a or b` short-circuit. The rhs's MIR
                        // instructions are embedded in the operation rather
                        // than living in any block — walk them so the
                        // call-graph BFS sees their callees, named imports,
                        // and string-comparison BinaryOps. Without this,
                        // e.g. `i < len and substring(i, i+1) == " "` would
                        // tree-shake `string_compare` (the rhs's `==` never
                        // appears in a block) and codegen would silently
                        // fall back to pointer-equality `i32.eq`. The walk
                        // is intentionally a flat scan rather than a
                        // recursive descent — at present rhs_instructions
                        // is always a linear sequence (the MIR builder
                        // rejects rhs expressions that open control flow,
                        // see `build_short_circuit_logical`).
                        MirOperation::LogicalShortCircuit {
                            rhs_instructions, ..
                        } => {
                            for sub in rhs_instructions {
                                match &sub.operation {
                                    MirOperation::Call {
                                        function: MirOperand::NamedFunction { name, symbol_id },
                                        ..
                                    } => {
                                        insert_name(&mut names, &mut worklist, &mut visited, name);
                                        let callee = if symbol_id.0 != 0 {
                                            Some(*symbol_id)
                                        } else {
                                            name_to_symbol.get(name.as_str()).copied()
                                        };
                                        if let Some(s) = callee {
                                            if visited.insert(s) {
                                                worklist.push(s);
                                            }
                                        }
                                    }
                                    MirOperation::Call {
                                        function: MirOperand::Function(callee_sym),
                                        ..
                                    } => {
                                        if let Some(name) =
                                            mir_program.symbol_name_map.get(callee_sym)
                                        {
                                            insert_name(
                                                &mut names,
                                                &mut worklist,
                                                &mut visited,
                                                name,
                                            );
                                        }
                                        if visited.insert(*callee_sym) {
                                            worklist.push(*callee_sym);
                                        }
                                    }
                                    MirOperation::Copy {
                                        source: MirOperand::Function(callee_sym),
                                    } => {
                                        if let Some(name) =
                                            mir_program.symbol_name_map.get(callee_sym)
                                        {
                                            insert_name(
                                                &mut names,
                                                &mut worklist,
                                                &mut visited,
                                                name,
                                            );
                                        }
                                        if visited.insert(*callee_sym) {
                                            worklist.push(*callee_sym);
                                        }
                                    }
                                    MirOperation::BinaryOp {
                                        op: MirBinaryOp::Eq | MirBinaryOp::Ne,
                                        left,
                                        right,
                                    } => {
                                        let left_is_str = match left {
                                            MirOperand::Value(vid) => current_func
                                                .locals
                                                .get(vid)
                                                .map(|l| is_string_type(&l.local_type))
                                                .unwrap_or(false),
                                            _ => false,
                                        };
                                        let right_is_str = match right {
                                            MirOperand::Value(vid) => current_func
                                                .locals
                                                .get(vid)
                                                .map(|l| is_string_type(&l.local_type))
                                                .unwrap_or(false),
                                            _ => false,
                                        };
                                        if left_is_str || right_is_str {
                                            names.insert("string_compare".to_string());
                                            names.insert("string.compare".to_string());
                                        }
                                    }
                                    _ => {}
                                }
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
        // The `__json_*` prefix covers the typed-collection encoders
        // (`__json_encode_cln_list`, `__json_encode_cln_pairs`) emitted by the
        // `json.encode(List<T>)` / `json.encode(Pairs<K,V>)` dispatch in
        // `mir_builder/expressions.rs` — those are the only call-graph names
        // when the user wrote `json.encode(...)` but never wrote any plain
        // string concatenation, so without this case the helpers would be
        // skipped at registration time and codegen would fail with
        // "Function '__json_encode_cln_pairs' not found in function map".
        if names
            .iter()
            .any(|n| n.starts_with("json.") || n.starts_with("json_") || n.starts_with("__json_"))
        {
            names.insert("string.concat".to_string());
        }

        // NOTE: the `now → _time_now` expansion is deliberately deferred to
        // run *after* the preamble-helper fixpoint below — the fixpoint can
        // add `now` to `names` when a kept-but-not-BFS-reachable user
        // function calls it (e.g. `doc_delete` doing
        // `Document.update: set: x = now()` in client mode). Expanding here
        // would miss those late additions and re-introduce
        // ORM-NOW-BRIDGE-CODEGEN-MISS-IN-CLEAN-STUDIO-CONTEXT (fp
        // `6e78a9f165f8`).

        // Flat scan for synthetic utility imports (SymbolId >= 1000).
        //
        // The BFS above only visits functions reachable from entry points. But
        // codegen generates ALL non-preamble functions in mir_program.functions,
        // including dead ones (unreachable from start). A dead function that calls
        // string.concat still needs the import registered — otherwise codegen
        // fails with "Function 'string.concat' not found in function map".
        //
        // Plugin preamble functions (location.file == "<plugin-output>") are
        // excluded here: they are dead-code eliminated from sorted_functions when
        // not reachable from the BFS (GEN003). We must not add their bridge imports
        // to the reachable set — otherwise ungated imports like _email_send leak
        // into the WASM import section even when no user code ever calls email.
        //
        // Layer 3 server imports (_http_*, _req_*, etc.) are NOT affected here
        // because those are never emitted as synthetic SymbolId calls; they come
        // from NamedFunction operands and are correctly gated by the BFS above.
        for function in mir_program.functions.values() {
            if function.location.file == "<plugin-output>" {
                continue; // dead-code eliminated when unreachable; don't register their imports
            }
            for block in function.blocks.values() {
                for instruction in &block.instructions {
                    match &instruction.operation {
                        MirOperation::Call {
                            function: MirOperand::Function(sym),
                            ..
                        } if sym.0 >= 1000 => {
                            if let Some(name) = mir_program.symbol_name_map.get(sym) {
                                insert_name(&mut names, &mut worklist, &mut visited, name);
                            }
                        }
                        // NamedFunction calls to non-Layer3 functions in dead code.
                        //
                        // User endpoint handlers (e.g. getTimestamp) may call Layer2
                        // bridge functions (time.now, db.query) without being reachable
                        // from start: via BFS. We include these here so their imports
                        // are registered. Server-only bridges (identified by their
                        // plugin manifest `hosts` field excluding "browser"/"all")
                        // are intentionally excluded — they only register when
                        // reachable from a seeded entry point (Import Minimality
                        // Rule, GEN003). See `server_only_bridge_names` above.
                        MirOperation::Call {
                            function: MirOperand::NamedFunction { name, .. },
                            ..
                        } if !server_only_bridge_names.contains(name) => {
                            insert_name(&mut names, &mut worklist, &mut visited, name);
                        }
                        MirOperation::BinaryOp {
                            op: MirBinaryOp::Eq | MirBinaryOp::Ne,
                            left,
                            right,
                        } => {
                            let left_str = match left {
                                MirOperand::Value(vid) => function
                                    .locals
                                    .get(vid)
                                    .map(|l| is_string_type(&l.local_type))
                                    .unwrap_or(false),
                                _ => false,
                            };
                            let right_str = match right {
                                MirOperand::Value(vid) => function
                                    .locals
                                    .get(vid)
                                    .map(|l| is_string_type(&l.local_type))
                                    .unwrap_or(false),
                                _ => false,
                            };
                            if left_str || right_str {
                                names.insert("string_compare".to_string());
                                names.insert("string.compare".to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Fixpoint: preserve plugin-preamble helpers transitively called from
        // any non-preamble function that will be retained
        // (ORM-MUTATION-OPS-MISSING-IN-CLIENT-CODEGEN-FUNCTION-MAP, fp
        // 44c1dc978900).
        //
        // In client builds (and in plugin-source builds in general) the BFS
        // roots above intentionally exclude user-defined functions to keep
        // server-only bridge imports from leaking. But user functions are
        // *kept* in MIR unconditionally (see `mod.rs::generate()` DCE
        // filter, which only filters PLUGIN_OUTPUT_MARKER). A user function
        // that calls a plugin-emitted helper (e.g. frame.data's
        // `__Widget_raw_update`/`__Widget_raw_delete`) would leave a
        // dangling reference once the helper is DCE'd:
        //     `Function '__Widget_raw_update' not found in function map`
        //
        // Sweep every non-preamble function (regardless of BFS reachability)
        // plus every preamble already in `names`, collect preamble names
        // they call, and iterate until no new names are added. Bridge
        // imports inside the kept preamble bodies still flow through the
        // host-mismatch stubbing path in `register_plugin_bridge_imports`,
        // so the Import Minimality Rule remains intact.
        let preamble_names_in_mir: HashSet<String> = mir_program
            .functions
            .values()
            .filter(|f| f.location.file == crate::ast::PLUGIN_OUTPUT_MARKER)
            .map(|f| f.name.clone())
            .collect();

        if !preamble_names_in_mir.is_empty() {
            // Resolve any MIR operation form that can reference a callee to
            // the name we'd find in `preamble_names_in_mir`. The fixpoint
            // walked only `MirOperation::Call` initially, but ORM blocks like
            // `Model.count: where: …` can be lowered in shapes the simple
            // Call walk misses (e.g. inside a `LogicalShortCircuit` rhs, or
            // surfaced as a function-pointer `Copy` when handed to a host
            // bridge). Centralizing the resolution keeps every variant on the
            // same path. (CODEGEN-ORM-METHOD-NOT-IN-FN-MAP, fp `ea5d66dcf89e`)
            let called_names_in_instr =
                |instr: &crate::mir::mir_types::MirInstruction, out: &mut Vec<String>| {
                    let push_operand = |op: &MirOperand, out: &mut Vec<String>| match op {
                        MirOperand::NamedFunction { name, .. } => out.push(name.clone()),
                        MirOperand::Function(sym) => {
                            if let Some(n) = mir_program.symbol_name_map.get(sym) {
                                out.push(n.clone());
                            }
                        }
                        _ => {}
                    };
                    match &instr.operation {
                        MirOperation::Call { function, .. } => push_operand(function, out),
                        MirOperation::Copy {
                            source: source @ MirOperand::Function(_),
                        } => push_operand(source, out),
                        MirOperation::AsyncFireCall { fn_name, .. }
                        | MirOperation::AsyncAwaitCall { fn_name, .. } => out.push(fn_name.clone()),
                        MirOperation::LogicalShortCircuit {
                            rhs_instructions, ..
                        } => {
                            for sub in rhs_instructions {
                                match &sub.operation {
                                    MirOperation::Call { function, .. } => {
                                        push_operand(function, out)
                                    }
                                    MirOperation::Copy {
                                        source: source @ MirOperand::Function(_),
                                    } => push_operand(source, out),
                                    MirOperation::AsyncFireCall { fn_name, .. }
                                    | MirOperation::AsyncAwaitCall { fn_name, .. } => {
                                        out.push(fn_name.clone())
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                };

            loop {
                let mut grew = false;
                let mut buf: Vec<String> = Vec::new();
                for f in mir_program.functions.values() {
                    let is_preamble = f.location.file == crate::ast::PLUGIN_OUTPUT_MARKER;
                    // Only walk functions that will actually be retained:
                    // non-preamble are always kept; preamble is kept only
                    // when its name is in `names`.
                    if is_preamble && !names.contains(&f.name) {
                        continue;
                    }
                    for block in f.blocks.values() {
                        for instr in &block.instructions {
                            buf.clear();
                            called_names_in_instr(instr, &mut buf);
                            for name in buf.drain(..) {
                                // Filter for server-only bridges — these MUST NOT
                                // be propagated from non-BFS-reachable code or
                                // they'd appear as imports in the client build
                                // (CLIENT_MODULE_LEAK). Preserved preamble bodies
                                // (`is_preamble == true`) get to keep them too —
                                // the host-mismatch stubbing path handles those
                                // separately, so adding the name to `names` is a
                                // no-op for import registration.
                                if !is_preamble && server_only_bridge_names.contains(&name) {
                                    continue;
                                }
                                let is_pa = preamble_names_in_mir.contains(&name);
                                // Add EVERY remaining callee to `names`. Two
                                // reasons: (1) preamble names trigger another
                                // fixpoint iteration so their bodies get walked;
                                // (2) non-preamble names (bridge aliases like
                                // `now`, `today`, namespace fns like
                                // `string.concat`) feed the alias-expansion
                                // machinery further down — e.g. `names
                                // .contains("now")` flips `_time_now` into
                                // `names`, which keeps it from being tree-shaken
                                // at `register_import_function`. Without this,
                                // `now()` referenced inside a kept-but-not-BFS-
                                // reachable user function (e.g. `doc_delete`
                                // calling `Document.update: set: x = now()`)
                                // fails codegen with `Function 'now' not found
                                // in function map`.
                                // (ORM-NOW-BRIDGE-CODEGEN-MISS-IN-CLEAN-STUDIO-
                                // CONTEXT, fp `6e78a9f165f8`.)
                                if names.insert(name) && is_pa {
                                    grew = true;
                                }
                            }
                        }
                    }
                }
                if !grew {
                    break;
                }
            }
        }

        // now() is a bare alias for time.now() used by frame.data plugin-generated code.
        // language_to_bridge_map only has "time.now" → "_time_now", not "now" → "_time_now",
        // so without this explicit expansion _time_now is tree-shaken when only now() is called.
        // Runs after the preamble-helper fixpoint so additions from that pass are picked up
        // (e.g. user functions kept by the fixpoint that call now() — ORM-NOW-BRIDGE-CODEGEN-
        // MISS-IN-CLEAN-STUDIO-CONTEXT, fp `6e78a9f165f8`).
        if names.contains("now") || names.contains("time.now") {
            names.insert("_time_now".to_string());
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
            // camelCase crypto: dot→underscore expansion produces wrong import name
            ("crypto.randomHex", "_crypto_random_hex"),
            ("crypto.randomBytes", "_crypto_random_bytes"),
            // prefixed crypto: "crypto.sha256" → "_crypto_sha256" (wrong)
            //                   actual import:   "_crypto_hash_sha256"
            ("crypto.sha256", "_crypto_hash_sha256"),
            ("crypto.sha512", "_crypto_hash_sha512"),
            ("crypto.hashPassword", "_crypto_hash_password"),
            ("crypto.verifyPassword", "_crypto_verify_password"),
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

        // Include the names of every function that the BFS visited (seeded or
        // called from a seed). This ensures that plugin-generated functions such as
        // `start` (which is never *called* — it's the entry point) are in the
        // reachable set, so the dead-code elimination filter in `generate()` does
        // not accidentally drop them (GEN003).
        for sym in &visited {
            if let Some(f) = mir_program.functions.get(sym) {
                names.insert(f.name.clone());
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
            // "now" is a bare alias for "time.now" — when a plugin provides time.now
            // via the bridge map, resolve "now" calls through "time.now" so that
            // _time_now ends up in used_bridge_function_names and gets registered.
            if name == "now" {
                if let Some(bridge_name) = self.language_to_bridge_map.get("time.now") {
                    if bridge_function_names.contains(bridge_name.as_str()) {
                        return Some(bridge_name.clone());
                    }
                }
            }
            None
        };

        for function in mir_program.functions.values() {
            if function.location.file == "<plugin-output>" {
                // Dead preamble functions are eliminated by DCE — skip them to preserve
                // the Import Minimality Rule (GEN003). Reachable preamble functions ARE
                // compiled and their bridge imports MUST be registered. The BFS pass
                // (collect_all_called_names_from_mir) already ran and is the authority:
                // any preamble function in reachable_imports survived DCE and is compiled.
                let is_reachable = self
                    .wasm_generator
                    .reachable_imports
                    .as_ref()
                    .map(|r| r.contains(&function.name))
                    .unwrap_or(false);
                if !is_reachable {
                    continue;
                }
                // Reachable preamble — fall through and scan its bridge calls
            }
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

            // Register the bridge function's return type so that no-destination call sites
            // (expression statements) can determine whether to emit DROP. Without this, the
            // fallback path defaults to non-void and emits a spurious DROP after a void-return
            // bridge call, causing "type mismatch: nothing on stack" at WASM validation time.
            let mir_return = match &return_type {
                BuiltinType::Void => MirType::Void,
                BuiltinType::Integer | BuiltinType::Boolean => MirType::I32,
                BuiltinType::Number => MirType::F64,
                BuiltinType::String => MirType::Ptr(Box::new(MirType::I8)),
                _ => MirType::I32,
            };
            // Collect language aliases first to avoid borrow conflicts.
            let aliases: Vec<String> = self
                .language_to_bridge_map
                .iter()
                .filter(|(_, bridge)| **bridge == func.name)
                .map(|(lang, _)| lang.clone())
                .collect();
            self.function_return_types
                .insert(func.name.clone(), mir_return.clone());
            for alias in aliases.iter().cloned() {
                self.function_return_types.insert(alias, mir_return.clone());
            }

            // CLIENT_BUILD_ENTRY_LEAK — Plugin Contracts v2 §6 (Phase C+).
            //
            // If this bridge's `hosts` declaration excludes the active build's
            // host class (e.g. a Frame page companion's server-only `guard()`
            // calls `_auth_require_auth` in a `target: web` client build),
            // register a local no-op stub instead of an import. The
            // `BRIDGE-HOST-MISMATCH` warning has already informed the user;
            // the stub keeps the call site resolvable so the build succeeds
            // and DCE eliminates the surrounding dead code where possible.
            if bridge_is_host_mismatched(self.host_class.as_deref(), func.hosts.as_deref()) {
                let wasm_params: Vec<WasmType> = param_types
                    .iter()
                    .map(Self::builtin_type_to_wasm_type)
                    .collect();
                let wasm_return = match &return_type {
                    BuiltinType::Void => None,
                    _ => Some(Self::builtin_type_to_wasm_type(&return_type)),
                };
                // DEFER stub registration. Registering a local function via
                // `wasm_generator.register_function` here — mid-import-phase —
                // would shift the funcidx of every import emitted after it,
                // because WASM's funcidx space puts imports before locals
                // and `function_count` is the global counter. The resulting
                // `Call(N)` instructions would target the wrong function and
                // fail wasmparser validation (CODEGEN-WASM-STACK-MISMATCH /
                // CODEGEN_STACK_REMAINING — both originally caused by this
                // out-of-order registration). The deferred queue is drained
                // by `register_pending_host_mismatched_stubs` after every
                // other import is in place.
                self.pending_host_mismatched_stubs.push(
                    crate::codegen::mir_codegen::PendingHostMismatchedStub {
                        name: func.name.clone(),
                        params: wasm_params,
                        wasm_return,
                        aliases: aliases.clone(),
                    },
                );
                tracing::info!(
                    bridge = %func.name,
                    host_class = ?self.host_class,
                    bridge_hosts = ?func.hosts,
                    "Deferred no-op stub for host-mismatched bridge (CLIENT_BUILD_ENTRY_LEAK)"
                );
                continue;
            }

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
                    wrap_i64: false,
                });
            } else if func.name == "_time_now" {
                // _time_now is a special case: the host bridge contract specifies () -> i64
                // (Unix timestamp), but Clean's integer type is i32. Register the import with
                // the correct i64 return type, then defer a wrapper that applies i32.wrap_i64
                // so callers receive a Clean integer. This mirrors register_time_builtin_imports
                // + register_time_builtin_wrappers but handles the plugin-bridge code path
                // where language_to_bridge_map already maps "time.now" → "_time_now".
                let raw_index = self.wasm_generator.register_import_function(
                    module,
                    "_time_now",
                    &[],
                    Some(WasmType::I64),
                )?;

                if raw_index == u32::MAX {
                    continue;
                }

                tracing::debug!(
                    raw_index = raw_index,
                    "Registered _time_now as () -> i64 via plugin bridge path; deferring i32.wrap_i64 wrapper"
                );

                // Defer a wrapper: () -> i32, body = [call _time_now, i32.wrap_i64].
                // The wrapper is registered as "time.now" after all imports, and the
                // language alias is set to the wrapper index (not the raw import).
                self.pending_bridge_wrappers.push(
                    crate::codegen::mir_codegen::PendingBridgeWrapper {
                        name: func.name.clone(), // "_time_now" — wrapper phase maps alias to this
                        params: vec![],
                        wasm_return: Some(WasmType::I32),
                        raw_func_index: raw_index,
                        param_types: vec![],
                        wrap_i64: true,
                    },
                );
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

            if wrapper.raw_func_index == u32::MAX {
                continue; // import was tree-shaken, no wrapper needed
            }
            wrapper_instructions.push(Instruction::Call(wrapper.raw_func_index));

            // _time_now: host returns i64, Clean integer is i32 — wrap before returning.
            if wrapper.wrap_i64 {
                wrapper_instructions.push(Instruction::I32WrapI64);
            }

            // For the _time_now i64-wrap case the public name is "time.now" (not "_time_now").
            let public_name = if wrapper.name == "_time_now" {
                "time.now".to_string()
            } else {
                wrapper.name.clone()
            };

            tracing::debug!(
                name = %public_name,
                params = ?wrapper.params,
                returns = ?wrapper.wasm_return,
                raw_func_index = wrapper.raw_func_index,
                "Registering wrapper function for bridge (after all imports)"
            );

            self.wasm_generator.register_function(
                &public_name,
                &wrapper.params,
                wrapper.wasm_return,
                &wrapper_instructions,
            )?;

            // Register language-name aliases for the wrapper
            if let Some(&wrapper_index) = self.wasm_generator.function_map.get(&public_name) {
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
                // now() is a bare alias for time.now() used by frame.data plugin-generated code
                if public_name == "time.now" {
                    self.wasm_generator
                        .function_map
                        .insert("now".to_string(), wrapper_index);
                }
            }
        }

        Ok(())
    }

    /// Register the deferred no-op stubs for host-mismatched bridges queued
    /// during `register_plugin_bridge_imports`.
    ///
    /// Must be called AFTER all imports are emitted — these stubs are local
    /// functions and the WASM funcidx allocator counts imports first. See
    /// [`PendingHostMismatchedStub`] doc comment for the bug this prevents
    /// (CODEGEN-WASM-STACK-MISMATCH / CODEGEN_STACK_REMAINING).
    pub(super) fn register_pending_host_mismatched_stubs(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        use wasm_encoder::Instruction;

        let stubs = std::mem::take(&mut self.pending_host_mismatched_stubs);

        for stub in stubs {
            let body: Vec<Instruction> = match stub.wasm_return {
                None | Some(WasmType::Unit) => Vec::new(),
                Some(WasmType::I32) => vec![Instruction::I32Const(0)],
                Some(WasmType::I64) => vec![Instruction::I64Const(0)],
                Some(WasmType::F32) => vec![Instruction::F32Const(0.0)],
                Some(WasmType::F64) => vec![Instruction::F64Const(0.0)],
                Some(WasmType::V128) => vec![Instruction::V128Const(0)],
            };

            let stub_index = self.wasm_generator.register_function(
                &stub.name,
                &stub.params,
                stub.wasm_return,
                &body,
            )?;

            for alias in &stub.aliases {
                self.wasm_generator
                    .function_map
                    .insert(alias.clone(), stub_index);
            }

            tracing::info!(
                bridge = %stub.name,
                wasm_index = stub_index,
                "Registered no-op stub for host-mismatched bridge (deferred)"
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

            // Plugin bridge functions that come from plugin.toml are added to
            // ast.externals by lib.rs Stage 2.6 for all plugins, even when user
            // code never calls them (e.g. _email_send for a non-email app).
            // is_reachability_gated_import() only covers the platform-level
            // Layer 2/3 functions — not plugin-specific bridge functions like
            // _email_send or any future plugin-defined imports.
            //
            // Gate ALL externals by the BFS reachable set: if the function is
            // not in the reachable set and reachability filtering is active, skip it.
            // This preserves the Import Minimality Rule for plugin bridge functions.
            if let Some(reachable) = &self.wasm_generator.reachable_imports {
                if !reachable.contains(&external.name) {
                    tracing::debug!(
                        name = %external.name,
                        "Skipping unreachable external function (Import Minimality Rule)"
                    );
                    continue;
                }
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

            // register_import_function returns u32::MAX when the import is
            // tree-shaken (Import Minimality Rule). Never store the sentinel
            // in function_map — it propagates to symbol_to_function_index
            // and produces Call(u32::MAX) in function bodies (GEN003).
            if func_index == u32::MAX {
                tracing::debug!(
                    name = %external.name,
                    "Skipping tree-shaken external function (not reachable)"
                );
                continue;
            }

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

#[cfg(test)]
mod host_class_enforcement_tests {
    use super::*;
    use crate::codegen::mir_codegen::MirCodeGenerator;
    use crate::plugins::BridgeFunction;

    fn bridge(name: &str, hosts: Option<Vec<&str>>) -> BridgeFunction {
        BridgeFunction {
            name: name.to_string(),
            params: vec!["string".to_string()],
            returns: "string".to_string(),
            module: "env".to_string(),
            description: None,
            expand_strings: true,
            hosts: hosts.map(|v| v.into_iter().map(String::from).collect()),
            ..Default::default()
        }
    }

    #[test]
    fn test_no_host_class_disables_check() {
        let mut gen = MirCodeGenerator::new_minimal();
        gen.set_bridge_functions(vec![bridge("_db_query", Some(vec!["server"]))]);
        gen.set_host_class(None);
        let reachable: HashSet<String> = ["_db_query".to_string()].into_iter().collect();

        let diags = gen.check_bridge_host_classes(&reachable);
        assert!(
            diags.is_empty(),
            "no host_class set must skip enforcement entirely"
        );
    }

    #[test]
    fn test_bridge_without_hosts_is_skipped() {
        // Phase C compatibility — third-party plugins without hosts continue
        // to compile cleanly. Phase D will surface a deprecation warning.
        let mut gen = MirCodeGenerator::new_minimal();
        gen.set_bridge_functions(vec![bridge("_legacy_func", None)]);
        gen.set_host_class(Some("browser".to_string()));
        let reachable: HashSet<String> = ["_legacy_func".to_string()].into_iter().collect();

        let diags = gen.check_bridge_host_classes(&reachable);
        assert!(diags.is_empty());
    }

    #[test]
    fn test_server_bridge_in_browser_build_fails() {
        // The canonical BRIDGE-HOST-MISMATCH case.
        let mut gen = MirCodeGenerator::new_minimal();
        gen.set_bridge_functions(vec![bridge("_db_query", Some(vec!["server"]))]);
        gen.set_host_class(Some("browser".to_string()));
        let reachable: HashSet<String> = ["_db_query".to_string()].into_iter().collect();

        let diags = gen.check_bridge_host_classes(&reachable);
        assert_eq!(diags.len(), 1);
        let msg = format!("{}", diags[0]);
        assert!(msg.contains("BRIDGE-HOST-MISMATCH"));
        assert!(msg.contains("_db_query"));
        assert!(msg.contains("server"));
    }

    #[test]
    fn test_all_hosts_value_is_accepted_by_every_host_class() {
        let mut gen = MirCodeGenerator::new_minimal();
        gen.set_bridge_functions(vec![bridge("file_read", Some(vec!["all"]))]);
        let reachable: HashSet<String> = ["file_read".to_string()].into_iter().collect();

        for host in ["server", "browser", "native"] {
            gen.set_host_class(Some(host.to_string()));
            let diags = gen.check_bridge_host_classes(&reachable);
            assert!(
                diags.is_empty(),
                "all-hosts bridge must be callable from {} target",
                host
            );
        }
    }

    #[test]
    fn test_multiple_hosts_passes_when_current_listed() {
        let mut gen = MirCodeGenerator::new_minimal();
        gen.set_bridge_functions(vec![bridge(
            "_ui_get_bounds",
            Some(vec!["browser", "server"]),
        )]);
        let reachable: HashSet<String> = ["_ui_get_bounds".to_string()].into_iter().collect();

        for host in ["server", "browser"] {
            gen.set_host_class(Some(host.to_string()));
            assert!(gen.check_bridge_host_classes(&reachable).is_empty());
        }

        gen.set_host_class(Some("native".to_string()));
        assert_eq!(gen.check_bridge_host_classes(&reachable).len(), 1);
    }

    #[test]
    fn test_unreachable_bridge_is_not_diagnosed() {
        // Phase C respects Import Minimality — if the bridge is never called
        // in this build's reachable graph, no diagnostic.
        let mut gen = MirCodeGenerator::new_minimal();
        gen.set_bridge_functions(vec![bridge("_db_query", Some(vec!["server"]))]);
        gen.set_host_class(Some("browser".to_string()));

        let empty_reachable: HashSet<String> = HashSet::new();
        assert!(gen.check_bridge_host_classes(&empty_reachable).is_empty());
    }

    #[test]
    fn test_dot_alias_reachability_is_picked_up() {
        // db.query in user code resolves to _db_query at codegen via the
        // language_to_bridge_map. The enforcement must follow the alias so a
        // call written as `db.query(...)` isn't a false negative.
        let mut gen = MirCodeGenerator::new_minimal();
        gen.set_bridge_functions(vec![bridge("_db_query", Some(vec!["server"]))]);
        gen.set_language_to_bridge_map(
            [("db.query".to_string(), "_db_query".to_string())]
                .into_iter()
                .collect(),
        );
        gen.set_host_class(Some("browser".to_string()));

        // Only the language-level name appears in the reachable set.
        let reachable: HashSet<String> = ["db.query".to_string()].into_iter().collect();
        let diags = gen.check_bridge_host_classes(&reachable);
        assert_eq!(diags.len(), 1, "alias must propagate to enforcement");
    }

    /// CLIENT_BUILD_ENTRY_LEAK regression test.
    ///
    /// When a reachable bridge function declares `hosts = ["server"]` and the
    /// active build's host class is `"browser"`, `register_plugin_bridge_imports`
    /// must register a local no-op stub under the bridge's name (and all its
    /// language aliases) instead of attempting an import that the browser host
    /// cannot provide. Without the stub, codegen subsequently fails with
    /// "Function `_auth_require_auth` not found in function map".
    #[test]
    fn test_host_mismatched_bridge_is_stubbed_in_function_map() {
        let mut gen = MirCodeGenerator::new_minimal();
        // Bridge with no string params — avoids the wrapper path so the test
        // exercises the direct-import branch where the stub substitution lives.
        let bf = BridgeFunction {
            name: "_auth_require_auth".to_string(),
            params: vec![],
            returns: "integer".to_string(),
            module: "env".to_string(),
            description: None,
            expand_strings: false,
            hosts: Some(vec!["server".to_string()]),
            ..Default::default()
        };
        gen.set_bridge_functions(vec![bf]);
        gen.set_language_to_bridge_map(
            [(
                "auth.requireAuth".to_string(),
                "_auth_require_auth".to_string(),
            )]
            .into_iter()
            .collect(),
        );
        gen.set_host_class(Some("browser".to_string()));
        gen.used_bridge_function_names
            .insert("_auth_require_auth".to_string());

        gen.register_plugin_bridge_imports()
            .expect("stub queuing must not fail");

        // The import phase must DEFER stub registration to avoid shifting
        // funcidx for subsequent imports (CODEGEN-WASM-STACK-MISMATCH /
        // CODEGEN_STACK_REMAINING). The function_map should not contain
        // the bridge name yet.
        assert!(
            !gen.wasm_generator
                .function_map
                .contains_key("_auth_require_auth"),
            "stub must NOT be in function_map after import phase — registration is deferred"
        );
        assert_eq!(
            gen.pending_host_mismatched_stubs.len(),
            1,
            "the host-mismatched bridge must be queued as a pending stub"
        );
        let queued = &gen.pending_host_mismatched_stubs[0];
        assert_eq!(queued.name, "_auth_require_auth");
        assert!(
            queued.aliases.iter().any(|a| a == "auth.requireAuth"),
            "language alias must be carried on the queued stub for later registration"
        );

        // Drain the queue (mirrors what generate() does after imports are done).
        gen.register_pending_host_mismatched_stubs()
            .expect("deferred stub registration must succeed");

        let function_map = &gen.wasm_generator.function_map;
        assert!(
            function_map.contains_key("_auth_require_auth"),
            "bridge name must resolve to the stub after the deferred registration"
        );
        assert!(
            function_map.contains_key("auth.requireAuth"),
            "language alias must also resolve to the stub after deferred registration"
        );
        assert_eq!(
            function_map.get("_auth_require_auth"),
            function_map.get("auth.requireAuth"),
            "alias and canonical name must point to the same stub index"
        );
    }

    #[test]
    fn test_host_matched_bridge_uses_import_path() {
        // Counter-check: when hosts include the active class, the import path
        // runs as normal — no stub substitution.
        let mut gen = MirCodeGenerator::new_minimal();
        let bf = BridgeFunction {
            name: "_browser_only_fn".to_string(),
            params: vec![],
            returns: "integer".to_string(),
            module: "env".to_string(),
            description: None,
            expand_strings: false,
            hosts: Some(vec!["browser".to_string()]),
            ..Default::default()
        };
        gen.set_bridge_functions(vec![bf]);
        gen.set_host_class(Some("browser".to_string()));
        gen.used_bridge_function_names
            .insert("_browser_only_fn".to_string());

        gen.register_plugin_bridge_imports()
            .expect("import registration must not fail for host-matched bridge");

        // Import-path success: the bridge name lands in function_map via the
        // import-index path (also valid for stub). The key signal that this
        // is NOT the stub path: function_return_types is populated by the
        // shared preamble for both paths, so we don't differentiate on that;
        // instead, the unit test relies on the negative-correlation test
        // above (host_mismatched) catching the differing branch.
        assert!(
            gen.wasm_generator
                .function_map
                .contains_key("_browser_only_fn"),
            "host-matched bridge must still be registered"
        );
    }
}
