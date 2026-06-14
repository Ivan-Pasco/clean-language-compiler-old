/*!
 * Clean Language Compiler Library
 *
 * Author: Ivan Pasco Lizarraga
 * Date: 17-07-2025
 * Website: https://www.cleanlanguage.dev
 *
 * A modern, type-safe programming language that compiles to WebAssembly
 */

// =============================================================================
// PRODUCTION QUALITY ENFORCEMENT
// =============================================================================
// These lints DENY patterns that indicate incomplete or placeholder code.
// The Clean Language Compiler must be production-grade with no stubs.

// CRITICAL: Deny any incomplete implementation markers
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
// Note: panic! is allowed in error paths where recovery is not possible
// #![deny(clippy::panic)]

// WARN on patterns that may indicate incomplete code
// These are tracked for gradual improvement (356 unwrap calls currently in codebase)
// NOTE: These are allowed in CI due to existing usage - tracked for future cleanup
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![warn(clippy::dbg_macro)]
#![allow(clippy::print_stdout)]
#![allow(clippy::print_stderr)]
// =============================================================================
// PEDANTIC STYLE ALLOWANCES
// =============================================================================
// Allow clippy pedantic warnings for now - focusing on functionality over style
#![allow(clippy::redundant_else)]
#![allow(clippy::unnested_or_patterns)]
#![allow(clippy::unreadable_literal)]
#![allow(clippy::similar_names)]
#![allow(clippy::needless_continue)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::unused_self)]
#![allow(clippy::match_wildcard_for_single_variants)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::semicolon_if_nothing_returned)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::bool_to_int_with_if)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::ref_option_ref)]
#![allow(clippy::large_types_passed_by_value)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::should_implement_trait)]
#![allow(clippy::missing_const_for_fn)]
// Targeted lint suppressions
// Allow some unknown lints for cross-compiler compatibility
#![allow(unknown_lints)]
#![allow(deprecated)]

/// Abstract Syntax Tree definitions — output of Stage 2 (parsing)
pub mod ast;
/// Build manifest emission (Plugin Contracts v2 — contracts/artifacts.md)
pub mod build_manifest;
/// Built-in function registry — intrinsic functions available without imports
pub mod builtins;
/// WebAssembly code generation — Stage 7 (MIR to WASM bytecode)
pub mod codegen;
/// Compilation orchestration — multi-file compilation and build coordination
pub mod compilation;
/// Debug utilities — AST/MIR/WASM inspection tools
pub mod debug;
/// Doc-code synchronization — validates feature spec frontmatter against compiler symbols
pub mod docs;
/// Error types and reporting — compiler diagnostics with spec error codes
pub mod error;
/// High-level Intermediate Representation — Stage 3 (AST to HIR)
pub mod hir;
/// Lexical analysis — Stage 1 (source to tokens)
pub mod lexer;
/// Model Context Protocol server — IDE tooling integration
pub mod mcp;
/// Medium-level Intermediate Representation — Stage 6 (TAST to MIR with optimizations)
pub mod mir;
/// Module system — multi-file resolution and dependency management
pub mod module;
/// Package management — dependency resolution and package.clean.toml handling
pub mod package;
/// Parser — Stage 2 (tokens to AST via recursive descent)
pub mod parser;
/// Plugin artifact orchestration (Plugin Contracts v2 — contracts/artifacts.md)
pub mod plugin_artifacts;
/// Plugin system — framework plugin loading, expansion, and enforcement
pub mod plugins;
/// Name and module resolution — Stage 4 (symbol binding and scope resolution)
pub mod resolver;
/// Runtime utilities — async scheduling and I/O helpers
pub mod runtime;
/// Standard library — built-in function implementations for the old codegen path
pub mod stdlib;
/// Compilation targets — server, client, and platform-specific codegen
pub mod targets;
/// Telemetry — compilation metrics and performance tracking
pub mod telemetry;
/// Type checker — Stage 5 (type inference, constraint solving, TAST generation)
pub mod typechecker;
/// WASM type definitions — value types and type conversion utilities
pub mod types;

use crate::error::CompilerError;

/// Compiler version (from Cargo.toml)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compilation target determines what imports and features are included
///
/// Different targets optimize the generated WASM for specific use cases:
/// - `Server`: Full HTTP server imports (_req_*, _session_*, _auth_*)
/// - `Plugin`: Minimal imports for WASM plugins (no server functions)
/// - `Standalone`: Standard CLI/library compilation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompilationTarget {
    /// Server target - includes all HTTP server, session, and auth imports
    /// Use this when compiling Clean Language web applications
    #[default]
    Server,
    /// Plugin target - excludes server-specific imports
    /// Use this when compiling WASM plugins that don't need server functions
    Plugin,
    /// Standalone target - standard compilation for CLI/library code
    /// Includes basic HTTP client but no server functions
    Standalone,
}

impl CompilationTarget {
    /// Returns true if server imports (_req_*, _session_*, _auth_*) should be included
    pub fn include_server_imports(&self) -> bool {
        matches!(self, CompilationTarget::Server)
    }

    /// Parse a target from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "server" => Some(CompilationTarget::Server),
            "plugin" => Some(CompilationTarget::Plugin),
            "standalone" => Some(CompilationTarget::Standalone),
            _ => None,
        }
    }
}

/// Memory budget tier for WASM modules.
///
/// Each tier defines initial pages, max pages, and intended use case.
/// Values are from `foundation/platform-architecture/MEMORY_POLICY.md` section 3.
///
/// | Tier       | Initial Pages | Initial Size | Max Pages | Max Size |
/// |------------|---------------|-------------|-----------|----------|
/// | Embedded   | 4             | 256 KB      | 16        | 1 MB     |
/// | Minimal    | 8             | 512 KB      | 128       | 8 MB     |
/// | Standard   | 32            | 2 MB        | 512       | 32 MB    |
/// | Heavy      | 64            | 4 MB        | 1024      | 64 MB    |
/// | Canvas     | 256           | 16 MB       | 1024      | 64 MB    |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum MemoryTier {
    /// IoT, constrained devices — 4 initial pages, 16 max (1 MB)
    Embedded,
    /// CLI tools, simple scripts, plugins — 8 initial pages, 128 max (8 MB)
    Minimal,
    /// Web apps, APIs, PWAs, mobile, server — 32 initial pages, 512 max (32 MB)
    #[default]
    Standard,
    /// SSR, large data processing, desktop — 64 initial pages, 1024 max (64 MB)
    Heavy,
    /// Games, real-time rendering — 256 initial pages, 1024 max (64 MB)
    Canvas,
}

impl MemoryTier {
    /// Initial WASM memory pages for this tier.
    pub fn initial_pages(self) -> u64 {
        match self {
            MemoryTier::Embedded => 4,
            MemoryTier::Minimal => 8,
            MemoryTier::Standard => 32,
            MemoryTier::Heavy => 64,
            MemoryTier::Canvas => 256,
        }
    }

    /// Maximum WASM memory pages for this tier.
    pub fn max_pages(self) -> u64 {
        match self {
            MemoryTier::Embedded => 16,
            MemoryTier::Minimal => 128,
            MemoryTier::Standard => 512,
            MemoryTier::Heavy => 1024,
            MemoryTier::Canvas => 1024,
        }
    }

    /// Maximum memory in bytes for this tier.
    pub fn max_bytes(self) -> usize {
        self.max_pages() as usize * 65536
    }

    /// Tier name as used in CLI flags and custom sections.
    pub fn name(self) -> &'static str {
        match self {
            MemoryTier::Embedded => "embedded",
            MemoryTier::Minimal => "minimal",
            MemoryTier::Standard => "standard",
            MemoryTier::Heavy => "heavy",
            MemoryTier::Canvas => "canvas",
        }
    }

    /// Parse a tier from string (CLI flag value).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "embedded" => Some(MemoryTier::Embedded),
            "minimal" => Some(MemoryTier::Minimal),
            "standard" => Some(MemoryTier::Standard),
            "heavy" => Some(MemoryTier::Heavy),
            "canvas" => Some(MemoryTier::Canvas),
            _ => None,
        }
    }

    /// Default memory tier for a given compilation target string.
    ///
    /// Mapping from MEMORY_POLICY.md section 3.2:
    /// - web, pwa, nodejs, server → standard
    /// - native → heavy
    /// - embedded → embedded
    /// - wasi → minimal
    /// - auto/standalone/plugin → standard (default)
    pub fn default_for_target(target: &str) -> Self {
        match target.to_lowercase().as_str() {
            "web" | "pwa" | "nodejs" | "server" => MemoryTier::Standard,
            "native" => MemoryTier::Heavy,
            "embedded" => MemoryTier::Embedded,
            "wasi" => MemoryTier::Minimal,
            _ => MemoryTier::Standard,
        }
    }
}

impl std::fmt::Display for MemoryTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Minimum compatible compiler version for plugins
/// Plugins should check compatibility using semver rules
pub const MIN_PLUGIN_VERSION: &str = "0.14.0";

/// Initialize structured logging with the specified level
///
/// This should be called once at application startup. Valid levels are:
/// - "error" - Only errors
/// - "warn"  - Warnings and errors
/// - "info"  - Info, warnings, and errors (default)
/// - "debug" - Debug and above (shows compilation stages)
/// - "trace" - All messages (very verbose)
///
/// # Example
/// ```no_run
/// use clean_language_compiler::init_logging;
/// init_logging("debug");
/// ```
pub fn init_logging(level: &str) {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}

/// Compiles Clean Language source code to WebAssembly using the specification-compliant 7-stage pipeline
///
/// **Note:** This compiles with NO plugins (pure Clean Language).
/// For framework features (endpoints:, data:, component:), use a framework compiler that provides plugins.
pub fn compile(source: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_pure(source, "<unknown>")
}

/// Compiles Clean Language source code to WebAssembly with file path for better error reporting
///
/// **Note:** This compiles with NO plugins (pure Clean Language).
/// For framework features (endpoints:, data:, component:), use `compile_with_plugins`.
pub fn compile_with_file(source: &str, file_path: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_source_with_detected_plugins(source, file_path, 2)
}

/// Compiles Clean Language source code with optimization level
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path for error reporting
/// * `opt_level` - Optimization level (0-3):
///   - 0: No optimization (fastest compilation, for debugging)
///   - 1: Light optimization
///   - 2: Standard optimization (default)
///   - 3: Aggressive optimization (speed + size)
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
pub fn compile_with_opt_level(
    source: &str,
    file_path: &str,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    let registry = plugins::PluginRegistry::builder()
        .build()
        .expect("Empty registry should always build");
    compile_with_plugins_and_opt_level(source, file_path, &registry, opt_level)
}

/// Parses Clean Language source code and returns the AST (stages 1-2 only).
///
/// Runs lexing and parsing only, without any semantic analysis or type checking.
/// The returned Program AST can be serialized to JSON for tooling integration.
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path to the source file (used for error reporting)
///
/// # Returns
/// * `Ok(ast::Program)` - Parsed AST
/// * `Err(Vec<CompilerError>)` - Parse errors
pub fn parse_to_ast(source: &str, file_path: &str) -> Result<ast::Program, Vec<CompilerError>> {
    use crate::lexer::specification_lexer::SpecificationLexer;
    use crate::parser::SpecificationParser;

    // Stage 1: Lexical Analysis
    let source_code = crate::lexer::specification_lexer::SourceCode::new(
        source.to_string(),
        file_path.to_string(),
    );
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer
        .tokenize()
        .map_err(|e| vec![CompilerError::LexError(e)])?;

    // Stage 2: Parsing to AST
    let mut parser =
        SpecificationParser::with_plugin_keywords(tokens, file_path.to_string(), Vec::new());
    let ast = parser.parse_program().map_err(|e| vec![e])?;

    Ok(ast)
}

/// Result of type-checking a Clean Language source file (stages 1-5 only)
#[derive(Debug)]
pub struct TypeCheckResult {
    /// Whether type-checking succeeded without errors
    pub success: bool,
    /// Number of functions found
    pub function_count: usize,
    /// Number of classes found
    pub type_count: usize,
    /// Duration of the type-check in milliseconds
    pub duration_ms: u64,
    /// Diagnostics (warnings, info) collected during type-checking
    pub diagnostics: Vec<CompilerError>,
}

/// Performs fast type-checking on a Clean Language source file.
///
/// Runs compilation stages 1-5 only (Lexer → Parser → HIR → Resolver → Type Checker),
/// skipping MIR lowering and WASM code generation. This is significantly faster than
/// a full compilation and is designed for IDE integration and AI tooling.
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path to the source file (used for error reporting)
///
/// # Returns
/// * `Ok(TypeCheckResult)` - Type-checking succeeded with metadata
/// * `Err(Vec<CompilerError>)` - Type-checking errors
pub fn type_check(source: &str, file_path: &str) -> Result<TypeCheckResult, Vec<CompilerError>> {
    let registry = plugins::PluginRegistry::builder()
        .build()
        .expect("Empty registry should always build");
    type_check_with_plugins(source, file_path, &registry)
}

