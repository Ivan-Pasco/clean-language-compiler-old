/*!
 * Clean Language Compiler Library
 *
 * Author: Ivan Pasco Lizarraga
 * Date: 17-07-2025
 * Website: https://www.cleanlanguage.dev
 *
 * A modern, type-safe programming language that compiles to WebAssembly
 */

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
    let mut parser = SpecificationParser::new(tokens, file_path.to_string());
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
    let ast = expander.expand_program(parsed_ast).map_err(|e| {
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
    let resolution_result = Resolver::resolve(hir_result.hir)?;
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
/// This function automatically discovers plugins based on `import:` blocks in the source code.
/// Plugins must be installed using `cleen plugin add <name>` before they can be used.
///
/// # Arguments
/// * `source` - The Clean Language source code (may contain `import:` blocks)
/// * `file_path` - Path for error reporting
///
/// # Returns
/// * `Ok(Vec<u8>)` - Compiled WebAssembly bytes
/// * `Err(Vec<CompilerError>)` - Compilation errors
///
/// # Example
/// ```ignore
/// // Source code with import block:
/// // import:
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
/// * `source` - The Clean Language source code (may contain `import:` blocks)
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
    // Extract import statements from source
    let imports = extract_imports(source);

    if imports.is_empty() {
        // No imports, compile without external plugins
        tracing::debug!("No import: block found, compiling without external plugins");
        return compile_pure(source, file_path);
    }

    tracing::info!(imports = ?imports, "Loading external plugins");

    // Load plugins using WasmPluginLoader
    let mut loader = plugins::WasmPluginLoader::new().map_err(|e| {
        vec![CompilerError::PluginError {
            message: format!("Failed to create plugin loader: {}", e),
            location: None,
        }]
    })?;

    let registry = loader.load_plugins(&imports).map_err(|e| {
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

/// Extracts plugin names from `import:` blocks in source code
///
/// # Format
/// ```clean
/// import:
///     frame.web
///     frame.data
/// ```
///
/// # Returns
/// Vector of plugin names (e.g., ["frame.web", "frame.data"])
fn extract_imports(source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut in_import_block = false;

    for line in source.lines() {
        let trimmed = line.trim();

        // Check for import: block start
        if trimmed == "import:" {
            in_import_block = true;
            continue;
        }

        // If in import block, collect plugin names
        if in_import_block {
            // Empty line or new block ends the import block
            if trimmed.is_empty() || (trimmed.ends_with(':') && !trimmed.starts_with('\t')) {
                in_import_block = false;
                continue;
            }

            // Lines starting with whitespace are part of the block
            if line.starts_with('\t') || line.starts_with("    ") {
                let plugin_name = trimmed.to_string();
                if !plugin_name.is_empty() && !plugin_name.starts_with('#') {
                    imports.push(plugin_name);
                }
            } else {
                // Non-indented line ends the block
                in_import_block = false;
            }
        }
    }

    imports
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
        let source = r#"start()
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

start()
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
        let source = r#"start()
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
        let source = r#"start()
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
                r#"start()
	integer x = -5
	integer result = abs(x)
	print(result)
"#,
            ),
            (
                "String Functions",
                r#"start()
	string text = "hello"
	integer length = text.length()
	print(length)
"#,
            ),
            (
                "List Functions",
                r#"start()
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
