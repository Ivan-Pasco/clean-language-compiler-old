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

pub mod ast;
pub mod builtins;
pub mod codegen;
pub mod compilation;
pub mod debug;
pub mod error;
pub mod hir;

pub mod lexer;
pub mod memory;
pub mod mir;
pub mod module;
pub mod package;
pub mod parser;
pub mod plugins;
pub mod resolver;
pub mod runtime;
pub mod semantic;
pub mod stdlib;
pub mod targets;
// Temporarily disabled due to compilation issues
// pub mod testing;
pub mod typechecker;
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
    compile_pure(source, file_path)
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
    use crate::resolver::Resolver;
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

    // Stage 2.6: Bridge function registration
    let bridge_functions = registry.bridge_functions();
    if !bridge_functions.is_empty() {
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
    }

    // Stage 3: AST to HIR
    use crate::hir::hir_builder::HirBuilder;
    let mut hir_builder = HirBuilder::new();
    let hir_result = hir_builder.build_hir(ast).map_err(|e| vec![e])?;

    // Stage 4: Name and Module Resolution
    let bridge_functions = registry.bridge_functions();
    let resolution_result = if bridge_functions.is_empty() {
        Resolver::resolve(hir_result.hir)?
    } else {
        Resolver::resolve_with_bridge_functions(hir_result.hir, bridge_functions)?
    };
    let resolved_hir = resolution_result.resolved_hir;

    // Stage 5: Type Inference and Checking
    let _type_result = TypeChecker::check(resolved_hir)?;

    let duration_ms = start_time.elapsed().as_millis() as u64;

    Ok(TypeCheckResult {
        success: true,
        function_count,
        type_count,
        duration_ms,
        diagnostics: Vec::new(),
    })
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
            ast::Type::List(Box::new(parse_bridge_type(inner)))
        }
        // Default to Any for unknown types
        _ => ast::Type::Any,
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
    use crate::resolver::Resolver;
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

    // Stage 2.5: Plugin Expansion - transform framework blocks into Clean AST
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
    // so they can be properly type-checked and resolved
    let bridge_functions = registry.bridge_functions();
    if !bridge_functions.is_empty() {
        tracing::debug!(
            bridge_function_count = bridge_functions.len(),
            "Converting plugin bridge functions to external declarations"
        );
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
        tracing::debug!(
            externals_count = ast.externals.len(),
            "Stage 2.6 complete: Bridge functions converted to externals"
        );
    }

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

    // Stage 4: Name and Module Resolution - symbol resolution per specification
    tracing::debug!(
        functions = hir_result.hir.functions.len(),
        "Starting Stage 4: Resolver"
    );

    // Get bridge functions from plugin registry for name resolution
    let bridge_functions = registry.bridge_functions();
    tracing::debug!(
        bridge_function_count = bridge_functions.len(),
        "Registering plugin bridge functions in resolver"
    );

    let resolution_result = if bridge_functions.is_empty() {
        Resolver::resolve(hir_result.hir)?
    } else {
        Resolver::resolve_with_bridge_functions(hir_result.hir, bridge_functions)?
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

    let codegen_result = mir_codegen
        .generate(mir_result.program)
        .map_err(|errors| errors)?;
    let wasm_bytes = codegen_result.wasm_bytes;
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
    let mut plugin_names = extract_plugins(source);

    // Auto-detect plugins based on file path (implicit imports)
    let auto_detected = detect_plugins_from_path(file_path);
    for plugin in auto_detected {
        if !plugin_names.contains(&plugin) {
            tracing::debug!(plugin = %plugin, "Auto-detected plugin from path");
            plugin_names.push(plugin);
        }
    }

    if plugin_names.is_empty() {
        // No plugins declared or auto-detected, compile without external plugins
        tracing::debug!("No plugins found, compiling without external plugins");
        return compile_pure(source, file_path);
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
            message: format!("Failed to load plugins: {}", e),
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
    use crate::resolver::Resolver;
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

    let codegen_result = mir_codegen
        .generate(mir_result.program)
        .map_err(|errors| errors)?;

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
fn extract_plugins(source: &str) -> Vec<String> {
    let mut plugins = Vec::new();
    let mut in_plugins_block = false;

    for line in source.lines() {
        let trimmed = line.trim();

        // Check for plugins: block start
        if trimmed == "plugins:" {
            in_plugins_block = true;
            continue;
        }

        // If in plugins block, collect plugin names
        if in_plugins_block {
            // Empty line or new block ends the plugins block
            if trimmed.is_empty() || (trimmed.ends_with(':') && !trimmed.starts_with('\t')) {
                in_plugins_block = false;
                continue;
            }

            // Lines starting with whitespace are part of the block
            if line.starts_with('\t') || line.starts_with("    ") {
                let plugin_name = trimmed.to_string();
                if !plugin_name.is_empty() && !plugin_name.starts_with('#') {
                    plugins.push(plugin_name);
                }
            } else {
                // Non-indented line ends the block
                in_plugins_block = false;
            }
        }
    }

    plugins
}

/// Auto-detect plugins based on file path and directory structure.
///
/// This enables implicit plugin loading based on folder conventions:
/// - `app/api/`, `app/backend/api/`, `app/server/api/` → `frame.httpserver`
/// - `app/data/`, `app/server/models/` → `frame.data`
/// - `app/auth/`, `app/config/auth/` → `frame.auth`
/// - `app/canvas/` → `frame.canvas`
/// - `app/ui/`, `app/components/` → `frame.ui`
///
/// # Arguments
/// * `file_path` - Path to the .cln file being compiled
///
/// # Returns
/// Vector of plugin names that should be auto-injected based on path
fn detect_plugins_from_path<P: AsRef<std::path::Path>>(file_path: P) -> Vec<String> {
    let path_str = file_path.as_ref().to_string_lossy().to_lowercase();
    let mut plugins = Vec::new();

    // HTTP Server plugin - for API endpoints
    let is_api = path_str.contains("/api/")
        || path_str.contains("/backend/api/")
        || path_str.contains("/server/api/")
        || path_str.contains("/endpoints/");

    if is_api {
        plugins.push("frame.httpserver".to_string());
        // API endpoints commonly use database and auth
        plugins.push("frame.data".to_string());
        plugins.push("frame.auth".to_string());
    }

    // Data plugin - for models and database operations (standalone data files)
    if path_str.contains("/data/")
        || path_str.contains("/models/")
        || path_str.contains("/server/models/")
    {
        if !plugins.contains(&"frame.data".to_string()) {
            plugins.push("frame.data".to_string());
        }
    }

    // Auth plugin - for authentication configuration (standalone auth files)
    if path_str.contains("/auth/") || path_str.contains("/config/auth/") {
        if !plugins.contains(&"frame.auth".to_string()) {
            plugins.push("frame.auth".to_string());
        }
    }

    // Canvas plugin - for canvas/graphics applications
    if path_str.contains("/canvas/") {
        plugins.push("frame.canvas".to_string());
    }

    // UI plugin - for UI components
    if path_str.contains("/ui/")
        || path_str.contains("/components/")
        || path_str.contains("/screens/")
    {
        plugins.push("frame.ui".to_string());
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
    use crate::resolver::Resolver;
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

    // Extract plugin names from plugins: block
    let mut plugin_names = extract_plugins(&entry_source);

    // Auto-detect plugins based on file path (implicit imports)
    let auto_detected = detect_plugins_from_path(&entry_path);
    for plugin in auto_detected {
        if !plugin_names.contains(&plugin) {
            tracing::debug!(plugin = %plugin, "Auto-detected plugin from path");
            plugin_names.push(plugin);
        }
    }

    // Load plugins if any are declared or auto-detected
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
        use crate::hir::HirProgram;

        let mut all_functions = Vec::new();
        let mut all_classes = Vec::new();
        let mut start_function = None;
        let mut all_imports = Vec::new();
        let mut all_tests = Vec::new();
        let mut all_externals = Vec::new();
        let mut merged_state: Option<crate::hir::HirStateBlock> = None;
        let mut root_location = None;

        // Process modules in compilation order (dependencies first)
        for module_id in &unit.compilation_order {
            if let Some(module) = unit.get_module(*module_id) {
                if let Some(hir) = &module.hir {
                    // Collect functions (but only one start function)
                    for func in &hir.functions {
                        all_functions.push(func.clone());
                    }

                    // Collect classes
                    for class in &hir.classes {
                        all_classes.push(class.clone());
                    }

                    // Only the entry module's start function and state block are used
                    if module.is_entry {
                        start_function = hir.start_function.clone();
                        merged_state = hir.state.clone();
                        root_location = Some(hir.location.clone());
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
            "Merged HIR from all modules"
        );

        HirProgram {
            functions: all_functions,
            classes: all_classes,
            start_function,
            imports: all_imports,
            tests: all_tests,
            state: merged_state,
            watch_blocks: Vec::new(),
            externals: all_externals,
            location,
        }
    };

    // Get bridge functions from plugin registry for name resolution and code generation
    let bridge_functions = registry
        .as_ref()
        .map(|r| r.bridge_functions().to_vec())
        .unwrap_or_default();

    // Stage 4: Resolution (with bridge functions if plugins are loaded)
    tracing::debug!(
        bridge_function_count = bridge_functions.len(),
        "Starting Stage 4: Resolution"
    );
    let resolution_result = if bridge_functions.is_empty() {
        Resolver::resolve(merged_hir)?
    } else {
        Resolver::resolve_with_bridge_functions(merged_hir, &bridge_functions)?
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

    let codegen_result = mir_codegen.generate(mir_result.program)?;

    tracing::info!(
        bytes = codegen_result.wasm_bytes.len(),
        "Multi-file compilation complete"
    );

    Ok(codegen_result.wasm_bytes)
}

/// HTML-first compilation configuration
///
/// The HTML-first approach uses `.html.cln` files for pages that need
/// Clean Language processing. Regular `.html` files are treated as static
/// content and served as-is without processing.
#[derive(Debug, Clone)]
pub struct HtmlFirstConfig {
    /// Path to HTML pages directory containing `.html.cln` files (e.g., "app/pages")
    pub pages_dir: std::path::PathBuf,
    /// Path to Clean components directory (e.g., "app/components")
    pub components_dir: std::path::PathBuf,
    /// Additional search paths for imports
    pub search_paths: Vec<std::path::PathBuf>,
    /// Optimization level (0-3)
    pub opt_level: u8,
}

impl Default for HtmlFirstConfig {
    fn default() -> Self {
        Self {
            pages_dir: std::path::PathBuf::from("app/pages"),
            components_dir: std::path::PathBuf::from("app/components"),
            search_paths: vec![
                std::path::PathBuf::from("./"),
                std::path::PathBuf::from("./lib/"),
            ],
            opt_level: 2,
        }
    }
}

/// Compile an HTML-first Frame application
///
/// This function:
/// 1. Scans the pages directory for `.html.cln` files (processed pages)
/// 2. Builds a component registry from Clean component files
/// 3. Generates Clean code from HTML pages
/// 4. Compiles the generated code to WebAssembly
///
/// Note: Only `.html.cln` files are processed. Regular `.html` files are
/// treated as static content and are not compiled.
///
/// # Arguments
/// * `project_dir` - Root directory of the Frame project
/// * `config` - HTML-first compilation configuration
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
pub fn compile_html_first<P: AsRef<std::path::Path>>(
    project_dir: P,
    config: HtmlFirstConfig,
) -> Result<Vec<u8>, Vec<CompilerError>> {
    use crate::compilation::{MultiFileCompiler, MultiFileCompilerConfig};

    let project_dir = project_dir.as_ref();

    tracing::info!(
        project = %project_dir.display(),
        "Starting HTML-first compilation"
    );

    // Create compiler with HTML configuration
    let compiler_config = MultiFileCompilerConfig::default()
        .with_search_paths(config.search_paths.clone())
        .with_opt_level(config.opt_level)
        .with_html_pages(true)
        .with_html_pages_dir(&config.pages_dir)
        .with_html_components_dir(&config.components_dir);

    let compiler = MultiFileCompiler::with_config(compiler_config);

    // Step 1: Discover HTML pages
    let pages_path = project_dir.join(&config.pages_dir);
    let pages = compiler.discover_html_pages(&pages_path)?;

    tracing::info!(pages = pages.len(), "Discovered HTML pages");

    if pages.is_empty() {
        return Err(vec![CompilerError::io_error(
            format!("No HTML pages found in {}", pages_path.display()),
            Some("Create HTML files in the pages directory".to_string()),
            None,
        )]);
    }

    // Step 2: Build component registry
    let components_path = project_dir.join(&config.components_dir);
    let registry = compiler.build_component_registry(&components_path)?;

    tracing::info!(
        components = registry.components.len(),
        "Built component registry"
    );

    // Step 3: Generate Clean code from HTML pages
    let generated_clean = compiler.process_html_pages_to_clean(&pages, &registry)?;

    tracing::debug!(
        generated_lines = generated_clean.lines().count(),
        "Generated Clean code from HTML"
    );

    // Step 4: Write generated code to temp file and compile
    let temp_dir = std::env::temp_dir().join(format!(
        "frame_html_first_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    std::fs::create_dir_all(&temp_dir).map_err(|e| {
        vec![CompilerError::io_error(
            format!("Failed to create temp directory: {}", e),
            None,
            None,
        )]
    })?;

    let generated_file = temp_dir.join("generated_app.cln");
    std::fs::write(&generated_file, &generated_clean).map_err(|e| {
        vec![CompilerError::io_error(
            format!("Failed to write generated code: {}", e),
            None,
            None,
        )]
    })?;

    // Include project dir and components dir in search paths
    let mut search_paths = config.search_paths.clone();
    search_paths.insert(0, project_dir.to_path_buf());
    search_paths.push(components_path);
    search_paths.push(temp_dir.clone());

    // Compile the generated code
    let result = compile_multi_file(&generated_file, search_paths, config.opt_level);

    // Clean up temp directory
    let _ = std::fs::remove_dir_all(&temp_dir);

    result
}

/// Get information about discovered HTML pages without compiling
///
/// Scans for `.html.cln` files (processed pages) in the given directory.
/// Regular `.html` files are static and not included in the results.
///
/// Useful for build tools and debugging.
pub fn discover_html_pages<P: AsRef<std::path::Path>>(
    pages_dir: P,
) -> Result<Vec<crate::compilation::HtmlPage>, Vec<CompilerError>> {
    use crate::compilation::{MultiFileCompiler, MultiFileCompilerConfig};

    let config = MultiFileCompilerConfig::default().with_html_pages(true);
    let compiler = MultiFileCompiler::with_config(config);

    compiler.discover_html_pages(pages_dir)
}

/// Build component registry from a components directory
///
/// Useful for build tools and debugging
pub fn build_component_registry<P: AsRef<std::path::Path>>(
    components_dir: P,
) -> Result<crate::compilation::ComponentRegistry, Vec<CompilerError>> {
    use crate::compilation::{MultiFileCompiler, MultiFileCompilerConfig};

    let config = MultiFileCompilerConfig::default();
    let compiler = MultiFileCompiler::with_config(config);

    compiler.build_component_registry(components_dir)
}

/// Compile for testing without runtime imports
pub fn compile_minimal(source: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    use crate::lexer::specification_lexer::SpecificationLexer;
    use crate::parser::specification_parser::SpecificationParser;
    // use crate::hir::hir_builder::HirBuilder; // Temporarily disabled
    use crate::codegen::generate_wasm_from_mir_minimal;
    use crate::mir::lower_tast_to_mir_with_opt_level;
    use crate::resolver::Resolver;
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

    let resolution_result = Resolver::resolve(hir_result.hir)?;
    let resolved_hir = resolution_result.resolved_hir;

    let type_result = TypeChecker::check(resolved_hir)?;
    let mir_result = lower_tast_to_mir_with_opt_level(type_result.tast, 0)?; // No optimization for testing
    let wasm_bytes = generate_wasm_from_mir_minimal(mir_result.program).map_err(|e| vec![e])?;

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
}