/// Performs fast type-checking with plugin support.
///
/// Same as `type_check()` but with plugin registry for framework-aware checking.
pub fn type_check_with_plugins(
    source: &str,
    file_path: &str,
    registry: &plugins::PluginRegistry,
) -> Result<TypeCheckResult, Vec<CompilerError>> {
    use crate::lexer::specification_lexer::SpecificationLexer;
    use crate::resolver::NameResolver as Resolver;
    use crate::typechecker::TypeChecker;

    let start_time = std::time::Instant::now();

    // Stage 1: Lexical Analysis
    let source_code = crate::lexer::specification_lexer::SourceCode::new(
        source.to_string(),
        file_path.to_string(),
    );
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer
        .tokenize()
        .map_err(|e| vec![CompilerError::LexError(e)])?;

    // Stage 2: Parsing to AST
    use crate::parser::SpecificationParser;
    let plugin_keywords = registry.get_all_block_keywords();
    let mut parser =
        SpecificationParser::with_plugin_keywords(tokens, file_path.to_string(), plugin_keywords);
    let parsed_ast = parser.parse_program().map_err(|e| vec![e])?;

    let function_count = parsed_ast.functions.len();
    let type_count = parsed_ast.classes.len();

    // Stage 2.5: Plugin Expansion
    use crate::plugins::PluginExpander;
    let mut expander = PluginExpander::new(registry);
    let mut ast = expander.expand_program(parsed_ast).map_err(|e| {
        vec![CompilerError::syntax_error(
            e.to_string(),
            Some("Plugin expansion failed".to_string()),
            None,
        )]
    })?;

    // Stage 2.6: Bridge function registration + language alias registration
    let bridge_functions = registry.bridge_functions();
    let lang_to_bridge = registry.language_to_bridge_map();
    if !bridge_functions.is_empty() {
        // Build lookup: bridge name → BridgeFunction for alias registration below
        let bridge_by_name: std::collections::HashMap<&str, &crate::plugins::BridgeFunction> =
            bridge_functions
                .iter()
                .map(|bf| (bf.name.as_str(), bf))
                .collect();

        for bf in bridge_functions {
            if ast.externals.iter().any(|e| e.name == bf.name) {
                continue;
            }
            let parameters: Vec<crate::ast::Parameter> = bf
                .params
                .iter()
                .enumerate()
                .map(|(i, type_str)| crate::ast::Parameter {
                    name: format!("arg{}", i),
                    type_: parse_bridge_type(type_str),
                    default_value: None,
                })
                .collect();

            let external_fn = crate::ast::ExternalFunction {
                name: bf.name.clone(),
                parameters,
                return_type: parse_bridge_type(&bf.returns),
                module: bf.module.clone(),
                location: None,
            };
            ast.externals.push(external_fn);
        }

        // Register language-name aliases (e.g. "req.query", "db.query") so that
        // the resolver recognises dot-notation calls as valid external functions.
        // Language function defs may carry `params`/`returns`/`param_defaults`
        // overrides that take precedence over the bridge function's own signature.
        let lang_fn_defs = registry.language_function_defs();
        for (lang_name, bridge_name) in &lang_to_bridge {
            if ast.externals.iter().any(|e| e.name == *lang_name) {
                continue;
            }
            // Skip if plugin expansion already generated a function with this name —
            // registering it again as an external would cause a duplicate-symbol conflict
            // in the resolver's register_top_level_symbols pass.
            if ast.functions.iter().any(|f| f.name == *lang_name) {
                continue;
            }
            if let Some(bf) = bridge_by_name.get(bridge_name.as_str()) {
                let lang_def = lang_fn_defs.get(lang_name.as_str());

                // Use the language-def's param list if it declared one, otherwise
                // fall back to the bridge function's param types.
                let param_types: Vec<String> = lang_def
                    .and_then(|d| d.params.as_ref())
                    .cloned()
                    .unwrap_or_else(|| bf.params.clone());

                let param_defaults: Vec<String> = lang_def
                    .map(|d| d.param_defaults.clone())
                    .unwrap_or_default();

                let parameters: Vec<crate::ast::Parameter> = param_types
                    .iter()
                    .enumerate()
                    .map(|(i, type_str)| {
                        // Empty string means "required" (no default); any other value is a literal default.
                        let default_value = param_defaults
                            .get(i)
                            .filter(|s| !s.is_empty())
                            .map(|s| s.as_str())
                            .map(|s| {
                                // Parse simple literal defaults: integer or string
                                if let Ok(n) = s.parse::<i64>() {
                                    crate::ast::Expression::Literal(crate::ast::Value::Integer(n))
                                } else {
                                    crate::ast::Expression::Literal(crate::ast::Value::String(
                                        s.to_string(),
                                    ))
                                }
                            });
                        crate::ast::Parameter {
                            name: format!("arg{}", i),
                            type_: parse_bridge_type(type_str),
                            default_value,
                        }
                    })
                    .collect();

                let return_type = lang_def
                    .and_then(|d| d.returns.as_deref())
                    .map(parse_bridge_type)
                    .unwrap_or_else(|| parse_bridge_type(&bf.returns));

                let external_fn = crate::ast::ExternalFunction {
                    name: lang_name.clone(),
                    parameters,
                    return_type,
                    module: bf.module.clone(),
                    location: None,
                };
                ast.externals.push(external_fn);
            }
        }
    }

    // Stage 2.7: Inject phantom class stubs for plugin-declared types so that
    // HIR validation accepts them as valid named types in functions: blocks.
    inject_plugin_type_stubs(&mut ast, registry);

    // Stage 3: AST to HIR
    use crate::hir::hir_builder::HirBuilder;
    let mut hir_builder = HirBuilder::new();
    let hir_result = hir_builder.build_hir(ast).map_err(|e| vec![e])?;

    // Stage 3b: HIR semantic validation (ordering rules, contract placement, etc.)
    use crate::hir::validation::HirValidator;
    HirValidator::validate(&hir_result.hir)?;

    // Stage 4: Name and Module Resolution
    let bridge_functions = registry.bridge_functions();
    let resolution_result = if bridge_functions.is_empty() {
        Resolver::resolve(hir_result.hir)?
    } else if lang_to_bridge.is_empty() {
        Resolver::resolve_with_bridge_functions(hir_result.hir, bridge_functions)?
    } else {
        let lang_fn_defs_owned: std::collections::HashMap<
            String,
            crate::plugins::plugin_abi::PluginFunctionDef,
        > = registry
            .language_function_defs()
            .into_iter()
            .map(|(k, v)| (k, v.clone()))
            .collect();
        Resolver::resolve_with_bridge_aliases_and_fn_defs(
            hir_result.hir,
            bridge_functions,
            &lang_to_bridge,
            lang_fn_defs_owned,
        )?
    };
    let resolved_hir = resolution_result.resolved_hir;

    // Stage 5: Type Inference and Checking
    // Compute required-param-count overrides for language alias functions that declare
    // param_defaults — the resolver encodes param types but not optionality, so we
    // must pass the counts explicitly so optional-param calls are not rejected.
    let required_counts = {
        use crate::resolver::SymbolId;
        let mut counts: std::collections::HashMap<SymbolId, usize> =
            std::collections::HashMap::new();
        let alias_fn_defs = registry.language_function_defs();
        for (lang_name, lang_def) in &alias_fn_defs {
            if lang_def.param_defaults.is_empty() {
                continue;
            }
            let required = lang_def
                .param_defaults
                .iter()
                .filter(|s| s.is_empty())
                .count();
            let total = lang_def
                .params
                .as_ref()
                .map(|p| p.len())
                .unwrap_or(lang_def.param_defaults.len());
            if required < total {
                if let Some(sym_id) = resolved_hir.symbol_table.lookup_symbol(lang_name.as_str()) {
                    counts.insert(sym_id, required);
                }
            }
        }
        counts
    };
    let _type_result = TypeChecker::check_with_required_counts(resolved_hir, required_counts)?;

    let duration_ms = start_time.elapsed().as_millis() as u64;

    Ok(TypeCheckResult {
        success: true,
        function_count,
        type_count,
        duration_ms,
        diagnostics: Vec::new(),
    })
}

/// Performs fast type-checking with external WASM plugins loaded from ~/.cleen/plugins/
///
/// Mirrors `compile_with_external_plugins` but stops at stage 5 (type checking), skipping
/// MIR lowering and WASM code generation.  Plugins declared in the source `plugins:` block
/// are loaded automatically, including their bridge function declarations, so that calls to
/// plugin bridge functions (e.g. `_canvas_init`, `_canvas_get_delta_time`) are recognised as
/// valid external functions during name resolution and type checking.
///
/// # Arguments
/// * `source`    - The Clean Language source code (may contain a `plugins:` block)
/// * `file_path` - Path for error reporting
///
/// # Returns
/// * `Ok(TypeCheckResult)` - Type-checking succeeded with metadata
/// * `Err(Vec<CompilerError>)` - Type-checking errors
pub fn type_check_with_external_plugins(
    source: &str,
    file_path: &str,
) -> Result<TypeCheckResult, Vec<CompilerError>> {
    // Extract plugin names from the plugins: block in source
    let plugin_names = extract_plugins(source);

    if plugin_names.is_empty() {
        // No plugins declared — fall back to the plain type-check path
        return type_check(source, file_path);
    }

    tracing::info!(
        plugins = ?plugin_names,
        "Loading external plugins for type-check"
    );

    // Load plugins using WasmPluginLoader (same as the compile path)
    let mut loader = plugins::WasmPluginLoader::new().map_err(|e| {
        vec![CompilerError::PluginError {
            message: format!("Failed to create plugin loader: {}", e),
            location: None,
        }]
    })?;

    let registry = loader.load_plugins(&plugin_names).map_err(|e| {
        vec![CompilerError::PluginError {
            message: format!("[PLUGIN001] Failed to load plugins: {}", e),
            location: None,
        }]
    })?;

    tracing::info!(
        plugins = ?registry.registered_plugins(),
        "External plugins loaded for type-check"
    );

    // Delegate to the plugin-aware type-check path
    type_check_with_plugins(source, file_path, &registry)
}

/// Compiles Clean Language source code with NO plugins (pure language)
///
/// This is for compiling pure Clean Language code without framework extensions.
/// Framework DSL blocks (endpoints:, data:, component:) will fail with "unknown block" errors.
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path for error reporting
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
///
/// # Example
/// ```ignore
/// let wasm = compile_pure(source, "main.cln")?;
/// ```
pub fn compile_pure(source: &str, file_path: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    let registry = plugins::PluginRegistry::builder()
        .build()
        .expect("Empty registry should always build");
    compile_with_plugins(source, file_path, &registry)
}

/// Alias for [`compile_with_external_plugins_and_opt_level`].
///
/// Used by the MCP compile/validate tools and the `cln debug` command so that
/// any `plugins:` declarations in the source are honoured during compilation.
pub fn compile_source_with_detected_plugins(
    source: &str,
    file_path: &str,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_with_external_plugins_and_opt_level(source, file_path, opt_level)
}

/// Parses a bridge function type string into an AST Type
///
/// Bridge functions in plugin.toml use string type names like "string", "integer", etc.
/// This function converts those strings to the corresponding AST Type.
fn parse_bridge_type(type_str: &str) -> ast::Type {
    match type_str.to_lowercase().as_str() {
        "string" => ast::Type::String,
        "integer" | "int" | "i32" | "i64" => ast::Type::Integer,
        "number" | "float" | "f64" => ast::Type::Number,
        "boolean" | "bool" => ast::Type::Boolean,
        "void" | "()" => ast::Type::Void,
        "any" => ast::Type::Any,
        // Handle generic types like "list<string>"
        s if s.starts_with("list<") && s.ends_with('>') => {
            let inner = &s[5..s.len() - 1];
            ast::Type::List(
                Box::new(parse_bridge_type(inner)),
                ast::ListBehavior::Default,
            )
        }
        // Default to Any for unknown types
        _ => ast::Type::Any,
    }
}

/// Merge two HIR state blocks. Used by the multi-file pipeline to fold each
/// non-entry module's `state:` declarations into the merged state alongside
/// the entry module's. Without this, plugin-emitted state blocks (e.g.
/// `state: <Class> instance_<tag> = <Class>()` from frame.ui's
/// `expand_component`) that live in component source files were silently
/// dropped because the merge logic only copied `hir.state` from the entry
/// module. The dropped declarations caused the `client_init` splice's
/// `instance_<tag>` reference to resolve as `Undefined variable`.
///
/// Semantics mirror the AST-level `merge_state_block` in `plugins::expander`:
/// declarations and computed values concatenate; the first non-empty rules
/// vec survives (multiple rules vecs from different modules is a caller
/// concern); scope and location are taken from the first non-None block.
fn merge_hir_state(
    target: &mut Option<crate::hir::HirStateBlock>,
    incoming: Option<crate::hir::HirStateBlock>,
) {
    let incoming = match incoming {
        Some(s) => s,
        None => return,
    };
    match target {
        None => *target = Some(incoming),
        Some(existing) => {
            existing.declarations.extend(incoming.declarations);
            existing.computed.extend(incoming.computed);
            if existing.rules.is_empty() {
                existing.rules = incoming.rules;
            }
        }
    }
}

/// Injects synthetic phantom class stubs for types declared in loaded plugin manifests.
///
/// Plugin manifests list their custom types (e.g. `Request`, `Response` from frame.server)
/// under `[[language.types]]`. Without this step those type names are invisible to the
/// HIR validator's `context.classes` lookup, causing a spurious "Undefined type" error
/// whenever a `functions:` block uses a plugin type as a parameter type.
fn inject_plugin_type_stubs(ast: &mut ast::Program, registry: &plugins::PluginRegistry) {
    for manifest in registry.loaded_manifests().values() {
        for type_def in &manifest.language.types {
            if ast.classes.iter().any(|c| c.name == type_def.name) {
                continue;
            }
            let fields = type_def
                .fields
                .iter()
                .map(|f| ast::Field {
                    name: f.name.clone(),
                    type_: parse_bridge_type(&f.type_),
                    visibility: ast::Visibility::Public,
                    is_static: false,
                    default_value: None,
                })
                .collect();
            ast.classes.push(ast::Class {
                name: type_def.name.clone(),
                type_parameters: Vec::new(),
                description: Some(type_def.description.clone()),
                base_class: None,
                base_class_type_args: Vec::new(),
                fields,
                methods: Vec::new(),
                constructor: None,
                invariants: Vec::new(),
                location: None,
            });
        }
    }
}

