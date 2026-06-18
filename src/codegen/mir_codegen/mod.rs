//! MIR to WebAssembly Code Generation
//!
//! This module implements code generation from MIR (Medium-level Intermediate Representation)
//! to WebAssembly bytecode. It provides a cleaner, more optimized path from typed code
//! to WASM compared to the direct AST-to-WASM generation.
//!
//! The implementation is split into focused submodules:
//!
//! * [`blocks`]       — function-level and basic-block emission
//! * [`control_flow`] — CFG analysis helpers (loop detection, continuation detection)
//! * [`instructions`] — per-instruction and per-terminator WASM emission
//! * [`operands`]     — operand loading, constant loading, string expansion
//! * [`utilities`]    — type helpers, module setup, bridge registration

use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::mir::mir_types::{
    AnyTypeTag, BasicBlockId, MirBasicBlock, MirBinaryOp, MirConstant, MirFunction, MirInstruction,
    MirOperand, MirOperation, MirProgram, MirTerminator, MirType, MirUnaryOp, ValueId,
};
use crate::resolver::SymbolId;
use std::collections::{HashMap, HashSet};
use wasm_encoder::{BlockType, Instruction, ValType};

// Submodules
mod blocks;
mod control_flow;
mod instructions;
mod operands;
mod utilities;

// ---------------------------------------------------------------------------
// Debug macro (must be in mod.rs so `use super::*` imports it in submodules)
// ---------------------------------------------------------------------------

/// Conditional debug macro for MIR code generation — maps to `tracing::trace!`.
macro_rules! debug_mir {
    ($($arg:tt)*) => {
        tracing::trace!($($arg)*)
    };
}

// Make the macro accessible to all submodules via `use super::*`
pub(super) use debug_mir;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of MIR code generation.
#[derive(Debug)]
pub struct MirCodegenResult {
    /// Generated WASM bytecode.
    pub wasm_bytes: Vec<u8>,

    /// Generation statistics.
    pub stats: MirCodegenStats,

    /// Warnings produced during generation.
    pub warnings: Vec<CompilerError>,
}

/// Statistics about MIR code generation.
#[derive(Debug, Default)]
pub struct MirCodegenStats {
    /// Number of functions generated.
    pub functions_generated: usize,

    /// Number of basic blocks generated.
    pub blocks_generated: usize,

    /// Number of instructions generated.
    pub instructions_generated: usize,

    /// Generation time in microseconds.
    pub generation_time_us: u64,
}

// ---------------------------------------------------------------------------
// Internal types (shared across submodules via `use super::*`)
// ---------------------------------------------------------------------------

/// Context for loop code generation.
#[derive(Clone)]
pub(super) struct LoopCodegenContext {
    /// MIR block ID of the loop's exit block (where `break` jumps to).
    pub(super) exit_block_id: BasicBlockId,
    /// WASM block depth when the outer `Block` instruction was pushed.
    /// Used to calculate the relative `br` depth for `break`.
    pub(super) block_depth: u32,
}

/// Info for a pending bridge wrapper function.
///
/// Wrapper functions must be registered AFTER all imports are done to avoid
/// function-index collisions between WASM imports and internal functions.
#[derive(Clone)]
pub(super) struct PendingBridgeWrapper {
    pub(super) name: String,
    pub(super) params: Vec<crate::types::WasmType>,
    pub(super) wasm_return: Option<crate::types::WasmType>,
    pub(super) raw_func_index: u32,
    pub(super) param_types: Vec<crate::builtins::registry::BuiltinType>,
    /// When true, emit `i32.wrap_i64` after the call (host returns i64, Clean caller expects i32).
    pub(super) wrap_i64: bool,
}

/// Info for a deferred no-op stub for a host-mismatched bridge.
///
/// Like [`PendingBridgeWrapper`], these must be registered AFTER every WASM
/// import to keep function-index allocation correct. WASM's funcidx space
/// puts all imported functions before any local functions; registering a
/// local function mid-import phase would shift the indices of every import
/// that follows, producing a Call(N) that targets the wrong function and
/// fails wasmparser validation (CODEGEN-WASM-STACK-MISMATCH / CODEGEN_STACK_REMAINING).
///
/// The body is a single zero-valued constant matching `wasm_return` so the
/// stub satisfies its declared signature without doing any work.
#[derive(Clone)]
pub(super) struct PendingHostMismatchedStub {
    pub(super) name: String,
    pub(super) params: Vec<crate::types::WasmType>,
    pub(super) wasm_return: Option<crate::types::WasmType>,
    /// Language-name aliases (e.g. "auth.requireAuth") that must point at
    /// the same stub index in function_map once registered.
    pub(super) aliases: Vec<String>,
}

