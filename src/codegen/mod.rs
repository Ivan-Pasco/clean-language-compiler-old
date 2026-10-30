//! Module for WebAssembly code generation.

use wasm_encoder::{CodeSection, EntityType, ExportSection, FunctionSection, ImportSection};

use crate::error::CompilerError;

use crate::types::WasmType;
use std::collections::{HashMap, HashSet};

// Declare the modules
pub mod bridge_generator;
mod codegen_module_builder;
mod codegen_registration;
pub mod const_eval;
mod memory;
pub mod mir_codegen;
pub mod native_stdlib;
mod type_manager;
pub mod validate;

#[cfg(test)]
mod tests;

// Import the StringPool struct
use self::memory::MemoryUtils;
pub use mir_codegen::{MirCodeGenerator, MirCodegenResult};
use type_manager::TypeManager;

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
/// Data-section layout offset: static data (string pool, globals) is placed
/// starting at byte 1024 in WASM linear memory.  This is *not* the runtime
/// heap start — see `native_stdlib::HEAP_START` (1 MB) for that.
///
/// Layout:
///   [0 .. 1 KB]           Reserved (null-pointer guard, stack scratch)
///   [1 KB .. ~1 MB]       Data section (string literals, static data)
///   [1 MB .. top]         Runtime heap (bump allocator, `__heap_ptr` global)
pub const DATA_SECTION_START: usize = 1024;

/// Default maximum WASM memory pages for the `standard` tier (32 MB).
/// Other tiers override this via `MemoryTier::max_pages()`.
pub const DEFAULT_MAX_MEMORY_PAGES: u64 = 512;

/// Code generator for Clean Language
pub struct CodeGenerator {
    function_section: FunctionSection,
    export_section: ExportSection,
    code_section: CodeSection,

    import_section: ImportSection,
    type_manager: TypeManager,
    memory_utils: MemoryUtils,
    function_count: u32,
    function_map: HashMap<String, u32>,
    function_names: Vec<String>,
    /// WASM return type for every registered function (imports + internal),
    /// keyed by the canonical name registered in `function_map`.
    ///
    /// Populated by `register_import_function` and the public `register_function`
    /// on `CodeGenerator`. Consumed by the MIR codegen call-destination path as
    /// a last-resort source-type for `store_to_local_with_conversion`, so that
    /// call results stored into locals of a mismatched MIR type get an
    /// `I32TruncF64S` / `F64ConvertI32S` coercion even when neither
    /// `function_signatures` nor `function_return_types` resolved a type.
    /// Without this, plugin-DSL-generated code that stores a Number-returning
    /// call into an `integer` local (or vice versa) emitted invalid WASM —
    /// see CODEGEN_F64 fingerprint `1a20405b`.
    wasm_function_return_types: HashMap<String, Option<WasmType>>,
    file_import_indices: HashMap<String, u32>,
    http_import_indices: HashMap<String, u32>,

    // String management for imports
    string_offset_counter: u32,
    string_pool: HashMap<String, u32>,

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

        Self {
            function_section: FunctionSection::new(),
            export_section: ExportSection::new(),
            code_section: CodeSection::new(),

            import_section: ImportSection::new(),
            type_manager,
            memory_utils: MemoryUtils::new(DATA_SECTION_START),
            function_count: 0,
            function_map: HashMap::new(),
            function_names: Vec::new(),
            wasm_function_return_types: HashMap::new(),
            file_import_indices: HashMap::new(),
            http_import_indices: HashMap::new(),

            // String management for imports
            string_offset_counter: 4096, // Start at 4KB to avoid conflicts
            string_pool: HashMap::new(),

            // Configuration for runtime imports
            include_runtime_imports,

            // Tree-shaking disabled by default; MirCodegen opts in.
            reachable_imports: None,

            // Track imported function names to avoid exporting them
            imported_functions: HashSet::new(),
        }
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
        // foundation/platform-architecture/EXECUTION_LAYERS.md).
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
        self.wasm_function_return_types
            .insert(field.to_string(), return_type);

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

    /// Enable reachability-based filtering of Layer 2/3 external-I/O imports.
    ///
    /// After this is set, `register_import_function` will skip any gated
    /// import (see `is_reachability_gated_import`) whose field name is not
    /// present in `reachable`.
    pub fn set_reachable_imports(&mut self, reachable: HashSet<String>) {
        self.reachable_imports = Some(reachable);
    }