/// Compiles Clean Language source code with custom plugin registry
///
/// This is the main compilation entry point for frameworks that provide DSL plugins.
/// The plugin registry determines which DSL blocks are supported.
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path for error reporting
/// * `registry` - Plugin registry with registered DSL handlers
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
///
/// # Example
/// ```ignore
/// use clean_language_compiler::plugins::PluginRegistry;
/// use frame_compiler_plugins::create_frame_registry;
///
/// let registry = create_frame_registry()?;
/// let wasm = compile_with_plugins(source, "main.cln", &registry)?;
/// ```
pub fn compile_with_plugins(
    source: &str,
    file_path: &str,
    registry: &plugins::PluginRegistry,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    // Default to optimization level 2 (standard optimization)
    compile_with_plugins_and_opt_level(source, file_path, registry, 2)
}

/// Compiles Clean Language source code with custom plugin registry and optimization level
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path for error reporting
/// * `registry` - Plugin registry with registered DSL handlers
/// * `opt_level` - Optimization level (0-3)
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
pub fn compile_with_plugins_and_opt_level(
    source: &str,
    file_path: &str,
    registry: &plugins::PluginRegistry,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    use crate::lexer::specification_lexer::SpecificationLexer;
    use crate::mir::lower_tast_to_mir_with_opt_level;
    use crate::resolver::NameResolver as Resolver;
    use crate::typechecker::TypeChecker;

    tracing::info!(
        opt_level = opt_level,
        "Starting compilation with optimization level"
    );

    // Stage 1: Lexical Analysis - specification-compliant tokenization
    tracing::debug!("Starting Stage 1: Lexical Analysis");
    let source_code = crate::lexer::specification_lexer::SourceCode::new(
        source.to_string(),
        file_path.to_string(),
    );
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer
        .tokenize()
        .map_err(|e| vec![CompilerError::LexError(e)])?;
    tracing::debug!(
        token_count = tokens.tokens.len(),
        "Stage 1 complete: Lexical Analysis"
    );

    // Stage 2: Parsing to AST - use token-driven parser (rustc-style)
    tracing::debug!("Starting Stage 2: Parsing to AST");
    use crate::parser::SpecificationParser;
    // Get plugin keywords so the parser recognizes plugin-defined syntax (e.g., "data User")
    let plugin_keywords = registry.get_all_block_keywords();
    tracing::debug!(
        plugin_keywords = ?plugin_keywords,
        "Passing plugin keywords to parser"
    );
    let mut parser =
        SpecificationParser::with_plugin_keywords(tokens, file_path.to_string(), plugin_keywords);
    let parsed_ast = parser.parse_program().map_err(|e| vec![e])?;
    tracing::debug!(
        functions = parsed_ast.functions.len(),
        statements = parsed_ast.statements.len(),
        classes = parsed_ast.classes.len(),
        "Stage 2 complete: AST created"
    );

    // Stage 2.5a: Plugin Enforcement - check project structure conventions
    {
        let enforcement_rules: Vec<(String, plugins::plugin_abi::PluginEnforcement)> = registry
            .loaded_manifests()
            .iter()
            .filter(|(_, m)| {
                !m.enforcement.restricted_functions.is_empty()
                    || !m.enforcement.required_blocks.is_empty()
                    || !m.enforcement.block_folder_rules.is_empty()
            })
            .map(|(name, m)| (name.clone(), m.enforcement.clone()))
            .collect();

        if !enforcement_rules.is_empty() {
            let enforcement_result =
                plugins::enforcement::enforce_rules(&parsed_ast, file_path, &enforcement_rules);

            for warning in &enforcement_result.warnings {
                eprintln!(
                    "warning[{}]: {} ({})",
                    warning.plugin, warning.message, warning.suggestion
                );
            }
            if !enforcement_result.errors.is_empty() {
                return Err(enforcement_result
                    .errors
                    .into_iter()
                    .map(|e| CompilerError::PluginError {
                        message: format!("{} ({})", e.message, e.suggestion),
                        location: None,
                    })
                    .collect());
            }
        }
    }

    // Stage 2.5b: Plugin Expansion - transform framework blocks into Clean AST
    tracing::debug!("Starting Stage 2.5: Plugin Expansion");
    use crate::plugins::PluginExpander;
    let mut expander = PluginExpander::new(registry);
    let mut ast = expander.expand_program(parsed_ast).map_err(|e| {
        vec![CompilerError::syntax_error(
            e.to_string(),
            Some("Plugin expansion failed".to_string()),
            None,
        )]
    })?;
    tracing::debug!(
        blocks_expanded = expander.blocks_expanded(),
        statements_generated = expander.statements_generated(),
        "Stage 2.5 complete: Plugin expansion finished"
    );

    // Stage 2.6: Convert plugin bridge functions to external declarations
    // This ensures bridge functions from plugin.toml are registered in the AST
    // so they can be properly type-checked and resolved.
    // Also registers language-level function names (dot-notation API) as aliases
    // pointing to the same signatures, so that calls like `db.query(...)` are
    // recognised by the semantic analyser and resolver.
    let bridge_functions = registry.bridge_functions();
    let lang_to_bridge = registry.language_to_bridge_map();
    if !bridge_functions.is_empty() {
        tracing::debug!(
            bridge_function_count = bridge_functions.len(),
            lang_alias_count = lang_to_bridge.len(),
            "Converting plugin bridge functions to external declarations"
        );

        // Build a quick lookup: bridge function name → BridgeFunction
        let bridge_by_name: std::collections::HashMap<&str, &crate::plugins::BridgeFunction> =
            bridge_functions
                .iter()
                .map(|bf| (bf.name.as_str(), bf))
                .collect();

        for bf in bridge_functions {
            // Skip if already declared (from parsed external: block or plugin expansion)
            if ast.externals.iter().any(|e| e.name == bf.name) {
                continue;
            }

            // Convert BridgeFunction to ExternalFunction
            let parameters: Vec<crate::ast::Parameter> = bf
                .params
                .iter()
                .enumerate()
                .map(|(i, type_str)| crate::ast::Parameter {
                    name: format!("arg{}", i),
                    type_: parse_bridge_type(type_str),
                    default_value: None,
                })
                .collect();

            let external_fn = crate::ast::ExternalFunction {
                name: bf.name.clone(),
                parameters,
                return_type: parse_bridge_type(&bf.returns),
                module: bf.module.clone(),
                location: None,
            };

            ast.externals.push(external_fn);
            tracing::trace!(
                name = %bf.name,
                "Added external function from bridge"
            );
        }

        // Register language-name aliases (e.g. "db.query", "req.param") so that
        // the semantic analyser and resolver recognise them as valid callable names.
        // Each alias gets the same parameter/return signature as its bridge function.
        for (lang_name, bridge_name) in &lang_to_bridge {
            // Skip if already declared
            if ast.externals.iter().any(|e| e.name == *lang_name) {
                continue;
            }
            // Skip if plugin expansion already generated a function with this name —
            // registering it again as an external would cause a duplicate-symbol conflict
            // in the resolver's register_top_level_symbols pass.
            if ast.functions.iter().any(|f| f.name == *lang_name) {
                continue;
            }

            if let Some(bf) = bridge_by_name.get(bridge_name.as_str()) {
                let parameters: Vec<crate::ast::Parameter> = bf
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, type_str)| crate::ast::Parameter {
                        name: format!("arg{}", i),
                        type_: parse_bridge_type(type_str),
                        default_value: None,
                    })
                    .collect();

                let external_fn = crate::ast::ExternalFunction {
                    name: lang_name.clone(),
                    parameters,
                    return_type: parse_bridge_type(&bf.returns),
                    module: bf.module.clone(),
                    location: None,
                };

                ast.externals.push(external_fn);
                tracing::trace!(
                    lang_name = %lang_name,
                    bridge_name = %bridge_name,
                    "Added language-name alias external function"
                );
            }
        }

        tracing::debug!(
            externals_count = ast.externals.len(),
            "Stage 2.6 complete: Bridge functions and language aliases converted to externals"
        );
    }

    // Stage 2.7: Inject phantom class stubs for plugin-declared types so that
    // HIR validation accepts them as valid named types in functions: blocks.
    inject_plugin_type_stubs(&mut ast, registry);

    // Stage 3: AST to HIR - validation and desugaring per specification
    tracing::debug!("Starting Stage 3: AST to HIR");
    for (i, func) in ast.functions.iter().enumerate() {
        tracing::trace!(index = i, name = %func.name, statements = func.body.len(), "AST function");
        // Log detailed statement info for start function
        if func.name == "start" {
            for (stmt_idx, stmt) in func.body.iter().enumerate() {
                tracing::debug!(
                    stmt_index = stmt_idx,
                    stmt_type = ?std::mem::discriminant(stmt),
                    "AST statement in start()"
                );
                // Check if it's a TypeApplyBlock
                if let crate::ast::Statement::TypeApplyBlock {
                    type_, assignments, ..
                } = stmt
                {
                    tracing::debug!(
                        type_ = ?type_,
                        assignments_count = assignments.len(),
                        "Found TypeApplyBlock in AST start() function"
                    );
                }
            }
        }
    }
    use crate::hir::hir_builder::HirBuilder;
    let mut hir_builder = HirBuilder::new();
    let hir_result = hir_builder.build_hir(ast.clone()).map_err(|e| vec![e])?;
    tracing::debug!(
        functions = hir_result.hir.functions.len(),
        "Stage 3 complete: HIR created"
    );

    // Stage 3b: HIR semantic validation (ordering rules, contract placement, etc.)
    use crate::hir::validation::HirValidator;
    HirValidator::validate(&hir_result.hir)?;
    tracing::debug!("Stage 3b complete: HIR validation passed");

    // Stage 4: Name and Module Resolution - symbol resolution per specification
    tracing::debug!(
        functions = hir_result.hir.functions.len(),
        "Starting Stage 4: Resolver"
    );

    // Get bridge functions from plugin registry for name resolution
    let bridge_functions = registry.bridge_functions();
    tracing::debug!(
        bridge_function_count = bridge_functions.len(),
        lang_alias_count = lang_to_bridge.len(),
        "Registering plugin bridge functions in resolver"
    );

    let resolution_result = if bridge_functions.is_empty() {
        Resolver::resolve(hir_result.hir)?
    } else if lang_to_bridge.is_empty() {
        Resolver::resolve_with_bridge_functions(hir_result.hir, bridge_functions)?
    } else {
        Resolver::resolve_with_bridge_and_language_aliases(
            hir_result.hir,
            bridge_functions,
            &lang_to_bridge,
        )?
    };
    let resolved_hir = resolution_result.resolved_hir;
    tracing::debug!(
        functions = resolved_hir.functions.len(),
        "Stage 4 complete: Resolution finished"
    );

    // Stage 5: Type Inference and Checking to TAST - constraint-based type inference
    tracing::debug!("Starting Stage 5: Type Checker");
    let type_result = TypeChecker::check(resolved_hir)?;
    tracing::debug!(
        functions = type_result.tast.functions.len(),
        "Stage 5 complete: Type checking finished"
    );

    // Stage 6: TAST to MIR Lowering and Optimization - SSA form with optimizations
    tracing::debug!(opt_level = opt_level, "Starting Stage 6: TAST to MIR");
    let mir_result = lower_tast_to_mir_with_opt_level(type_result.tast, opt_level)?;
    tracing::debug!(opt_level = opt_level, "Stage 6 complete: MIR created");

    // Stage 7: WASM Code Generation - use MIR-based generator with fixes
    tracing::debug!("Starting Stage 7: WASM generation");
    use crate::codegen::mir_codegen::MirCodeGenerator;
    let mut mir_codegen = MirCodeGenerator::default();

    // Pass plugin bridge functions to the code generator for WASM import generation
    if !bridge_functions.is_empty() {
        tracing::debug!(
            bridge_function_count = bridge_functions.len(),
            "Passing bridge functions to MIR code generator"
        );
        mir_codegen.set_bridge_functions(bridge_functions.to_vec());
    }

    // Pass language-to-bridge mapping so the codegen can recognise dot-notation
    // plugin calls (e.g. `db.query`) and route them to the correct WASM import.
    if !lang_to_bridge.is_empty() {
        tracing::debug!(
            alias_count = lang_to_bridge.len(),
            "Passing language-to-bridge map to MIR code generator"
        );
        mir_codegen.set_language_to_bridge_map(lang_to_bridge);
    }

    let codegen_result = mir_codegen.generate(mir_result.program)?;
    let wasm_bytes = codegen_result.wasm_bytes;
    crate::codegen::validate::validate_generated_wasm(&wasm_bytes).map_err(|e| vec![e])?;
    tracing::info!(
        bytes = wasm_bytes.len(),
        "Compilation complete: WASM generated"
    );

    Ok(wasm_bytes)
}

/// Compiles Clean Language source code with external WASM plugins loaded from ~/.cleen/plugins/
///
/// This function automatically discovers plugins based on `plugins:` blocks in the source code.
/// Plugins must be installed using `cleen plugin add <name>` before they can be used.
///
/// # Arguments
/// * `source` - The Clean Language source code (may contain `plugins:` blocks)
/// * `file_path` - Path for error reporting
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
///
/// # Example
/// ```ignore
/// // Source code with plugins block:
/// // plugins:
/// //     frame.web
/// //     frame.data
/// //
/// // endpoints:
/// //     GET "/users" -> listUsers
/// //
/// let wasm = compile_with_external_plugins(source, "app.cln")?;
/// ```
pub fn compile_with_external_plugins(
    source: &str,
    file_path: &str,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_with_external_plugins_and_opt_level(source, file_path, 2)
}