/// Per-function generation statistics (internal).
#[derive(Debug, Default)]
pub(super) struct FunctionStats {
    pub(super) blocks_generated: usize,
    pub(super) instructions_generated: usize,
}

// ---------------------------------------------------------------------------
// MirCodeGenerator struct
// ---------------------------------------------------------------------------

/// MIR to WASM code generator.
pub struct MirCodeGenerator<'a> {
    /// The underlying WASM code generator.
    pub(super) wasm_generator: CodeGenerator,

    /// Mapping from MIR `ValueId` to WASM local indices.
    pub(super) value_to_local: HashMap<ValueId, u32>,

    /// Mapping from MIR `BasicBlockId` to WASM block label counters.
    pub(super) block_labels: HashMap<BasicBlockId, u32>,

    /// Current WASM local index counter.
    pub(super) next_local_index: u32,

    /// Current WASM block label counter.
    pub(super) next_block_label: u32,

    /// Stack of WASM instructions for the current function.
    pub(super) current_instructions: Vec<Instruction<'a>>,

    /// The function currently being generated.
    pub(super) current_function: Option<MirFunction>,

    /// String pool from the MIR program for string constant handling.
    pub(super) string_pool: Option<Vec<String>>,

    /// Mapping from `ValueId` to string pool index (for string constants loaded as locals).
    pub(super) value_to_string_index: HashMap<ValueId, usize>,

    /// Mapping from `SymbolId` to function name for proper function resolution.
    pub(super) function_symbol_map: HashMap<SymbolId, String>,

    /// Direct mapping from `SymbolId` to WASM function index.
    ///
    /// Avoids name collisions for constructors/methods with the same names.
    pub(super) symbol_to_function_index: HashMap<SymbolId, u32>,

    /// Function signature map for parameter/return type handling.
    pub(super) function_signatures: HashMap<SymbolId, MirFunction>,

    /// Name-based return type registry for stdlib/builtin functions.
    ///
    /// Used when `SymbolId`-based signature lookup fails (e.g., for stdlib functions
    /// that don't have fixed `SymbolId`s).
    pub(super) function_return_types: HashMap<String, MirType>,

    /// Type tracking for values (needed to expand string pointers).
    pub(super) value_to_type: HashMap<ValueId, MirType>,

    /// Types of temporary locals created during code generation.
    ///
    /// Maps local index → WASM type for temporaries (e.g., string expansion temps).
    pub(super) temp_local_types: HashMap<u32, ValType>,

    /// Plugin bridge functions to register as WASM imports.
    pub(super) bridge_functions: Vec<crate::plugins::BridgeFunction>,

    /// Set of bridge function names actually used in the code.
    ///
    /// Used for selective imports — only import functions that are called.
    pub(super) used_bridge_function_names: HashSet<String>,

    /// Pending wrapper functions to be registered AFTER ALL imports are done.
    pub(super) pending_bridge_wrappers: Vec<PendingBridgeWrapper>,

    /// Pending host-mismatched bridge stubs to be registered AFTER ALL
    /// imports are done. See [`PendingHostMismatchedStub`] for why this is
    /// deferred (the bug it prevents is CODEGEN-WASM-STACK-MISMATCH).
    pub(super) pending_host_mismatched_stubs: Vec<PendingHostMismatchedStub>,

    /// Stack of loop contexts for proper break/continue code generation.
    pub(super) loop_context_stack: Vec<LoopCodegenContext>,

    /// Current block nesting depth (for calculating `br` depths).
    pub(super) current_block_depth: u32,

    /// Mapping from state variable `SymbolId` to WASM global index.
    ///
    /// Global index 0 is reserved for the heap pointer; state variables start at index 1.
    pub(super) state_global_indices: HashMap<SymbolId, u32>,

    /// State variable globals for the global section (populated before `finalize_module`).
    pub(super) state_globals: Vec<(SymbolId, String, MirType, Option<MirConstant>)>,

    /// Compilation target — determines which imports to include.
    pub(super) target: crate::CompilationTarget,

    /// WASM function indices for `external:` block functions.
    pub(super) external_function_indices: HashMap<String, u32>,

    /// Mapping from language API function names to bridge function names.
    ///
    /// Example: `"db.query"` → `"_db_query"`.
    pub(super) language_to_bridge_map: HashMap<String, String>,

    /// Handler index tracking for plugin callback dispatch.
    ///
    /// Maps function name → handler index (0, 1, 2, …).
    pub(super) handler_indices: HashMap<String, u32>,

    /// Next handler index to assign (auto-incrementing counter).
    pub(super) next_handler_index: u32,

    /// Bridge function parameter type info for handler resolution.
    ///
    /// Maps bridge function name → list of `BuiltinType` parameter types.
    pub(super) bridge_param_types: HashMap<String, Vec<crate::builtins::registry::BuiltinType>>,

    /// Memory budget tier — controls initial/max pages in the WASM memory section
    /// and the `clean:memory` custom section emitted into the module.
    pub(super) memory_tier: crate::MemoryTier,

    /// WASM function indices referenced as first-class values (function pointers).
    ///
    /// Populated while lowering `MirOperand::Function` / `MirOperand::NamedFunction`
    /// to i32 constants. When non-empty, a `__indirect_function_table` is emitted
    /// and populated with these entries so hosts can invoke referenced functions
    /// via `call_indirect` using the i32 value as the table index.
    pub(super) referenced_function_indices: HashSet<u32>,

    /// Names of functions that originate from the user program (including plugin-generated
    /// functions from `expand_block`). These are always exported regardless of naming
    /// convention — including double-underscore route handler stubs like `__redirect_0`.
    /// Compiler-internal helpers (`__malloc`, `__string_concat`, etc.) are NOT in this
    /// set because they are registered via `register_function`, not from `mir_program.functions`.
    pub(super) user_defined_function_names: HashSet<String>,

    /// Host class this build targets, used by Plugin Contracts v2 bridge-host
    /// enforcement (`foundation/spec/plugins/contracts/bridge-host-classes.md` §6).
    /// `None` disables the check (e.g. plugin builds). Values: `"server"`,
    /// `"browser"`, `"native"`.
    pub(super) host_class: Option<String>,

    /// When true, bridge-host mismatches against the active `host_class` are
    /// returned as compile errors. When false (Phase C default), mismatches
    /// are emitted as warnings and the build proceeds. Set via the
    /// `CLEAN_STRICT_HOSTS=1` environment variable or `--strict-hosts` CLI flag.
    pub(super) strict_hosts: bool,

    /// When true, the output is a browser `frontend.wasm`. DCE roots are
    /// restricted to `_start` + explicitly exported functions only — user-defined
    /// functions that are not reachable from those roots are dead-code eliminated
    /// so server-only bridge imports (`_db_query`, `_http_listen`, etc.) are never
    /// emitted into the browser module. See CLIENT_MODULE_LEAK.
    pub(super) client_mode: bool,
}