    /// Add additional bridge names to the reachability set after the initial
    /// BFS scan. Bug 99fd4557f821: the BFS excludes server-only bridges found
    /// in preamble bodies, but the plain-walk in `collect_used_function_names_from_mir`
    /// correctly identifies them as reachable via retained preamble helpers.
    /// This function unions those late-discovered names into `reachable_imports`
    /// so `emit_import` (and by extension `register_http_imports`) no longer
    /// tree-shakes them.
    ///
    /// No-op when reachability has not been populated.
    pub fn extend_reachable_imports<'a, I>(&mut self, names: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        if let Some(set) = self.reachable_imports.as_mut() {
            for name in names {
                set.insert(name.to_string());
            }
        }
    }

    /// Returns true if any reachable call name starts with the given prefix.
    /// Used by stdlib class registration to decide whether to generate
    /// wrapper functions (e.g. skip HttpClass wrappers entirely when no
    /// `http_*` import is reachable — the wrappers would reference
    /// tree-shaken imports and fail to generate).
    ///
    /// When reachability has not been populated (legacy compilation path),
    /// returns true so registration behaves as before.
    pub fn has_reachable_prefix(&self, prefix: &str) -> bool {
        match &self.reachable_imports {
            Some(set) => set.iter().any(|n| n.starts_with(prefix)),
            None => true,
        }
    }
} // impl CodeGenerator (constructors and basic setup)

/// Returns true if the WASM import `field` name is a Layer 2 or Layer 3
/// external-I/O function whose emission should be gated on reachability.
///
/// Spec: foundation/platform-architecture/EXECUTION_LAYERS.md — Import Minimality Rule.
///
/// **Layer 3 (server extensions):** HTTP server routing, request context,
/// response, session, auth. A browser host cannot provide these.
///
/// **Layer 2 (host bridge):** HTTP client, file I/O, crypto, database,
/// timer. A browser host cannot provide most of these either; a CLI host
/// may not provide HTTP client etc. Per the spec:
///
/// > A **browser host** (client-only) cannot provide Layer 3 functions.
/// > A **CLI host** (no HTTP stack) cannot provide `http_get`, `http_post`.
/// > Emitting them forces non-networked hosts to stub them.
///
/// Internal runtime imports (print, math, memory, string ops, type
/// conversion, list ops, JSON) are never gated — they are called from
/// synthesized codegen paths without a 1:1 MIR call name, and are cheap
/// for any host to provide.
pub(crate) fn is_reachability_gated_import(field: &str) -> bool {
    // Layer 3 — server extensions
    if field.starts_with("_http_") || field.starts_with("_req_") || field.starts_with("_res_") {
        return true;
    }
    if field.starts_with("_session_") || field.starts_with("_auth_") {
        return true;
    }

    // Layer 2 — host bridge external I/O.
    // Gated so that a client-only program (`plugins: frame.ui` with no
    // Http/File/etc. references) produces a .wasm whose import section
    // contains zero Layer 2 I/O functions.
    if field.starts_with("http_") {
        return true;
    }
    if field.starts_with("file_") {
        return true;
    }
    if field.starts_with("_crypto_") || field.starts_with("crypto_") {
        return true;
    }
    if field.starts_with("_db_") || field.starts_with("db_") {
        return true;
    }
    // _fs_* — bytes-safe filesystem bridges (opaque handle convention,
    // see spec/type-system.md §9b). Gated so client-only builds don't
    // pay the import cost.
    if field.starts_with("_fs_") {
        return true;
    }
    if field.starts_with("_env_") || field.starts_with("env_") {
        return true;
    }
    if field.starts_with("_time_") || field.starts_with("time_") {
        return true;
    }
    if field.starts_with("_jwt_") || field.starts_with("jwt_") {
        return true;
    }

    // String primitives: only used when the program actually calls these operations.
    // The MIR call graph analysis in `collect_all_called_names_from_mir` marks
    // these reachable when:
    //   - string.concat  → any string `+` expression (SymbolId(1000) → "string.concat")
    //   - string_compare → any string `==` / `!=` expression (BinaryOp::Eq/Ne on strings)
    //   - string_replace → an explicit `string.replace()` call
    //   - string.split   → an explicit `string.split()` call
    // A minimal `print("hello")` program contains none of these, so they are
    // safely tree-shaken from the import section.
    if matches!(
        field,
        "string.concat"
            | "string_compare"
            | "string.compare"
            | "string_replace"
            | "string.replace"
            | "string.replaceAll"
            | "string.split"
    ) {
        return true;
    }

    // list.push_f64: only used when the program contains float array literals.
    // The MIR builder emits SymbolId(1005) → "list.push_f64" for each f64
    // element pushed onto a list. Safe to gate — a program with no float lists
    // never calls this import.
    if field == "list.push_f64" {
        return true;
    }

    // Async host bridge: only emitted when the program contains
    // `background` or `later` statements that lower to AsyncFireCall /
    // AsyncAwaitCall MIR operations.
    if field == "_async_fire" || field == "_async_await" {
        return true;
    }

    false
}