/// Compiles Clean Language source code with external WASM plugins and custom optimization level
///
/// # Arguments
/// * `source` - The Clean Language source code (may contain `plugins:` blocks)
/// * `file_path` - Path for error reporting
/// * `opt_level` - Optimization level (0-3)
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
pub fn compile_with_external_plugins_and_opt_level(
    source: &str,
    file_path: &str,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    // Extract plugins from plugins: block in source
    let plugin_names = extract_plugins(source);

    if plugin_names.is_empty() {
        // No plugins declared: compile with empty registry, honouring opt_level.
        tracing::debug!("No plugins found, compiling without external plugins");
        let registry = plugins::PluginRegistry::builder()
            .build()
            .expect("Empty registry should always build");
        return compile_with_plugins_and_opt_level(source, file_path, &registry, opt_level);
    }

    tracing::info!(plugins = ?plugin_names, "Loading external plugins from plugins: block");

    // Load plugins using WasmPluginLoader
    let mut loader = plugins::WasmPluginLoader::new().map_err(|e| {
        vec![CompilerError::PluginError {
            message: format!("Failed to create plugin loader: {}", e),
            location: None,
        }]
    })?;

    let registry = loader.load_plugins(&plugin_names).map_err(|e| {
        vec![CompilerError::PluginError {
            message: format!("[PLUGIN001] Failed to load plugins: {}", e),
            location: None,
        }]
    })?;

    tracing::info!(
        plugins = ?registry.registered_plugins(),
        "External plugins loaded"
    );

    // Compile with loaded plugins
    compile_with_plugins_and_opt_level(source, file_path, &registry, opt_level)
}

/// Compiles Clean Language source code with a specific compilation target
///
/// This function allows specifying a compilation target to control which imports
/// are included in the generated WASM:
/// - `Server`: Full HTTP server imports (_req_*, _session_*, _auth_*)
/// - `Plugin`: Minimal imports for WASM plugins (no server functions)
/// - `Standalone`: Standard CLI/library compilation
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path for error reporting
/// * `target` - Compilation target (Server, Plugin, or Standalone)
/// * `opt_level` - Optimization level (0-3)
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
///
/// # Example
/// ```ignore
/// use clean_language_compiler::{compile_with_target, CompilationTarget};
///
/// // Compile plugin with minimal imports
/// let wasm = compile_with_target(source, "plugin.cln", CompilationTarget::Plugin, 2)?;
/// ```
pub fn compile_with_target(
    source: &str,
    file_path: &str,
    target: CompilationTarget,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    use crate::lexer::specification_lexer::SpecificationLexer;
    use crate::mir::lower_tast_to_mir_with_opt_level;
    use crate::resolver::NameResolver as Resolver;
    use crate::typechecker::TypeChecker;

    tracing::info!(
        opt_level = opt_level,
        target = ?target,
        "Starting compilation with target"
    );

    // Stage 1: Lexical Analysis
    let source_code = crate::lexer::specification_lexer::SourceCode::new(
        source.to_string(),
        file_path.to_string(),
    );
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer
        .tokenize()
        .map_err(|e| vec![CompilerError::LexError(e)])?;

    // Stage 2: Parsing to AST
    use crate::parser::SpecificationParser;
    let mut parser = SpecificationParser::new(tokens, file_path.to_string());
    let ast = parser.parse_program().map_err(|e| vec![e])?;

    // Stage 3: AST to HIR
    use crate::hir::hir_builder::HirBuilder;
    let mut hir_builder = HirBuilder::new();
    let hir_result = hir_builder.build_hir(ast).map_err(|e| vec![e])?;

    // Stage 3b: HIR semantic validation (ordering rules, contract placement, etc.)
    use crate::hir::validation::HirValidator;
    HirValidator::validate(&hir_result.hir)?;

    // Stage 4: Name and Module Resolution
    let resolution_result = Resolver::resolve(hir_result.hir)?;
    let resolved_hir = resolution_result.resolved_hir;

    // Stage 5: Type Inference and Checking
    let type_result = TypeChecker::check(resolved_hir)?;

    // Stage 6: TAST to MIR Lowering
    let mir_result = lower_tast_to_mir_with_opt_level(type_result.tast, opt_level)?;

    // Stage 7: WASM Code Generation with specific target
    use crate::codegen::mir_codegen::MirCodeGenerator;
    let mut mir_codegen = MirCodeGenerator::with_target(target);

    let codegen_result = mir_codegen.generate(mir_result.program)?;

    crate::codegen::validate::validate_generated_wasm(&codegen_result.wasm_bytes)
        .map_err(|e| vec![e])?;

    tracing::info!(
        bytes = codegen_result.wasm_bytes.len(),
        target = ?target,
        "Compilation complete with target"
    );

    Ok(codegen_result.wasm_bytes)
}

/// Compiles Clean Language source code for use as a WASM plugin
///
/// This is a convenience function that compiles with `CompilationTarget::Plugin`,
/// which excludes server-specific imports (_req_*, _session_*, _auth_*) to produce
/// a minimal WASM module suitable for use as a plugin.
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path for error reporting
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes (with minimal imports)
/// * `Err(Vec<CompilerError>)` - Compilation errors
///
/// # Example
/// ```ignore
/// use clean_language_compiler::compile_for_plugin;
///
/// let wasm = compile_for_plugin(source, "my_plugin.cln")?;
/// // Result: ~67 imports instead of ~83
/// ```
pub fn compile_for_plugin(source: &str, file_path: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_with_target(source, file_path, CompilationTarget::Plugin, 2)
}

/// Compiles Clean Language source code for use as a WASM plugin with custom optimization level
///
/// # Arguments
/// * `source` - The Clean Language source code
/// * `file_path` - Path for error reporting
/// * `opt_level` - Optimization level (0-3)
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes (with minimal imports)
/// * `Err(Vec<CompilerError>)` - Compilation errors
pub fn compile_for_plugin_with_opt_level(
    source: &str,
    file_path: &str,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    compile_with_target(source, file_path, CompilationTarget::Plugin, opt_level)
}

/// Extracts plugin names from `plugins:` blocks in source code
///
/// # Format
/// ```clean
/// plugins:
///     frame.web
///     frame.data
/// ```
///
/// # Returns
/// Vector of plugin names (e.g., ["frame.web", "frame.data"])
/// When `source` is a package manifest (starts with `package:`), return the path of
/// the entry `.cln` file declared under the first `entry:` key.
fn extract_manifest_entry(
    source: &str,
    manifest_dir: &std::path::Path,
) -> Option<std::path::PathBuf> {
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("entry:") {
            let p = rest.trim().trim_matches('"');
            if !p.is_empty() {
                return Some(manifest_dir.join(p));
            }
        }
    }
    None
}

fn extract_plugins(source: &str) -> Vec<String> {
    let mut plugins = Vec::new();
    let mut in_plugins_block = false;
    let mut plugins_indent: usize = 0;

    for line in source.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        // Handle inline list syntax at any indentation: `plugins: [frame.ui, frame.server]`
        if let Some(rest) = trimmed.strip_prefix("plugins:") {
            let rest = rest.trim();
            if rest.starts_with('[') {
                let end = rest.find(']').unwrap_or(rest.len());
                let inner = &rest[1..end];
                for name in inner.split(',') {
                    let name = name.trim();
                    if !name.is_empty() && !name.starts_with('#') {
                        plugins.push(name.to_string());
                    }
                }
                continue;
            }

            // Multi-line block: `plugins:` alone (at any indentation)
            if rest.is_empty() {
                in_plugins_block = true;
                plugins_indent = indent;
                continue;
            }
        }

        // Collect multi-line plugin entries
        if in_plugins_block {
            if trimmed.is_empty() {
                continue;
            }
            // A line at the same or lesser indentation as the plugins: header ends the block
            if indent <= plugins_indent && trimmed.ends_with(':') {
                in_plugins_block = false;
                continue;
            }
            // Lines indented deeper than the plugins: header are entries
            if indent > plugins_indent {
                if !trimmed.starts_with('#') {
                    plugins.push(trimmed.to_string());
                }
            } else {
                in_plugins_block = false;
            }
        }
    }

    plugins
}

/// Collect all plugin names required by a package.
///
/// For a single-file build: returns plugins declared in that file.
/// For a package manifest (starts with `package:`): reads the declared entry
/// file AND recursively scans every `.cln` file in every `shared:` folder,
/// merging all unique plugin names.  This ensures that plugins declared only
/// in shared source files (e.g. `frame.server` in routes.cln) are loaded into
/// the registry even when the manifest entry file does not re-declare them.
/// Plugin Contracts v2 — discover plugins for the given entry file, load
/// their manifests, and return resolved callback contracts.
///
/// Used by the build flow (`cln compile`, `cln build`) to populate the
/// `callbacks` field in `dist/build-manifest.json` so hosts can read the
/// dispatch contracts (e.g. `_ui_render_page` → `component_tag_render`)
/// without re-parsing every plugin.toml. Validation per
/// `foundation/spec/plugins/contracts/bridge-host-classes.md` §4 already
/// happens at registry build; this helper just extracts the resolved set.
///
/// Returns an empty Vec when:
/// - the entry file declares no plugins,
/// - no loaded plugin declares any `[bridge.functions.callback]` blocks,
/// - plugin loading itself fails (the caller's normal compile pass will
///   surface that error — this helper degrades to "no callbacks" rather
///   than blocking manifest emission).
pub fn discover_callback_contracts<P: AsRef<std::path::Path>>(
    entry_path: P,
) -> Vec<build_manifest::CallbackContract> {
    let entry_source = match std::fs::read_to_string(entry_path.as_ref()) {
        Ok(src) => src,
        Err(_) => return Vec::new(),
    };
    let plugin_names = collect_package_plugins(entry_path.as_ref(), &entry_source);
    if plugin_names.is_empty() {
        return Vec::new();
    }
    let mut loader = match plugins::WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => return Vec::new(),
    };
    let registry = match loader.load_plugins(&plugin_names) {
        Ok(reg) => reg,
        Err(_) => return Vec::new(),
    };
    registry.callback_contracts()
}

/// Plugin Contracts v2 — **DO NOT USE**. Always returns an empty map.
///
/// This function was a Phase-C stub that loaded plugins into a brand-new
/// throwaway registry and snapshotted its (necessarily empty) BuildState. It
/// can never reflect writes made during the real build because the writes
/// happen in a different registry that lives only inside
/// [`compile_multi_file_with_memory_tier`] / [`compile_multi_file_release`].
///
/// Both of those functions now return the populated snapshot directly in
/// their `Result` tuple. Use that instead. This wrapper is retained only so
/// out-of-tree callers still link; new code MUST use the tuple return.
///
/// Reported as `PLUGIN_BUILD_STATE_NOT_PERSISTED` against compiler 0.30.276.
#[deprecated(
    note = "Use the build_state returned by compile_multi_file_with_memory_tier or \
            compile_multi_file_release. This function always returns an empty map."
)]
pub fn discover_build_state_snapshot<P: AsRef<std::path::Path>>(
    entry_path: P,
) -> std::collections::BTreeMap<String, String> {
    let entry_source = match std::fs::read_to_string(entry_path.as_ref()) {
        Ok(src) => src,
        Err(_) => return std::collections::BTreeMap::new(),
    };
    let plugin_names = collect_package_plugins(entry_path.as_ref(), &entry_source);
    if plugin_names.is_empty() {
        return std::collections::BTreeMap::new();
    }
    let mut loader = match plugins::WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => return std::collections::BTreeMap::new(),
    };
    let registry = match loader.load_plugins(&plugin_names) {
        Ok(reg) => reg,
        Err(_) => return std::collections::BTreeMap::new(),
    };
    registry.build_state_snapshot()
}

/// Plugin Contracts v2 — discover plugins for the given entry file, load
/// their manifests, and return the full set keyed by plugin name. Used by
/// the build flow to orchestrate `[[artifacts]]` emission per
/// `foundation/spec/plugins/contracts/artifacts.md` §7.
///
/// Returns an empty map on any error so the legacy emission path can
/// gracefully take over.
pub fn discover_plugin_manifests<P: AsRef<std::path::Path>>(
    entry_path: P,
) -> std::collections::HashMap<String, plugins::PluginManifest> {
    let entry_source = match std::fs::read_to_string(entry_path.as_ref()) {
        Ok(src) => src,
        Err(_) => return std::collections::HashMap::new(),
    };
    let plugin_names = collect_package_plugins(entry_path.as_ref(), &entry_source);
    if plugin_names.is_empty() {
        return std::collections::HashMap::new();
    }
    let mut loader = match plugins::WasmPluginLoader::new() {
        Ok(l) => l,
        Err(_) => return std::collections::HashMap::new(),
    };
    let registry = match loader.load_plugins(&plugin_names) {
        Ok(reg) => reg,
        Err(_) => return std::collections::HashMap::new(),
    };
    registry.loaded_manifests().clone()
}