// ---------------------------------------------------------------------------
// Constructors and public configuration methods
// ---------------------------------------------------------------------------

impl MirCodeGenerator<'_> {
    /// Create a new MIR code generator with default (Server) target.
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
            function_return_types: HashMap::new(),
            value_to_type: HashMap::new(),
            temp_local_types: HashMap::new(),
            bridge_functions: Vec::new(),
            used_bridge_function_names: HashSet::new(),
            pending_bridge_wrappers: Vec::new(),
            pending_host_mismatched_stubs: Vec::new(),
            loop_context_stack: Vec::new(),
            current_block_depth: 0,
            state_global_indices: HashMap::new(),
            state_globals: Vec::new(),
            target: crate::CompilationTarget::Server,
            external_function_indices: HashMap::new(),
            language_to_bridge_map: HashMap::new(),
            handler_indices: HashMap::new(),
            next_handler_index: 0,
            bridge_param_types: HashMap::new(),
            memory_tier: crate::MemoryTier::Standard,
            referenced_function_indices: HashSet::new(),
            user_defined_function_names: HashSet::new(),
            host_class: None,
            strict_hosts: false,
            client_mode: false,
        }
    }

    /// Create a new MIR code generator for testing (without runtime imports).
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
            function_return_types: HashMap::new(),
            value_to_type: HashMap::new(),
            temp_local_types: HashMap::new(),
            bridge_functions: Vec::new(),
            used_bridge_function_names: HashSet::new(),
            pending_bridge_wrappers: Vec::new(),
            pending_host_mismatched_stubs: Vec::new(),
            loop_context_stack: Vec::new(),
            current_block_depth: 0,
            state_global_indices: HashMap::new(),
            state_globals: Vec::new(),
            target: crate::CompilationTarget::Standalone,
            external_function_indices: HashMap::new(),
            language_to_bridge_map: HashMap::new(),
            handler_indices: HashMap::new(),
            next_handler_index: 0,
            bridge_param_types: HashMap::new(),
            memory_tier: crate::MemoryTier::Standard,
            referenced_function_indices: HashSet::new(),
            user_defined_function_names: HashSet::new(),
            host_class: None,
            strict_hosts: false,
            client_mode: false,
        }
    }

    /// Create a new MIR code generator with a specific compilation target.
    pub fn with_target(target: crate::CompilationTarget) -> Self {
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
            function_return_types: HashMap::new(),
            value_to_type: HashMap::new(),
            temp_local_types: HashMap::new(),
            bridge_functions: Vec::new(),
            used_bridge_function_names: HashSet::new(),
            pending_bridge_wrappers: Vec::new(),
            pending_host_mismatched_stubs: Vec::new(),
            loop_context_stack: Vec::new(),
            current_block_depth: 0,
            state_global_indices: HashMap::new(),
            state_globals: Vec::new(),
            target,
            external_function_indices: HashMap::new(),
            language_to_bridge_map: HashMap::new(),
            handler_indices: HashMap::new(),
            next_handler_index: 0,
            bridge_param_types: HashMap::new(),
            memory_tier: crate::MemoryTier::Standard,
            referenced_function_indices: HashSet::new(),
            user_defined_function_names: HashSet::new(),
            host_class: None,
            strict_hosts: false,
            client_mode: false,
        }
    }

    /// Set the compilation target.
    pub fn set_target(&mut self, target: crate::CompilationTarget) {
        self.target = target;
    }

    /// Set the memory budget tier.
    pub fn set_memory_tier(&mut self, tier: crate::MemoryTier) {
        self.memory_tier = tier;
    }

    /// Set the host class for Plugin Contracts v2 bridge-host enforcement.
    /// See `foundation/spec/plugins/contracts/bridge-host-classes.md` §6.
    /// Pass `"server"`, `"browser"`, `"native"`, or `None` to disable the check.
    pub fn set_host_class(&mut self, host_class: Option<String>) {
        self.host_class = host_class;
    }

    /// Promote bridge-host mismatches from warnings to errors.
    /// Phase C default is false; Phase D default will be true. May also be set
    /// via the `CLEAN_STRICT_HOSTS=1` environment variable in `compile_multi_file_*`.
    pub fn set_strict_hosts(&mut self, strict: bool) {
        self.strict_hosts = strict;
    }

    /// Mark this as a browser (`frontend.wasm`) build.
    ///
    /// When enabled, DCE roots are restricted to `_start` + explicitly exported
    /// functions. User-defined functions unreachable from those roots are
    /// dead-code eliminated, preventing server-only bridge imports
    /// (`_db_query`, `_http_listen`, etc.) from leaking into the browser module.
    pub fn set_client_mode(&mut self, client_mode: bool) {
        self.client_mode = client_mode;
    }

    /// Set plugin bridge functions to be registered as WASM imports.
    ///
    /// Bridge functions are declared in `plugin.toml` `[bridge]` sections and are
    /// registered as WASM imports before code generation begins.
    pub fn set_bridge_functions(&mut self, bridge_functions: Vec<crate::plugins::BridgeFunction>) {
        for func in &bridge_functions {
            self.bridge_param_types
                .insert(func.name.clone(), func.get_param_types());
        }
        self.bridge_functions = bridge_functions;
    }

    /// Set the language-name → bridge-name mapping for dot-notation plugin calls.
    ///
    /// Obtained from `PluginRegistry::language_to_bridge_map()`. Must be called
    /// before `generate()` when the program uses language-level plugin APIs
    /// (e.g. `db.query(...)`, `req.param("id")`).
    pub fn set_language_to_bridge_map(&mut self, map: HashMap<String, String>) {
        self.language_to_bridge_map = map;
    }

    /// Return `true` when a language alias (e.g. `time.now`) is wired to a
    /// bridge target that is also declared in the plugin's `[bridge]` section.
    ///
    /// The naïve check `language_to_bridge_map.contains_key(alias)` is not
    /// enough: a plugin can list a `[[language.functions]]` entry whose
    /// `maps_to` references a bridge name that is missing from the `[bridge]`
    /// list. When that happens the bridge import is never registered (because
    /// `register_plugin_bridge_imports` iterates over `bridge_functions`, not
    /// over `language_to_bridge_map`), so call sites for that language API
    /// crash later with "Function 'X' not found in function map". Callers
    /// outside this check fall back to the compiler's built-in Layer-2
    /// registration for the namespace (e.g. `register_time_builtin_imports`)
    /// to keep the build successful in that case.
    fn alias_has_declared_bridge(&self, alias: &str) -> bool {
        let target = match self.language_to_bridge_map.get(alias) {
            Some(t) => t,
            None => return false,
        };
        self.bridge_functions.iter().any(|bf| bf.name == *target)
    }

    // -----------------------------------------------------------------------
    // Main generation entry point
    // -----------------------------------------------------------------------

    /// Generate WASM from a MIR program.
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
        let mut warnings = Vec::new();

        // Set up WASM generator with runtime imports
        if self.wasm_generator.include_runtime_imports {
            // Enable Import Minimality Rule (foundation/platform-architecture/EXECUTION_LAYERS.md):
            // filter Layer 2/3 external-I/O imports to those actually
            // reachable from the MIR call graph. Must happen BEFORE any
            // register_*_imports() / register_*_operations() call.
            let reachable = self.collect_all_called_names_from_mir(&mir_program);

            // Plugin Contracts v2 — bridge-host-class enforcement.
            // For each reachable bridge function, check that its declared
            // `hosts` includes the active build's host class. See
            // foundation/spec/plugins/contracts/bridge-host-classes.md §6.
            let host_diagnostics = self.check_bridge_host_classes(&reachable);
            if !host_diagnostics.is_empty() {
                if self.strict_hosts {
                    // Strict mode: any mismatch fails the build.
                    return Err(host_diagnostics);
                }
                // Phase C default: collect as warnings.
                for diag in &host_diagnostics {
                    tracing::warn!(
                        target: "plugin_contracts_v2",
                        diagnostic = %diag,
                        "bridge host-class mismatch (warning; pass --strict-hosts to promote to error)"
                    );
                }
                warnings.extend(host_diagnostics);
            }

            self.wasm_generator.set_reachable_imports(reachable);

            self.wasm_generator
                .register_print_imports()
                .map_err(|e| vec![e])?;

            // Console input is registered EARLY in the import sequence, so
            // skipping it shifts all subsequent import indices. Some wrapper
            // path captures a function index that expected input to be
            // present and produces invalid WASM when it isn't (COD001
            // follow-up f4030117). Keep input unconditional until the
            // shift-sensitive callers are traced.
            debug_mir!("DEBUG MIR: Registering console input imports");
            self.wasm_generator
                .register_console_imports()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering type conversion imports (int_to_string, etc.)");
            self.wasm_generator
                .register_type_conversion_imports()
                .map_err(|e| vec![e])?;

            // HTTP imports: skip functions handled by plugin expand_strings wrappers
            let skip_http_functions: HashSet<String> = self
                .bridge_functions
                .iter()
                .filter(|f| f.expand_strings && f.params.iter().any(|p| p == "string"))
                .map(|f| f.name.clone())
                .collect();
            let include_server_imports = self.target.include_server_imports();
            debug_mir!(
                "DEBUG MIR: Registering HTTP imports (skipping {} for plugin expand_strings, server_imports={})",
                skip_http_functions.len(),
                include_server_imports
            );
            self.wasm_generator
                .register_http_imports(&skip_http_functions, include_server_imports)
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering file imports");
            self.wasm_generator
                .register_file_imports()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering string.split import");
            self.wasm_generator
                .register_string_split_import()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering string_compare import");
            self.wasm_generator
                .register_string_compare_import()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering string_replace import");
            self.wasm_generator
                .register_string_replace_import()
                .map_err(|e| vec![e])?;

            if self.wasm_generator.has_reachable_prefix("string.repeat")
                || self.wasm_generator.has_reachable_prefix("string_repeat")
            {
                debug_mir!("DEBUG MIR: Registering string_repeat import (used in code)");
                self.wasm_generator
                    .register_string_repeat_import()
                    .map_err(|e| vec![e])?;
            }

            if self.wasm_generator.has_reachable_prefix("string.matches")
                || self.wasm_generator.has_reachable_prefix("string_matches")
            {
                debug_mir!("DEBUG MIR: Registering string_matches import (used in code)");
                self.wasm_generator
                    .register_string_matches_import()
                    .map_err(|e| vec![e])?;
            }

            if self
                .wasm_generator
                .has_reachable_prefix("_test_http_request")
                || self.wasm_generator.has_reachable_prefix("_test_response_")
            {
                debug_mir!("DEBUG MIR: Registering test bridge imports (endpoint tests)");
                self.wasm_generator
                    .register_test_bridge_imports()
                    .map_err(|e| vec![e])?;
            }

            // OPTIMIZATION: Only import bridge functions that are actually used in the code
            if !self.bridge_functions.is_empty() {
                debug_mir!("DEBUG MIR: Collecting used bridge function names from MIR");
                self.collect_used_function_names_from_mir(&mir_program);
                debug_mir!(
                    "DEBUG MIR: Registering {} used bridge function imports (out of {} total)",
                    self.used_bridge_function_names.len(),
                    self.bridge_functions.len()
                );
                self.register_plugin_bridge_imports().map_err(|e| vec![e])?;
            }

            // Register external functions declared via `external:` blocks
            if !mir_program.externals.is_empty() {
                debug_mir!(
                    "DEBUG MIR: Registering {} external function imports",
                    mir_program.externals.len()
                );
                self.register_external_function_imports(&mir_program.externals)
                    .map_err(|e| vec![e])?;
            }

            // Register Layer 2 host bridge builtin IMPORTS for db/env/time/crypto namespaces.
            // These register only raw WASM imports — NO local wrapper functions.
            // Wrapper functions are created later (register_db_builtin_wrappers etc.) after
            // ALL imports are registered, to prevent WASM function index corruption
            // (imports must precede local functions in the index space).
            //
            // Skipped when a plugin bridge (e.g. `frame.data`) already provides the functions.
            if !self.alias_has_declared_bridge("db.query") {
                debug_mir!("DEBUG MIR: Registering db builtin imports (no plugin bridge for db)");
                self.wasm_generator
                    .register_db_builtin_imports()
                    .map_err(|e| vec![e])?;
            }
            if !self.alias_has_declared_bridge("env.get") {
                debug_mir!("DEBUG MIR: Registering env builtin imports (no plugin bridge for env)");
                self.wasm_generator
                    .register_env_builtin_imports()
                    .map_err(|e| vec![e])?;
            }
            if !self.alias_has_declared_bridge("time.now") {
                debug_mir!(
                    "DEBUG MIR: Registering time builtin imports (no plugin bridge for time)"
                );
                self.wasm_generator
                    .register_time_builtin_imports()
                    .map_err(|e| vec![e])?;
            }
            if !self.alias_has_declared_bridge("crypto.sha256") {
                debug_mir!(
                    "DEBUG MIR: Registering crypto builtin imports (no plugin bridge for crypto)"
                );
                self.wasm_generator
                    .register_crypto_builtin_imports()
                    .map_err(|e| vec![e])?;
            }
            if !self.alias_has_declared_bridge("server.sleep") {
                debug_mir!(
                    "DEBUG MIR: Registering server_sleep import (no plugin bridge for server.sleep)"
                );
                self.wasm_generator
                    .register_server_sleep_import()
                    .map_err(|e| vec![e])?;
            }

            // State reset bridge — always registered (the reset statement may appear anywhere)
            debug_mir!("DEBUG MIR: Registering state reset imports");
            self.wasm_generator
                .register_state_reset_imports()
                .map_err(|e| vec![e])?;

            // Async host bridge imports — MUST be registered before any local (non-import)
            // functions so that WASM function indices stay consistent with binary layout.
            // These are reachability-gated: only emitted when the module uses
            // `background` / `later` statements.
            debug_mir!("DEBUG MIR: Registering async host bridge imports");
            self.wasm_generator
                .register_async_operations()
                .map_err(|e| vec![e])?;

            // Pre-register list.push_f64 as an import BEFORE any local functions
            {
                use crate::types::WasmType;
                self.wasm_generator
                    .register_import_function(
                        "env",
                        "list.push_f64",
                        &[WasmType::I32, WasmType::F64],
                        Some(WasmType::I32),
                    )
                    .map_err(|e| vec![e])?;
            }

            // Math operations pull in 16 transcendental imports plus
            // wrapper functions. Skip entirely if the module never
            // references `math.*` / `math_*` (Import Minimality Rule).
            if self.wasm_generator.has_reachable_prefix("math_")
                || self.wasm_generator.has_reachable_prefix("math.")
            {
                debug_mir!("DEBUG MIR: Registering math operation imports");
                self.wasm_generator
                    .register_math_operations()
                    .map_err(|e| vec![e])?;
            } else {
                debug_mir!("DEBUG MIR: Skipping math operations (no math.* reachable)");
            }

            debug_mir!("DEBUG MIR: Registering string class operations");
            self.wasm_generator
                .register_string_class_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering list class operations");
            self.wasm_generator
                .register_list_class_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering conditional operations");
            self.wasm_generator
                .register_conditional_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering HTTP operations");
            self.wasm_generator
                .register_http_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering file operations");
            self.wasm_generator
                .register_file_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering validator operations");
            self.wasm_generator
                .register_validator_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering native memory operations");
            self.wasm_generator
                .register_memory_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering native string trim functions");
            self.wasm_generator
                .register_string_trim_imports()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering native list operations");
            self.wasm_generator
                .register_list_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering native pairs operations");
            self.wasm_generator
                .register_pairs_operations()
                .map_err(|e| vec![e])?;

            debug_mir!("DEBUG MIR: Registering JSON operations");
            self.wasm_generator
                .register_json_operations()
                .map_err(|e| vec![e])?;

            // Register pending plugin bridge wrapper functions AFTER all imports
            if !self.pending_bridge_wrappers.is_empty() {
                debug_mir!(
                    "DEBUG MIR: Registering {} pending bridge wrapper functions",
                    self.pending_bridge_wrappers.len()
                );
                self.register_pending_bridge_wrappers()
                    .map_err(|e| vec![e])?;
            }

            // Register deferred host-mismatched bridge stubs AFTER all imports
            // for the same reason as pending_bridge_wrappers above (WASM funcidx
            // ordering — see PendingHostMismatchedStub doc).
            if !self.pending_host_mismatched_stubs.is_empty() {
                debug_mir!(
                    "DEBUG MIR: Registering {} host-mismatched bridge stubs",
                    self.pending_host_mismatched_stubs.len()
                );
                self.register_pending_host_mismatched_stubs()
                    .map_err(|e| vec![e])?;
            }

            // Layer 2 builtin namespace wrappers — created AFTER all imports so
            // WASM function indices are correct (imports before local functions).
            // The gate mirrors the import block above: emit the built-in wrapper
            // whenever the language alias is NOT served by a real bridge — either
            // because no alias was registered at all, or because the alias maps
            // to a bridge target the plugin forgot to declare.
            if !self.alias_has_declared_bridge("db.query") {
                debug_mir!("DEBUG MIR: Registering db builtin wrappers");
                self.wasm_generator
                    .register_db_builtin_wrappers()
                    .map_err(|e| vec![e])?;
            }
            if !self.alias_has_declared_bridge("env.get") {
                debug_mir!("DEBUG MIR: Registering env builtin wrappers");
                self.wasm_generator
                    .register_env_builtin_wrappers()
                    .map_err(|e| vec![e])?;
            }
            if !self.alias_has_declared_bridge("time.now") {
                debug_mir!("DEBUG MIR: Registering time builtin wrappers");
                self.wasm_generator
                    .register_time_builtin_wrappers()
                    .map_err(|e| vec![e])?;
            }
            if !self.alias_has_declared_bridge("crypto.sha256") {
                debug_mir!("DEBUG MIR: Registering crypto builtin wrappers");
                self.wasm_generator
                    .register_crypto_builtin_wrappers()
                    .map_err(|e| vec![e])?;
            }
            if !self.alias_has_declared_bridge("server.sleep") {
                debug_mir!("DEBUG MIR: Registering server_sleep wrapper");
                self.wasm_generator
                    .register_server_sleep_wrapper()
                    .map_err(|e| vec![e])?;
            }

            if self.wasm_generator.has_reachable_prefix("string.repeat")
                || self.wasm_generator.has_reachable_prefix("string_repeat")
            {
                debug_mir!("DEBUG MIR: Registering string_repeat wrapper (used in code)");
                self.wasm_generator
                    .register_string_repeat_wrapper()
                    .map_err(|e| vec![e])?;
            }
            if self.wasm_generator.has_reachable_prefix("string.matches")
                || self.wasm_generator.has_reachable_prefix("string_matches")
            {
                debug_mir!("DEBUG MIR: Registering string_matches wrapper (used in code)");
                self.wasm_generator
                    .register_string_matches_wrapper()
                    .map_err(|e| vec![e])?;
            }

            if self
                .wasm_generator
                .has_reachable_prefix("_test_http_request")
                || self.wasm_generator.has_reachable_prefix("_test_response_")
            {
                debug_mir!("DEBUG MIR: Registering test bridge wrappers (endpoint tests)");
                self.wasm_generator
                    .register_test_bridge_wrappers()
                    .map_err(|e| vec![e])?;
            }
        }

        // Set up memory section
        self.setup_memory_section().map_err(|e| vec![e])?;

        // Transfer string pool to WASM module BEFORE function generation
        self.setup_string_pool(&mir_program.string_pool)
            .map_err(|e| vec![e])?;

        // Process state variables from MirProgram.globals
        let mut sorted_globals: Vec<_> = mir_program.globals.into_iter().collect();
        sorted_globals.sort_by_key(|(symbol_id, _)| symbol_id.0);

        for (next_global_index, (symbol_id, global)) in (1_u32..).zip(sorted_globals) {
            self.state_global_indices
                .insert(symbol_id, next_global_index);
            self.state_globals.push((
                symbol_id,
                global.name.clone(),
                global.global_type.clone(),
                global.initializer.clone(),
            ));
            debug_mir!(
                name = %global.name,
                symbol_id = ?symbol_id,
                global_index = next_global_index,
                global_type = ?global.global_type,
                initializer = ?global.initializer,
                "Registered state variable as WASM global"
            );
        }
        debug_mir!(
            total_state_globals = self.state_globals.len(),
            "State variable globals registered"
        );

        // Build function symbol mapping for proper function resolution
        for (symbol_id, function) in &mir_program.functions {
            self.function_symbol_map
                .insert(*symbol_id, function.name.clone());
            self.function_signatures
                .insert(*symbol_id, function.clone());
            tracing::debug!(
                symbol_id = symbol_id.0,
                name = %function.name,
                "Mapped SymbolId to function name"
            );
        }

        // Register builtin function signatures
        self.register_builtin_function_signatures();

        // Pre-register ALL functions in function_map BEFORE generating code.
        // This ensures function B is in the map when function A calls it, even
        // if B hasn't been generated yet.
        debug_mir!("Starting function pre-registration");
        debug_mir!(
            function_count = self.wasm_generator.function_count,
            mir_functions = mir_program.functions.len(),
            "Pre-registration state"
        );

        // Initialise function_symbol_map from MirProgram's symbol_name_map
        debug_mir!(
            symbol_map_entries = mir_program.symbol_name_map.len(),
            "Initializing function_symbol_map from MirProgram"
        );
        for (symbol_id, name) in &mir_program.symbol_name_map {
            self.function_symbol_map.insert(*symbol_id, name.clone());
            debug_mir!(symbol_id = symbol_id.0, name = %name, "Symbol init");

            // Populate symbol_to_function_index for builtin functions
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

        // Safety net: if two MIR functions share a name (e.g. __redirect_0 emitted by
        // plugin preamble injection AND block expansion for the same route), the second
        // unconditional insert into function_map would overwrite the first, orphaning the
        // function body and silently dropping its WASM export (GEN002).  Retain only the
        // first occurrence of each name for plugin-generated preamble functions only.
        //
        // User-defined functions (including class constructors) share the name "constructor"
        // across multiple classes but have distinct SymbolIds and distinct WASM bodies —
        // they MUST all be kept.  Applying this dedup to user functions silently drops
        // constructors for all classes after the first, causing WASM stack underflows.
        {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            sorted_functions.retain(|(_, f)| {
                // Dedup any plugin-emitted function (v1 preamble or v2 root helper)
                // by name — multiple plugins may emit the same helper symbol.
                if f.location.file == crate::ast::PLUGIN_OUTPUT_MARKER
                    || f.location.file == crate::ast::PLUGIN_OUTPUT_V2_ROOT_MARKER
                {
                    seen.insert(f.name.clone())
                } else {
                    true
                }
            });
        }

        // Dead-code elimination for plugin preamble functions (GEN003).
        //
        // Plugin preamble functions (location.file == "<plugin-output>") are
        // generated for every import of a plugin, even when the user's code does
        // not call them. Their bodies reference bridge imports. If those imports are
        // tree-shaken (not reachable from user code), compiling the preamble function
        // would produce Call(u32::MAX) or "not found in function map".
        //
        // We skip preamble functions whose names are not in the BFS reachable set.
        // User-defined functions are always kept (they are seeded by the BFS).
        //
        // v2 (contracts/lifecycle.md §3.1): functions tagged with the v2-root
        // marker are explicit roots — never DCE'd, even when not transitively
        // reachable from user code. Their bridge imports stay too because the
        // BFS seeded them in the first place.
        if let Some(reachable) = &self.wasm_generator.reachable_imports {
            let reachable_snap = reachable.clone();
            sorted_functions.retain(|(_, f)| {
                if f.location.file == crate::ast::PLUGIN_OUTPUT_V2_ROOT_MARKER {
                    return true;
                }
                f.location.file != crate::ast::PLUGIN_OUTPUT_MARKER
                    || reachable_snap.contains(&f.name)
            });
        }

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

            self.user_defined_function_names
                .insert(function.name.clone());

            self.function_symbol_map
                .insert(*symbol_id, function.name.clone());

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

        // Generate all functions in sorted order
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
                    // Function generation failures are hard errors:
                    // phantom function indices in pre-registered map → "function index out of range"
                    return Err(vec![error]);
                }
            }
        }

        // Update function_count after all generation (must happen AFTER, because
        // pre-registered indices were based on the count before generation started)
        self.wasm_generator.function_count += stats.functions_generated as u32;
        tracing::debug!(
            functions_generated = stats.functions_generated,
            new_function_count = self.wasm_generator.function_count,
            "Updated function_count after generation"
        );

        // Handle entry point if it exists
        if let Some(entry_symbol_id) = mir_program.entry_point {
            self.generate_start_function_export(entry_symbol_id)
                .map_err(|e| vec![e])?;
        }

        // Generate _run_tests export if any test functions were compiled.
        if !mir_program.test_functions.is_empty() {
            self.generate_run_tests_export(&mir_program.test_functions)
                .map_err(|e| vec![e])?;

            // When there is no regular entry point (test-only file), generate a thin
            // _start wrapper so that clean-runner can execute the test suite.
            if mir_program.entry_point.is_none() {
                self.generate_test_start_export().map_err(|e| vec![e])?;
            }
        }

        // Finalise WASM module
        let wasm_bytes = self.finalize_module().map_err(|e| vec![e])?;

        stats.generation_time_us = start_time.elapsed().as_micros() as u64;

        Ok(MirCodegenResult {
            wasm_bytes,
            stats,
            warnings,
        })
    }
}

impl<'a> Default for MirCodeGenerator<'a> {
    fn default() -> Self {
        Self::new()
    }
}