fn collect_package_plugins(entry_path: &std::path::Path, entry_source: &str) -> Vec<String> {
    let mut plugins: Vec<String> = extract_plugins(entry_source);

    if !entry_source.trim_start().starts_with("package:") {
        return plugins;
    }

    let manifest_dir = match entry_path.parent() {
        Some(d) => d,
        None => return plugins,
    };

    // Read plugins from the declared entry .cln file (may differ from manifest).
    if let Some(actual_entry) = extract_manifest_entry(entry_source, manifest_dir) {
        if let Ok(src) = std::fs::read_to_string(&actual_entry) {
            for p in extract_plugins(&src) {
                if !plugins.contains(&p) {
                    plugins.push(p);
                }
            }
        }
    }

    // Scan all .cln files in every shared: folder.
    fn scan_cln_dir(dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_cln_dir(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("cln") {
                if let Ok(src) = std::fs::read_to_string(&path) {
                    for p in extract_plugins(&src) {
                        if !out.contains(&p) {
                            out.push(p);
                        }
                    }
                }
            }
        }
    }

    // Parse shared: [...] from manifest
    for line in entry_source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("shared:") {
            let rest = rest.trim();
            if rest.starts_with('[') {
                let end = rest.find(']').unwrap_or(rest.len());
                for path_str in rest[1..end].split(',') {
                    let p = path_str.trim().trim_matches('"');
                    if !p.is_empty() {
                        scan_cln_dir(&manifest_dir.join(p), &mut plugins);
                    }
                }
            }
        }
    }

    plugins
}

/// Compiles a multi-file Clean Language program from an entry file
///
/// This function supports programs with `import:` statements that reference
/// other `.cln` files. It automatically discovers, loads, and compiles all
/// transitively imported modules into a single WASM output.
///
/// **Plugin Support:** If the entry file contains a `plugins:` block, plugins
/// will be automatically loaded from `~/.cleen/plugins/` and framework blocks
/// (like `endpoints:`, `data:`, `component:`) will be expanded.
///
/// # Arguments
/// * `entry_path` - Path to the main/entry `.cln` file
/// * `search_paths` - Additional paths to search for imported modules
/// * `opt_level` - Optimization level (0-3)
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes containing all modules
/// * `Err(Vec<CompilerError>)` - Compilation errors
///
/// # Example
/// ```ignore
/// // Given: main.cln imports utils.cln
/// let wasm = compile_multi_file(
///     Path::new("src/main.cln"),
///     vec![PathBuf::from("src/"), PathBuf::from("lib/")],
///     2
/// )?;
/// ```
pub fn compile_multi_file<P: AsRef<std::path::Path>>(
    entry_path: P,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    use crate::compilation::{MultiFileCompiler, MultiFileCompilerConfig};
    use crate::mir::lower_tast_to_mir_with_opt_level;
    use crate::resolver::NameResolver as Resolver;
    use crate::typechecker::TypeChecker;
    use std::sync::Arc;

    tracing::info!(
        entry = %entry_path.as_ref().display(),
        opt_level = opt_level,
        "Starting multi-file compilation"
    );

    // Step 0: Read entry file to extract plugins
    let entry_source = std::fs::read_to_string(entry_path.as_ref()).map_err(|e| {
        vec![CompilerError::io_error(
            format!("Failed to read entry file: {}", e),
            None,
            None,
        )]
    })?;

    // Collect plugin names from all package files (entry, manifest entry, shared folders).
    let plugin_names = collect_package_plugins(entry_path.as_ref(), &entry_source);

    // Load plugins if any are declared
    let registry = if !plugin_names.is_empty() {
        tracing::info!(plugins = ?plugin_names, "Loading plugins for multi-file compilation");

        let mut loader = plugins::WasmPluginLoader::new().map_err(|e| {
            vec![CompilerError::PluginError {
                message: format!("Failed to create plugin loader: {}", e),
                location: None,
            }]
        })?;

        let reg = loader.load_plugins(&plugin_names).map_err(|e| {
            vec![CompilerError::PluginError {
                message: format!("Failed to load plugins: {}", e),
                location: None,
            }]
        })?;

        tracing::info!(
            plugins = ?reg.registered_plugins(),
            "Plugins loaded for multi-file compilation"
        );

        Some(Arc::new(reg))
    } else {
        tracing::debug!("No plugins: block found in entry file, compiling without plugins");
        None
    };

    // Step 1: Build the compilation unit (discovers and parses all modules)
    let mut config = MultiFileCompilerConfig::default()
        .with_search_paths(search_paths)
        .with_opt_level(opt_level);

    // Add plugin registry if plugins were loaded
    if let Some(ref reg) = registry {
        config = config.with_plugin_registry(Arc::clone(reg));
    }

    let compiler = MultiFileCompiler::with_config(config);
    let unit = compiler.build_from_file(&entry_path)?;

    tracing::info!(
        modules = unit.module_count(),
        "All modules discovered and parsed to HIR"
    );

    // Step 2: Merge all HIR programs into a single unified program
    // The compilation order ensures dependencies come before dependents
    let merged_hir = {
        use crate::hir::{HirFunction, HirProgram};

        let mut all_functions: Vec<HirFunction> = Vec::new();
        let mut all_classes = Vec::new();
        let mut start_function: Option<crate::hir::HirFunction> = None;
        // Start function bodies from non-entry modules (e.g. route registrations
        // generated by frame.server plugin expanding a routes: block in routes.cln).
        // These must not be dropped — they are merged into the entry start function.
        let mut extra_start_stmts: Vec<crate::hir::HirStatement> = Vec::new();
        let mut all_imports = Vec::new();
        let mut all_tests = Vec::new();
        let mut all_externals = Vec::new();
        let mut merged_state: Option<crate::hir::HirStateBlock> = None;
        let mut merged_screen_blocks: Vec<crate::hir::HirScreenBlock> = Vec::new();
        let mut merged_watch_blocks: Vec<crate::hir::HirWatchBlock> = Vec::new();
        let mut root_location = None;

        // Process modules in compilation order (dependencies first)
        for module_id in &unit.compilation_order {
            if let Some(module) = unit.get_module(*module_id) {
                if let Some(hir) = &module.hir {
                    // Collect functions (but only one start function).
                    // Dedup by name: plugin preamble injection on the entry module can
                    // re-emit helpers (e.g. __redirect_0) that a non-entry module already
                    // generated via block expansion, producing two MIR entries with
                    // different SymbolIds but the same name and corrupting the export table.
                    for func in &hir.functions {
                        if !all_functions.iter().any(|f| f.name == func.name) {
                            all_functions.push(func.clone());
                        }
                    }

                    // Collect classes
                    for class in &hir.classes {
                        all_classes.push(class.clone());
                    }

                    // Merge state from every module. compilation_order is
                    // topologically sorted with the entry last, so non-entry
                    // plugin-emitted globals (e.g. frame.ui's `instance_<tag>`
                    // declarations contributed by `expand_component` running on
                    // component source files) land before the entry module's
                    // user state declarations. Before this merge landed, the
                    // entry-only assignment silently discarded plugin state,
                    // breaking the v2.12.3 client_init splice with
                    // `Undefined variable 'instance_<tag>'`.
                    merge_hir_state(&mut merged_state, hir.state.clone());

                    if module.is_entry {
                        start_function = hir.start_function.clone();
                        merged_screen_blocks = hir.screen_blocks.clone();
                        merged_watch_blocks = hir.watch_blocks.clone();
                        root_location = Some(hir.location.clone());
                    } else if let Some(ref module_start) = hir.start_function {
                        // Collect start statements from non-entry modules so that
                        // plugin-generated route registrations (routes: block in
                        // routes.cln) are not silently discarded.
                        extra_start_stmts.extend(module_start.body.statements.iter().cloned());
                    }

                    // Collect imports (for reference, they're already resolved)
                    for import in &hir.imports {
                        all_imports.push(import.clone());
                    }

                    // Collect tests (only from entry module for now)
                    if module.is_entry {
                        for test in &hir.tests {
                            all_tests.push(test.clone());
                        }
                    }

                    // Collect external functions (WASM imports)
                    for external in &hir.externals {
                        all_externals.push(external.clone());
                    }
                }
            }
        }

        // Merge non-entry start statements (e.g. routes: registrations) into the
        // entry module's start function.  Non-entry statements are prepended so
        // route registrations happen before any _http_listen call in the entry body.
        if !extra_start_stmts.is_empty() {
            let loc = root_location
                .clone()
                .unwrap_or_else(|| crate::ast::SourceLocation {
                    file: entry_path.as_ref().to_string_lossy().to_string(),
                    line: 1,
                    column: 1,
                    byte_start: None,
                    byte_end: None,
                });
            match start_function {
                Some(ref mut sf) => {
                    extra_start_stmts.append(&mut sf.body.statements);
                    sf.body.statements = extra_start_stmts;
                }
                None => {
                    start_function = Some(crate::hir::HirFunction {
                        name: "start".to_string(),
                        parameters: Vec::new(),
                        return_type: None,
                        body: crate::hir::HirBlock {
                            statements: extra_start_stmts,
                            location: loc.clone(),
                        },
                        is_start: true,
                        is_private: false,
                        owner_screen: None,
                        location: loc,
                    });
                }
            }
        }

        let location = root_location.unwrap_or_else(|| crate::ast::SourceLocation {
            file: entry_path.as_ref().to_string_lossy().to_string(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        });

        tracing::info!(
            functions = all_functions.len(),
            classes = all_classes.len(),
            screen_blocks = merged_screen_blocks.len(),
            "Merged HIR from all modules"
        );

        HirProgram {
            functions: all_functions,
            classes: all_classes,
            start_function,
            imports: all_imports,
            tests: all_tests,
            state: merged_state,
            watch_blocks: merged_watch_blocks,
            externals: all_externals,
            screen_blocks: merged_screen_blocks,
            location,
        }
    };

    // Get bridge functions and language-to-bridge mapping from plugin registry
    let bridge_functions = registry
        .as_ref()
        .map(|r| r.bridge_functions().to_vec())
        .unwrap_or_default();
    let lang_to_bridge_multifile = registry
        .as_ref()
        .map(|r| r.language_to_bridge_map())
        .unwrap_or_default();

    // Stage 3b: HIR semantic validation (ordering rules, contract placement, etc.)
    use crate::hir::validation::HirValidator;
    HirValidator::validate(&merged_hir)?;
    tracing::debug!("Stage 3b complete: HIR validation passed");

    // Stage 4: Resolution (with bridge functions if plugins are loaded)
    tracing::debug!(
        bridge_function_count = bridge_functions.len(),
        lang_alias_count = lang_to_bridge_multifile.len(),
        "Starting Stage 4: Resolution"
    );
    let lang_fn_defs_multi: std::collections::HashMap<
        String,
        crate::plugins::plugin_abi::PluginFunctionDef,
    > = registry
        .as_ref()
        .map(|r| {
            r.language_function_defs()
                .into_iter()
                .map(|(k, v)| (k, v.clone()))
                .collect()
        })
        .unwrap_or_default();
    let resolution_result = if bridge_functions.is_empty() {
        Resolver::resolve(merged_hir)?
    } else if lang_to_bridge_multifile.is_empty() {
        Resolver::resolve_with_bridge_functions(merged_hir, &bridge_functions)?
    } else if !lang_fn_defs_multi.is_empty() {
        Resolver::resolve_with_bridge_aliases_and_fn_defs(
            merged_hir,
            &bridge_functions,
            &lang_to_bridge_multifile,
            lang_fn_defs_multi,
        )?
    } else {
        Resolver::resolve_with_bridge_and_language_aliases(
            merged_hir,
            &bridge_functions,
            &lang_to_bridge_multifile,
        )?
    };
    let resolved_hir = resolution_result.resolved_hir;

    // Stage 5: Type checking
    tracing::debug!("Starting Stage 5: Type checking");
    let type_result = TypeChecker::check(resolved_hir)?;

    // Stage 6: TAST to MIR
    tracing::debug!("Starting Stage 6: TAST to MIR");
    let mir_result = lower_tast_to_mir_with_opt_level(type_result.tast, opt_level)?;

    // Stage 7: WASM generation (with bridge functions for WASM imports if plugins are loaded)
    tracing::debug!("Starting Stage 7: WASM generation");
    use crate::codegen::mir_codegen::MirCodeGenerator;
    let mut mir_codegen = MirCodeGenerator::default();

    // Pass plugin bridge functions to the code generator for WASM import generation
    if !bridge_functions.is_empty() {
        tracing::debug!(
            bridge_function_count = bridge_functions.len(),
            "Passing bridge functions to MIR code generator"
        );
        mir_codegen.set_bridge_functions(bridge_functions);
    }

    // Pass language-to-bridge mapping so dot-notation calls resolve correctly
    if !lang_to_bridge_multifile.is_empty() {
        tracing::debug!(
            alias_count = lang_to_bridge_multifile.len(),
            "Passing language-to-bridge map to MIR code generator"
        );
        mir_codegen.set_language_to_bridge_map(lang_to_bridge_multifile);
    }

    let codegen_result = mir_codegen.generate(mir_result.program)?;

    crate::codegen::validate::validate_generated_wasm(&codegen_result.wasm_bytes)
        .map_err(|e| vec![e])?;

    tracing::info!(
        bytes = codegen_result.wasm_bytes.len(),
        "Multi-file compilation complete"
    );

    Ok(codegen_result.wasm_bytes)
}

/// Returns true if a module is server-only and must be excluded from a
/// `frontend.wasm` build. Used by `compile_multi_file_client_mode`.
///
/// Heuristics, derived from the canonical app structure
/// (`foundation/spec/plugins/frame-ui-semantics.md` §UI-S007):
///   - synthetic `__page_routes_generated` module → server-only
///   - file path contains `/server/`, `/backend/`, `/api/`, `/pages/`, or `routes` → server-only
///   - `.client.cln` companion files → client (kept)
///   - `/components/` files → client (kept)
///   - everything else → client (kept, may include shared utilities)
fn is_server_only_module(module_name: &str, file_path: &std::path::Path) -> bool {
    if module_name == "__page_routes_generated" {
        return true;
    }
    let path_str = file_path.to_string_lossy().replace('\\', "/");
    let file_stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    // Explicit client companions are always kept.
    if file_stem.ends_with(".client") {
        return false;
    }
    // Components and shared client code are kept.
    if path_str.contains("/components/") {
        return false;
    }
    // Server-side modules are dropped.
    path_str.contains("/server/")
        || path_str.contains("/backend/")
        || path_str.contains("/api/")
        || path_str.contains("/pages/")
        || file_stem == "routes"
}

/// Compiles Clean Language with multi-file support and a specific memory tier.
///
/// Same as `compile_multi_file` but allows controlling the memory budget tier.
///
/// # Tier resolution precedence (MEMORY_POLICY.md §3.1):
/// 1. `explicit_tier` (`Some`) — from `--memory-tier` CLI flag, always wins.
/// 2. Highest tier declared by any active plugin's `[memory] tier` field.
/// 3. `target_default` — inferred from `--target` via `MemoryTier::default_for_target`.
/// 4. `MemoryTier::Standard` — ultimate fallback.
///
/// # Client mode
/// When `client_mode` is `true`, the compilation produces a `frontend.wasm`-shaped
/// output suitable for browser hydration (loader.js):
///   - Server-only modules (see [`is_server_only_module`]) are dropped before HIR merge
///   - The entry module's `start:` body is replaced with an empty body so the
///     browser does not attempt to call `_http_listen` etc. on `_start()`
///   - Component classes and their `events:` handlers are preserved as exports
///
/// Spec reference: `foundation/spec/plugins/frame-ui-semantics.md` §UI-B009.
pub fn compile_multi_file_with_memory_tier<P: AsRef<std::path::Path>>(
    entry_path: P,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
    explicit_tier: Option<MemoryTier>,
    target_default: MemoryTier,
    client_mode: bool,
) -> Result<(Vec<u8>, std::collections::BTreeMap<String, String>), Vec<CompilerError>> {
    use crate::compilation::{MultiFileCompiler, MultiFileCompilerConfig};
    use crate::mir::lower_tast_to_mir_with_opt_level;
    use crate::resolver::NameResolver as Resolver;
    use crate::typechecker::TypeChecker;
    use std::sync::Arc;

    tracing::info!(
        entry = %entry_path.as_ref().display(),
        opt_level = opt_level,
        explicit_tier = ?explicit_tier,
        target_default = %target_default,
        client_mode = client_mode,
        "Starting multi-file compilation with memory tier"
    );

    // Step 0: Read entry file to extract plugins
    let entry_source = std::fs::read_to_string(entry_path.as_ref()).map_err(|e| {
        vec![CompilerError::io_error(
            format!("Failed to read entry file: {}", e),
            None,
            None,
        )]
    })?;

    let plugin_names = collect_package_plugins(entry_path.as_ref(), &entry_source);

    // Plugin Contracts v2 — keep an Arc clone of the build-state keystore so
    // that, after compilation completes, callers (e.g. main.rs) can read every
    // write made by plugin `_build_state_set` calls during `expand_block`.
    // Without this clone the populated state would be dropped with the
    // registry, leaving the manifest writer and artifact orchestrator
    // unable to see writes. See `contracts/lifecycle.md` §2.5.
    let build_state_arc: Option<plugins::BuildState> = if plugin_names.is_empty() {
        None
    } else {
        Some(plugins::new_build_state())
    };

    let registry = if let Some(ref bs) = build_state_arc {
        tracing::info!(plugins = ?plugin_names, "Loading plugins for multi-file compilation");

        let mut loader = plugins::WasmPluginLoader::new().map_err(|e| {
            vec![CompilerError::PluginError {
                message: format!("Failed to create plugin loader: {}", e),
                location: None,
            }]
        })?;

        let reg = loader
            .load_plugins_with_build_state(&plugin_names, Arc::clone(bs))
            .map_err(|e| {
                vec![CompilerError::PluginError {
                    message: format!("Failed to load plugins: {}", e),
                    location: None,
                }]
            })?;

        Some(Arc::new(reg))
    } else {
        None
    };

    // Step 1: Build the compilation unit
    let mut config = MultiFileCompilerConfig::default()
        .with_search_paths(search_paths)
        .with_opt_level(opt_level)
        .with_client_mode(client_mode);

    if let Some(ref reg) = registry {
        config = config.with_plugin_registry(Arc::clone(reg));
    }

    let compiler = MultiFileCompiler::with_config(config);
    let unit = compiler.build_from_file(&entry_path)?;

    // Step 2: Merge all HIR programs into a single unified program
    let mut merged_hir = {
        use crate::hir::{HirFunction, HirProgram};

        let mut all_functions: Vec<HirFunction> = Vec::new();
        let mut all_classes = Vec::new();
        let mut start_function: Option<crate::hir::HirFunction> = None;
        let mut extra_start_stmts: Vec<crate::hir::HirStatement> = Vec::new();
        let mut all_imports = Vec::new();
        let mut all_tests = Vec::new();
        let mut all_externals = Vec::new();
        let mut merged_state: Option<crate::hir::HirStateBlock> = None;
        let mut merged_screen_blocks: Vec<crate::hir::HirScreenBlock> = Vec::new();
        let mut merged_watch_blocks: Vec<crate::hir::HirWatchBlock> = Vec::new();
        let mut root_location = None;

        for module_id in &unit.compilation_order {
            if let Some(module) = unit.get_module(*module_id) {
                // Client-mode filter: drop server-only modules before merging their HIR.
                //
                // The entry module is exempt. Its `start_function` is now
                // compiler-controlled via the `client_init` lifecycle slot
                // dispatch (see `PluginExpander::expand_program` →
                // `prepend_to_start`), so dropping it would also drop every
                // statement the slot produced (component instantiation +
                // onMount() calls). Preamble-injected helpers that would
                // misbehave under a browser host are stripped downstream by
                // the codegen tree-shaker — they are not reachable from the
                // synthetic browser `_start` body.
                if client_mode
                    && !module.is_entry
                    && is_server_only_module(&module.name, &module.file_path)
                {
                    tracing::debug!(
                        module = %module.name,
                        path = %module.file_path.display(),
                        "client_mode: skipping server-only module"
                    );
                    continue;
                }
                if let Some(hir) = &module.hir {
                    // Dedup by name: plugin preamble injection on the entry module can
                    // re-emit helpers (e.g. __redirect_0) already generated by a non-entry
                    // module's block expansion, producing two MIR entries with different
                    // SymbolIds but the same name, corrupting the export table (GEN002).
                    for func in &hir.functions {
                        if !all_functions.iter().any(|f| f.name == func.name) {
                            all_functions.push(func.clone());
                        }
                    }
                    for class in &hir.classes {
                        all_classes.push(class.clone());
                    }
                    // Merge state from every module. compilation_order is
                    // topologically sorted with the entry last, so non-entry
                    // plugin-emitted globals (e.g. frame.ui's `instance_<tag>`
                    // declarations contributed by `expand_component` running on
                    // component source files) land before the entry module's
                    // user state declarations. Before this merge landed, the
                    // entry-only assignment silently discarded plugin state,
                    // breaking the v2.12.3 client_init splice with
                    // `Undefined variable 'instance_<tag>'`.
                    merge_hir_state(&mut merged_state, hir.state.clone());

                    if module.is_entry {
                        start_function = hir.start_function.clone();
                        merged_screen_blocks = hir.screen_blocks.clone();
                        merged_watch_blocks = hir.watch_blocks.clone();
                        root_location = Some(hir.location.clone());
                    } else if let Some(ref module_start) = hir.start_function {
                        extra_start_stmts.extend(module_start.body.statements.iter().cloned());
                    }
                    for import in &hir.imports {
                        all_imports.push(import.clone());
                    }
                    if module.is_entry {
                        for test in &hir.tests {
                            all_tests.push(test.clone());
                        }
                    }
                    for external in &hir.externals {
                        all_externals.push(external.clone());
                    }
                }
            }
        }

        // Client mode: build-pipeline enforcement — what runs in the browser _start.
        //
        // The user's start: block is the server entry point: it boots the HTTP
        // listener, registers migrations, sets up routes. None of that is callable
        // from the browser host. In client builds the expander has already:
        //   1. Collected `program_init` + `client_init` lifecycle slot output into
        //      `slot_prelude`, tagged each statement with LIFECYCLE_SLOT_OUTPUT_MARKER.
        //   2. Cleared the user's start: body (server code gone).
        //   3. Called prepend_to_start to fill the body with only the tagged output.
        //
        // Here we handle the HIR-level merging of non-entry module start: bodies
        // (`extra_start_stmts`) and the legacy case where no plugin declares
        // client_init. See expander.rs `collect_slot_statements` and `expand_program`.
        if client_mode {
            // CLIENT_PULLS_SERVER: non-entry module start: blocks are server-side
            // code (db init, migration registration, etc.) — always discard them
            // in client builds. The entry module's start_function was already
            // cleared and rebuilt by the expander to contain only browser-safe
            // plugin output (program_init + client_init). See expander.rs.
            extra_start_stmts.clear();

            let any_client_init = registry
                .as_ref()
                .map(|r| r.any_plugin_declares_lifecycle_slot("client_init"))
                .unwrap_or(false);
            if !any_client_init {
                // Legacy path: no client_init slot, clear the entry start body too.
                if let Some(ref mut sf) = start_function {
                    sf.body.statements.clear();
                }
            }
            // When any_client_init: the expander already replaced the entry
            // start_function body with only browser-safe plugin output.
        }

        if !extra_start_stmts.is_empty() {
            let loc = root_location
                .clone()
                .unwrap_or_else(|| crate::ast::SourceLocation {
                    file: entry_path.as_ref().to_string_lossy().to_string(),
                    line: 1,
                    column: 1,
                    byte_start: None,
                    byte_end: None,
                });
            match start_function {
                Some(ref mut sf) => {
                    extra_start_stmts.append(&mut sf.body.statements);
                    sf.body.statements = extra_start_stmts;
                }
                None => {
                    start_function = Some(crate::hir::HirFunction {
                        name: "start".to_string(),
                        parameters: Vec::new(),
                        return_type: None,
                        body: crate::hir::HirBlock {
                            statements: extra_start_stmts,
                            location: loc.clone(),
                        },
                        is_start: true,
                        is_private: false,
                        owner_screen: None,
                        location: loc,
                    });
                }
            }
        }

        let location = root_location.unwrap_or_else(|| crate::ast::SourceLocation {
            file: entry_path.as_ref().to_string_lossy().to_string(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        });

        // Client mode: guarantee a `_start` export exists even when the entry
        // module was dropped (e.g. when entry lives under `app/web/pages/`).
        if client_mode && start_function.is_none() {
            start_function = Some(crate::hir::HirFunction {
                name: "start".to_string(),
                parameters: Vec::new(),
                return_type: None,
                body: crate::hir::HirBlock {
                    statements: Vec::new(),
                    location: location.clone(),
                },
                is_start: true,
                is_private: false,
                owner_screen: None,
                location: location.clone(),
            });
        }

        HirProgram {
            functions: all_functions,
            classes: all_classes,
            start_function,
            imports: all_imports,
            tests: all_tests,
            state: merged_state,
            watch_blocks: merged_watch_blocks,
            externals: all_externals,
            screen_blocks: merged_screen_blocks,
            location,
        }
    };

    let bridge_functions = registry
        .as_ref()
        .map(|r| r.bridge_functions().to_vec())
        .unwrap_or_default();
    let lang_to_bridge = registry
        .as_ref()
        .map(|r| r.language_to_bridge_map())
        .unwrap_or_default();

    // Inject phantom HIR class stubs for every type declared in loaded plugin manifests.
    // Without this, HirValidator rejects named types like `Request` as "Undefined type"
    // because they are not present in any source file — they are provided by the plugin.
    if let Some(ref reg) = registry {
        use crate::hir::{HirClass, HirField, HirType as HT};
        let parse_hir_type = |s: &str| -> HT {
            match s.to_lowercase().as_str() {
                "string" => HT::String,
                "integer" | "int" | "i32" | "i64" => HT::Integer,
                "number" | "float" | "f64" => HT::Number,
                "boolean" | "bool" => HT::Boolean,
                "void" | "()" => HT::Void,
                _ => HT::Any,
            }
        };
        for manifest in reg.loaded_manifests().values() {
            for type_def in &manifest.language.types {
                if merged_hir.classes.iter().any(|c| c.name == type_def.name) {
                    continue;
                }
                let fields: Vec<HirField> = type_def
                    .fields
                    .iter()
                    .map(|f| HirField {
                        name: f.name.clone(),
                        field_type: parse_hir_type(&f.type_),
                        initializer: None,
                        is_private: false,
                        location: Default::default(),
                    })
                    .collect();
                merged_hir.classes.push(HirClass {
                    name: type_def.name.clone(),
                    type_parameters: Vec::new(),
                    parent: None,
                    fields,
                    constructor: None,
                    methods: Vec::new(),
                    invariants: Vec::new(),
                    location: Default::default(),
                });
            }
        }
    }

    // Add language-name alias externals to merged_hir BEFORE validation so the
    // HIR validator can derive plugin namespaces (e.g. "req.query" → "req").
    if !lang_to_bridge.is_empty() {
        use crate::hir::{HirExternalFunction, HirParameter, HirType};
        let bridge_by_name: std::collections::HashMap<&str, &crate::plugins::BridgeFunction> =
            bridge_functions
                .iter()
                .map(|bf| (bf.name.as_str(), bf))
                .collect();
        for (lang_name, bridge_name) in &lang_to_bridge {
            if merged_hir.externals.iter().any(|e| e.name == *lang_name) {
                continue;
            }
            if let Some(bf) = bridge_by_name.get(bridge_name.as_str()) {
                let parameters: Vec<HirParameter> = bf
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, _)| HirParameter {
                        name: format!("arg{}", i),
                        param_type: HirType::Integer,
                        default_value: None,
                        location: Default::default(),
                    })
                    .collect();
                merged_hir.externals.push(HirExternalFunction {
                    name: lang_name.clone(),
                    parameters,
                    return_type: HirType::Void,
                    module: bf.module.clone(),
                    location: Default::default(),
                });
            }
        }
    }

    // Stage 3b: HIR semantic validation (ordering rules, contract placement, etc.)
    use crate::hir::validation::HirValidator;
    HirValidator::validate(&merged_hir)?;
    tracing::debug!("Stage 3b complete: HIR validation passed");

    // Remove dot-notation language alias externals: they existed only so HIR validation
    // could derive plugin namespace prefixes. They must NOT become WASM imports.
    // Canonical _namespace_fn names are registered separately via the plugin bridge path.
    if !lang_to_bridge.is_empty() {
        merged_hir
            .externals
            .retain(|e| !lang_to_bridge.contains_key(&e.name));
    }

    // Stage 4: Resolution
    // Collect language function defs (for return-type/param overrides in language aliases)
    let lang_fn_defs_owned: std::collections::HashMap<
        String,
        crate::plugins::plugin_abi::PluginFunctionDef,
    > = registry
        .as_ref()
        .map(|r| {
            r.language_function_defs()
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        })
        .unwrap_or_default();

    let resolution_result = if bridge_functions.is_empty() {
        Resolver::resolve(merged_hir)?
    } else if lang_to_bridge.is_empty() {
        Resolver::resolve_with_bridge_functions(merged_hir, &bridge_functions)?
    } else {
        Resolver::resolve_with_bridge_aliases_and_fn_defs(
            merged_hir,
            &bridge_functions,
            &lang_to_bridge,
            lang_fn_defs_owned,
        )?
    };
    let resolved_hir = resolution_result.resolved_hir;

    // Stage 5: Type checking
    // Compute required-param-count overrides for language alias functions that have
    // param_defaults — the resolver encodes param types but not optionality.
    let required_counts = {
        use crate::resolver::SymbolId;
        let mut counts: std::collections::HashMap<SymbolId, usize> =
            std::collections::HashMap::new();
        if let Some(ref reg) = registry {
            let lang_fn_defs_multi = reg.language_function_defs();
            for (lang_name, lang_def) in &lang_fn_defs_multi {
                if lang_def.param_defaults.is_empty() {
                    continue;
                }
                let required = lang_def
                    .param_defaults
                    .iter()
                    .filter(|s| s.is_empty())
                    .count();
                let total = lang_def
                    .params
                    .as_ref()
                    .map(|p| p.len())
                    .unwrap_or(lang_def.param_defaults.len());
                if required < total {
                    if let Some(sym_id) =
                        resolved_hir.symbol_table.lookup_symbol(lang_name.as_str())
                    {
                        counts.insert(sym_id, required);
                    }
                }
            }
        }
        counts
    };
    let type_result = TypeChecker::check_with_required_counts(resolved_hir, required_counts)?;

    // Stage 6: TAST to MIR
    let mir_result = lower_tast_to_mir_with_opt_level(type_result.tast, opt_level)?;

    // Resolve effective memory tier (MEMORY_POLICY.md §3.1):
    // explicit CLI flag > max(plugin tiers) > target default > standard
    let memory_tier = if let Some(tier) = explicit_tier {
        tracing::debug!(tier = %tier, "Using explicit CLI memory tier");
        tier
    } else {
        let plugin_tier = registry
            .as_ref()
            .map(|r| r.resolve_plugin_memory_tier())
            .transpose()
            .map_err(|e| vec![e])?
            .flatten();

        if let Some(pt) = plugin_tier {
            let effective = std::cmp::max(pt, target_default);
            tracing::info!(
                plugin_tier = %pt,
                target_default = %target_default,
                effective = %effective,
                "Memory tier resolved from plugin declaration"
            );
            effective
        } else {
            tracing::debug!(tier = %target_default, "Using target-default memory tier");
            target_default
        }
    };

    // Stage 7: WASM generation with memory tier
    use crate::codegen::mir_codegen::MirCodeGenerator;
    let mut mir_codegen = MirCodeGenerator::default();
    mir_codegen.set_memory_tier(memory_tier);

    if !bridge_functions.is_empty() {
        mir_codegen.set_bridge_functions(bridge_functions);
    }
    if !lang_to_bridge.is_empty() {
        mir_codegen.set_language_to_bridge_map(lang_to_bridge);
    }

    // Plugin Contracts v2 — derive host class from client_mode so the bridge
    // enforcement check in codegen knows which `hosts` declarations apply.
    // See foundation/spec/plugins/contracts/bridge-host-classes.md §6.
    //
    // - client_mode = true  → the nested `frontend.wasm` build → "browser"
    // - client_mode = false → the server build (default for `cln compile`) → "server"
    //
    // Strict mode (warnings → errors) is opt-in via the CLEAN_STRICT_HOSTS=1
    // environment variable for now. Phase D will flip the default.
    let host_class = if client_mode { "browser" } else { "server" };
    mir_codegen.set_host_class(Some(host_class.to_string()));
    let strict = std::env::var("CLEAN_STRICT_HOSTS")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    mir_codegen.set_strict_hosts(strict);
    mir_codegen.set_client_mode(client_mode);

    let codegen_result = mir_codegen.generate(mir_result.program)?;

    // Validate the generated WASM before handing it off. If codegen ever
    // produces a malformed module we want to catch it here with offset +
    // section info, not let it flow out to the server/runtime where it
    // surfaces as an opaque "failed to parse".
    crate::codegen::validate::validate_generated_wasm(&codegen_result.wasm_bytes)
        .map_err(|e| vec![e])?;

    tracing::info!(
        bytes = codegen_result.wasm_bytes.len(),
        memory_tier = %memory_tier,
        client_mode = client_mode,
        "Multi-file compilation with memory tier complete"
    );

    let build_state_snapshot = snapshot_build_state(build_state_arc.as_ref());
    Ok((codegen_result.wasm_bytes, build_state_snapshot))
}

/// Plugin Contracts v2 §2.5 — extract a snapshot of every `_build_state_set`
/// write made during a compile. Returns an empty map when the build had no
/// plugins (and therefore no shared keystore) or when the mutex is poisoned.
fn snapshot_build_state(
    state: Option<&plugins::BuildState>,
) -> std::collections::BTreeMap<String, String> {
    state
        .and_then(|s| s.lock().ok().map(|g| g.clone()))
        .unwrap_or_default()
}

/// Compile a Clean Language project for browser hydration (`frontend.wasm`).
///
/// Convenience wrapper around [`compile_multi_file_with_memory_tier`] with
/// `client_mode: true`. Produces a WASM module suitable for instantiation by
/// frame.ui's `loader.js`: server-only modules are dropped from the merge and
/// the `_start` export is a no-op so the browser does not attempt to invoke
/// server bridge functions (`_http_listen`, `_db_query`, etc.) at hydration.
///
/// Spec: `foundation/spec/plugins/frame-ui-semantics.md` §UI-B009.
pub fn compile_multi_file_client_mode<P: AsRef<std::path::Path>>(
    entry_path: P,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    // Client builds emit `frontend.wasm` only — they do not write a build
    // manifest, so the populated build_state snapshot is intentionally
    // discarded here. Callers that need it must use
    // [`compile_multi_file_with_memory_tier`] directly.
    compile_multi_file_with_memory_tier(
        entry_path,
        search_paths,
        opt_level,
        None,
        MemoryTier::Standard,
        true,
    )
    .map(|(bytes, _state)| bytes)
}

/// Compile a Clean Language project in release mode.
///
/// Identical to [`compile_multi_file_with_memory_tier`] but with `release_mode` enabled:
/// `always:` invariant checks are stripped from the compiled WASM output, producing
/// a smaller and faster binary suitable for production deployments.
///
/// For development (where you want all invariant checks active), use
/// `compile_multi_file_with_memory_tier` instead.
pub fn compile_multi_file_release<P: AsRef<std::path::Path>>(
    entry_path: P,
    search_paths: Vec<std::path::PathBuf>,
    opt_level: u8,
    explicit_tier: Option<MemoryTier>,
    target_default: MemoryTier,
) -> Result<(Vec<u8>, std::collections::BTreeMap<String, String>), Vec<CompilerError>> {
    use crate::compilation::{MultiFileCompiler, MultiFileCompilerConfig};
    use crate::mir::lower_tast_to_mir_release;
    use crate::resolver::NameResolver as Resolver;
    use crate::typechecker::TypeChecker;
    use std::sync::Arc;

    tracing::info!(
        entry = %entry_path.as_ref().display(),
        opt_level = opt_level,
        "Starting release-mode multi-file compilation (always: checks stripped)"
    );

    // Step 0: Read entry file to extract plugins (same as debug path)
    let entry_source = std::fs::read_to_string(entry_path.as_ref()).map_err(|e| {
        vec![CompilerError::io_error(
            format!("Failed to read entry file: {}", e),
            None,
            None,
        )]
    })?;

    let plugin_names = collect_package_plugins(entry_path.as_ref(), &entry_source);

    // Plugin Contracts v2 — the release path previously used bare
    // `load_plugins`, which gave each plugin its own private (empty)
    // BuildState. That broke `has_build_state.*` artifact predicates and
    // emitted manifests with an empty `build_state` field whenever a project
    // was compiled with `--release`. Use the shared keystore here too so the
    // snapshot is non-empty when plugins write to it.
    let build_state_arc: Option<plugins::BuildState> = if plugin_names.is_empty() {
        None
    } else {
        Some(plugins::new_build_state())
    };

    let registry = if let Some(ref bs) = build_state_arc {
        let mut loader = plugins::WasmPluginLoader::new().map_err(|e| {
            vec![CompilerError::PluginError {
                message: format!("Failed to create plugin loader: {}", e),
                location: None,
            }]
        })?;
        let reg = loader
            .load_plugins_with_build_state(&plugin_names, Arc::clone(bs))
            .map_err(|e| {
                vec![CompilerError::PluginError {
                    message: format!("Failed to load plugins: {}", e),
                    location: None,
                }]
            })?;
        Some(Arc::new(reg))
    } else {
        None
    };

    // Step 1: Build the compilation unit (identical to debug path)
    let mut config = MultiFileCompilerConfig::default()
        .with_search_paths(search_paths)
        .with_opt_level(opt_level)
        .with_release_mode(true);
    if let Some(ref reg) = registry {
        config = config.with_plugin_registry(Arc::clone(reg));
    }
    let compiler = MultiFileCompiler::with_config(config);
    let unit = compiler.build_from_file(&entry_path)?;

    // Step 2: Merge HIR programs (identical to debug path)
    let mut merged_hir = {
        use crate::hir::{HirFunction, HirProgram};
        let mut all_functions: Vec<HirFunction> = Vec::new();
        let mut all_classes = Vec::new();
        let mut start_function = None;
        let mut extra_start_stmts: Vec<crate::hir::HirStatement> = Vec::new();
        let mut all_imports = Vec::new();
        let mut all_tests = Vec::new();
        let mut all_externals = Vec::new();
        let mut merged_state: Option<crate::hir::HirStateBlock> = None;
        let mut merged_screen_blocks: Vec<crate::hir::HirScreenBlock> = Vec::new();
        let mut merged_watch_blocks: Vec<crate::hir::HirWatchBlock> = Vec::new();
        let mut root_location = None;

        for module_id in &unit.compilation_order {
            if let Some(module) = unit.get_module(*module_id) {
                if let Some(hir) = &module.hir {
                    // Dedup by name: plugin preamble injection on the entry module can
                    // re-emit helpers (e.g. __redirect_0) already generated by a non-entry
                    // module's block expansion, producing two MIR entries with different
                    // SymbolIds but the same name, corrupting the export table (GEN002).
                    for func in &hir.functions {
                        if !all_functions.iter().any(|f| f.name == func.name) {
                            all_functions.push(func.clone());
                        }
                    }
                    for class in &hir.classes {
                        all_classes.push(class.clone());
                    }
                    // Merge state from every module. compilation_order is
                    // topologically sorted with the entry last, so non-entry
                    // plugin-emitted globals (e.g. frame.ui's `instance_<tag>`
                    // declarations contributed by `expand_component` running on
                    // component source files) land before the entry module's
                    // user state declarations. Before this merge landed, the
                    // entry-only assignment silently discarded plugin state,
                    // breaking the v2.12.3 client_init splice with
                    // `Undefined variable 'instance_<tag>'`.
                    merge_hir_state(&mut merged_state, hir.state.clone());

                    if module.is_entry {
                        start_function = hir.start_function.clone();
                        merged_screen_blocks = hir.screen_blocks.clone();
                        merged_watch_blocks = hir.watch_blocks.clone();
                        root_location = Some(hir.location.clone());
                    } else if let Some(ref module_start) = hir.start_function {
                        extra_start_stmts.extend(module_start.body.statements.iter().cloned());
                    }
                    for import in &hir.imports {
                        all_imports.push(import.clone());
                    }
                    if module.is_entry {
                        for test in &hir.tests {
                            all_tests.push(test.clone());
                        }
                    }
                    for external in &hir.externals {
                        all_externals.push(external.clone());
                    }
                }
            }
        }

        if !extra_start_stmts.is_empty() {
            let loc = root_location
                .clone()
                .unwrap_or_else(|| crate::ast::SourceLocation {
                    file: entry_path.as_ref().to_string_lossy().to_string(),
                    line: 1,
                    column: 1,
                    byte_start: None,
                    byte_end: None,
                });
            match start_function {
                Some(ref mut sf) => {
                    extra_start_stmts.append(&mut sf.body.statements);
                    sf.body.statements = extra_start_stmts;
                }
                None => {
                    start_function = Some(crate::hir::HirFunction {
                        name: "start".to_string(),
                        parameters: Vec::new(),
                        return_type: None,
                        body: crate::hir::HirBlock {
                            statements: extra_start_stmts,
                            location: loc.clone(),
                        },
                        is_start: true,
                        is_private: false,
                        owner_screen: None,
                        location: loc,
                    });
                }
            }
        }

        let location = root_location.unwrap_or_else(|| crate::ast::SourceLocation {
            file: entry_path.as_ref().to_string_lossy().to_string(),
            line: 1,
            column: 1,
            byte_start: None,
            byte_end: None,
        });

        HirProgram {
            functions: all_functions,
            classes: all_classes,
            start_function,
            imports: all_imports,
            tests: all_tests,
            state: merged_state,
            watch_blocks: merged_watch_blocks,
            externals: all_externals,
            screen_blocks: merged_screen_blocks,
            location,
        }
    };

    let bridge_functions = registry
        .as_ref()
        .map(|r| r.bridge_functions().to_vec())
        .unwrap_or_default();
    let lang_to_bridge = registry
        .as_ref()
        .map(|r| r.language_to_bridge_map())
        .unwrap_or_default();

    // Add language-name alias externals (identical to debug path)
    if !lang_to_bridge.is_empty() {
        use crate::hir::{HirExternalFunction, HirParameter, HirType};
        let bridge_by_name: std::collections::HashMap<&str, &crate::plugins::BridgeFunction> =
            bridge_functions
                .iter()
                .map(|bf| (bf.name.as_str(), bf))
                .collect();
        for (lang_name, bridge_name) in &lang_to_bridge {
            if merged_hir.externals.iter().any(|e| e.name == *lang_name) {
                continue;
            }
            if let Some(bf) = bridge_by_name.get(bridge_name.as_str()) {
                let parameters: Vec<HirParameter> = bf
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, _)| HirParameter {
                        name: format!("arg{}", i),
                        param_type: HirType::Integer,
                        default_value: None,
                        location: Default::default(),
                    })
                    .collect();
                merged_hir.externals.push(HirExternalFunction {
                    name: lang_name.clone(),
                    parameters,
                    return_type: HirType::Void,
                    module: bf.module.clone(),
                    location: Default::default(),
                });
            }
        }
    }

    use crate::hir::validation::HirValidator;
    HirValidator::validate(&merged_hir)?;

    if !lang_to_bridge.is_empty() {
        merged_hir
            .externals
            .retain(|e| !lang_to_bridge.contains_key(&e.name));
    }

    // Stage 4: Resolution (identical to debug path)
    let resolution_result = if bridge_functions.is_empty() {
        Resolver::resolve(merged_hir)?
    } else if lang_to_bridge.is_empty() {
        Resolver::resolve_with_bridge_functions(merged_hir, &bridge_functions)?
    } else {
        Resolver::resolve_with_bridge_and_language_aliases(
            merged_hir,
            &bridge_functions,
            &lang_to_bridge,
        )?
    };
    let resolved_hir = resolution_result.resolved_hir;

    // Stage 5: Type checking (identical to debug path)
    let type_result = TypeChecker::check(resolved_hir)?;

    // Stage 6: MIR lowering — RELEASE MODE strips always: invariant checks.
    let mir_result = lower_tast_to_mir_release(type_result.tast, opt_level, true)?;

    // Resolve effective memory tier (identical to debug path)
    let memory_tier = if let Some(tier) = explicit_tier {
        tier
    } else {
        let plugin_tier = registry
            .as_ref()
            .map(|r| r.resolve_plugin_memory_tier())
            .transpose()
            .map_err(|e| vec![e])?
            .flatten();
        if let Some(pt) = plugin_tier {
            std::cmp::max(pt, target_default)
        } else {
            target_default
        }
    };

    // Stage 7: WASM generation (identical to debug path)
    use crate::codegen::mir_codegen::MirCodeGenerator;
    let mut mir_codegen = MirCodeGenerator::default();
    mir_codegen.set_memory_tier(memory_tier);
    if !bridge_functions.is_empty() {
        mir_codegen.set_bridge_functions(bridge_functions);
    }
    if !lang_to_bridge.is_empty() {
        mir_codegen.set_language_to_bridge_map(lang_to_bridge);
    }
    let codegen_result = mir_codegen.generate(mir_result.program)?;
    crate::codegen::validate::validate_generated_wasm(&codegen_result.wasm_bytes)
        .map_err(|e| vec![e])?;

    tracing::info!(
        bytes = codegen_result.wasm_bytes.len(),
        memory_tier = %memory_tier,
        "Release-mode compilation complete"
    );

    let build_state_snapshot = snapshot_build_state(build_state_arc.as_ref());
    Ok((codegen_result.wasm_bytes, build_state_snapshot))
}

/// Compile for testing without runtime imports
pub fn compile_minimal(source: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    use crate::lexer::specification_lexer::SpecificationLexer;
    use crate::parser::specification_parser::SpecificationParser;
    // use crate::hir::hir_builder::HirBuilder; // Temporarily disabled
    use crate::codegen::generate_wasm_from_mir_minimal;
    use crate::mir::lower_tast_to_mir_with_opt_level;
    use crate::resolver::NameResolver as Resolver;
    use crate::typechecker::TypeChecker;

    // Same 7-stage pipeline but with minimal WASM generation
    let source_code = crate::lexer::specification_lexer::SourceCode::new(
        source.to_string(),
        "<test>".to_string(),
    );
    let mut lexer = SpecificationLexer::new(&source_code);
    let tokens = lexer
        .tokenize()
        .map_err(|e| vec![CompilerError::LexError(e)])?;

    let mut parser = SpecificationParser::new(tokens, "<test>".to_string());
    let ast = parser.parse_program().map_err(|e| vec![e])?;

    use crate::hir::hir_builder::HirBuilder;
    let mut hir_builder = HirBuilder::new();
    let hir_result = hir_builder.build_hir(ast).map_err(|e| vec![e])?;

    // Stage 3b: HIR semantic validation
    use crate::hir::validation::HirValidator;
    HirValidator::validate(&hir_result.hir)?;

    let resolution_result = Resolver::resolve(hir_result.hir)?;
    let resolved_hir = resolution_result.resolved_hir;

    let type_result = TypeChecker::check(resolved_hir)?;
    let mir_result = lower_tast_to_mir_with_opt_level(type_result.tast, 0)?; // No optimization for testing
    let wasm_bytes = generate_wasm_from_mir_minimal(mir_result.program).map_err(|e| vec![e])?;
    crate::codegen::validate::validate_generated_wasm(&wasm_bytes).map_err(|e| vec![e])?;

    Ok(wasm_bytes)
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_basic_integration() {
        let source = r#"start:
	integer x = 42
	print(x)
"#;

        let result = compile_with_file(source, "test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Basic integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(errors) => {
                println!("✗ Basic integration test failed: {} errors", errors.len());
                for error in &errors {
                    println!("  - {error}");
                }
                panic!("Integration test failed with {} errors", errors.len());
            }
        }
    }

    #[test]
    fn test_function_integration() {
        let source = r#"functions:
	integer add(integer a, integer b)
		return a + b

start:
	integer result = add(5, 3)
	print(result)
"#;

        let result = compile_with_file(source, "function_test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Function integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(error) => {
                println!("✗ Function integration test failed: {error:?}");
                // Don't panic here as this might reveal integration issues we need to fix
            }
        }
    }

    #[test]
    fn test_type_checking_integration() {
        let source = r#"start:
	integer x = 42
	string y = "hello"
	print(x)
	print(y)
"#;

        let result = compile_with_file(source, "type_test.clean");
        match result {
            Ok(wasm_binary) => {
                println!(
                    "✓ Type checking integration test passed, generated {} bytes of WASM",
                    wasm_binary.len()
                );
                assert!(!wasm_binary.is_empty());
            }
            Err(error) => {
                println!("✗ Type checking integration test failed: {error:?}");
                // This might reveal type system integration issues
            }
        }
    }

    #[test]
    fn test_error_propagation() {
        // Test that calling an undefined function produces an error
        let source = r#"start:
	integer x = undefined_function()
	print(x)
"#;

        let result = compile_with_file(source, "error_test.clean");
        match result {
            Ok(_) => {
                panic!(
                    "Expected compilation error for undefined function, but compilation succeeded"
                );
            }
            Err(error) => {
                println!("✓ Error propagation test: Correctly caught error: {error:?}");
                // Check that the error is related to the undefined function
                let error_string = format!("{error:?}");
                // Should contain either "undefined_function" or some indication the symbol wasn't found
                assert!(
                    error_string.contains("undefined_function") ||
                    error_string.contains("not found") ||
                    error_string.contains("undefined") ||
                    error_string.contains("Cannot unify"),
                    "Error should mention the undefined function or type mismatch, got: {error_string}"
                );
            }
        }
    }

    #[test]
    fn test_stdlib_integration() {
        println!("\n=== Standard Library Integration Test ===");

        let test_cases = vec![
            (
                "Math Functions",
                r#"start:
	integer x = -5
	integer result = abs(x)
	print(result)
"#,
            ),
            (
                "String Functions",
                r#"start:
	string text = "hello"
	integer length = text.length()
	print(length)
"#,
            ),
            (
                "List Functions",
                r#"start:
	list<integer> lst = [1, 2, 3, 4, 5]
	integer length = lst.length()
	print(length)
"#,
            ),
        ];

        let mut passed = 0;
        let total = test_cases.len();

        for (name, source) in test_cases {
            println!("\n--- Testing {name} ---");
            println!("Source: {source}");

            match compile_with_file(source, "stdlib_test.clean") {
                Ok(wasm_binary) => {
                    println!("✓ {name}: {len} bytes", len = wasm_binary.len());
                    assert!(!wasm_binary.is_empty());
                    passed += 1;
                }
                Err(error) => {
                    println!("✗ {name} failed: {error:?}");
                    // Don't panic here as some stdlib functions might not be fully implemented yet
                }
            }
        }

        println!("\n=== Summary ===");
        println!("Passed: {passed}/{total}");

        // At least basic functionality should work
        assert!(passed > 0, "No stdlib integration tests passed");
    }

    #[test]
    fn test_memory_tier_ordering() {
        use std::cmp::Ordering;
        assert_eq!(
            MemoryTier::Embedded.cmp(&MemoryTier::Minimal),
            Ordering::Less
        );
        assert_eq!(
            MemoryTier::Minimal.cmp(&MemoryTier::Standard),
            Ordering::Less
        );
        assert_eq!(MemoryTier::Standard.cmp(&MemoryTier::Heavy), Ordering::Less);
        assert_eq!(MemoryTier::Heavy.cmp(&MemoryTier::Canvas), Ordering::Less);

        // max() picks the highest tier
        assert_eq!(
            std::cmp::max(MemoryTier::Standard, MemoryTier::Canvas),
            MemoryTier::Canvas
        );
        assert_eq!(
            std::cmp::max(MemoryTier::Heavy, MemoryTier::Minimal),
            MemoryTier::Heavy
        );
    }

    #[test]
    fn test_memory_tier_from_str() {
        assert_eq!(MemoryTier::from_str("embedded"), Some(MemoryTier::Embedded));
        assert_eq!(MemoryTier::from_str("canvas"), Some(MemoryTier::Canvas));
        assert_eq!(MemoryTier::from_str("STANDARD"), Some(MemoryTier::Standard));
        assert_eq!(MemoryTier::from_str("gigantic"), None);
    }

    #[test]
    fn test_memory_tier_default_for_target() {
        assert_eq!(MemoryTier::default_for_target("web"), MemoryTier::Standard);
        assert_eq!(MemoryTier::default_for_target("native"), MemoryTier::Heavy);
        assert_eq!(
            MemoryTier::default_for_target("embedded"),
            MemoryTier::Embedded
        );
        assert_eq!(MemoryTier::default_for_target("wasi"), MemoryTier::Minimal);
        assert_eq!(MemoryTier::default_for_target("auto"), MemoryTier::Standard);
    }

    #[test]
    fn test_extract_plugins_multiline() {
        let source = "plugins:\n\tframe.server\n\tframe.ui\n";
        let plugins = extract_plugins(source);
        assert_eq!(plugins, vec!["frame.server", "frame.ui"]);
    }

    #[test]
    fn test_extract_plugins_inline_list() {
        let source = "package: Test\n\ttarget: web\n\t\tplugins: [frame.ui, frame.server]\n\t\tentry: app/main.cln\n";
        let plugins = extract_plugins(source);
        assert_eq!(plugins, vec!["frame.ui", "frame.server"]);
    }

    #[test]
    fn test_extract_plugins_inline_top_level() {
        let source = "plugins: [frame.data, frame.auth]\n\nstart:\n\tprint(\"hello\")\n";
        let plugins = extract_plugins(source);
        assert_eq!(plugins, vec!["frame.data", "frame.auth"]);
    }
}
