/*!
 * Clean Language Compiler - Command Line Interface
 *
 * Author: Ivan Pasco Lizarraga
 * Date: 17-07-2025
 * Website: https://www.cleanlanguage.dev
 *
 * A modern, type-safe programming language that compiles to WebAssembly
 */

#![allow(clippy::manual_inspect)]

use clean_language_compiler::codegen::CodeGenerator;
use clean_language_compiler::error::CompilerError;
use clean_language_compiler::parser::CleanParser;
use clean_language_compiler::runtime::runtime_manager::RuntimeManager;
use clean_language_compiler::runtime::runtime_trait::{RuntimeConfig, RuntimeType};
use clean_language_compiler::semantic::SemanticAnalyzer;
use clean_language_compiler::targets::{TargetManager, TargetOptimizer};
use std::env;
use std::fs;
use std::path::Path;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> Result<(), CompilerError> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "compile" => {
            let compile_args = parse_compile_args(&args[2..])?;
            compile_with_config(&compile_args)
        }
        "run" => {
            let run_args = parse_run_args(&args[2..])?;
            run_with_config(&run_args)
        }
        "parse" => {
            if args.len() < 3 {
                eprintln!("❌ Error: No input file specified.");
                print_usage();
                return Ok(());
            }
            let input_file = &args[2];
            parse_file(input_file)
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("❌ Error: No input file specified.");
                print_usage();
                return Ok(());
            }
            let input_file = &args[2];
            check_file(input_file)
        }
        "targets" => handle_targets_command(&args[2..]),
        "runtime" => handle_runtime_command(&args[2..]),
        "version" | "--version" | "-v" => {
            print_version();
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        _ => {
            eprintln!("❌ Unknown command: {command}", command = args[1]);
            print_usage();
            Ok(())
        }
    }
}

/// Configuration for compile command
#[derive(Debug)]
struct CompileConfig {
    input_file: String,
    output_file: String,
    target: String,
    runtime: RuntimeType,
    optimization: String,
    debug: bool,
    verbose: bool,
}

/// Configuration for run command
#[derive(Debug)]
struct RunConfig {
    input_file: String,
    target: String,
    runtime: RuntimeType,
    debug: bool,
    verbose: bool,
}

/// Parse compile command arguments
fn parse_compile_args(args: &[String]) -> Result<CompileConfig, CompilerError> {
    if args.is_empty() {
        return Err(CompilerError::runtime_error(
            "No input file specified".to_string(),
            None,
            None,
        ));
    }

    let mut config = CompileConfig {
        input_file: args[0].clone(),
        output_file: String::new(),
        target: "auto".to_string(),
        runtime: RuntimeType::Auto,
        optimization: "speed".to_string(),
        debug: false,
        verbose: false,
    };

    // Generate output filename if not specified
    if args.len() > 1 && !args[1].starts_with('-') {
        config.output_file = args[1].clone();
    } else {
        config.output_file = match Path::new(&config.input_file).file_stem() {
            Some(stem) => format!("{stem}.wasm", stem = stem.to_string_lossy()),
            None => format!("{}.wasm", config.input_file),
        };
    }

    // Parse flags
    let mut i = if config.output_file == *args.get(1).unwrap_or(&String::new()) {
        2
    } else {
        1
    };
    while i < args.len() {
        match args[i].as_str() {
            "--target" | "-t" => {
                if i + 1 < args.len() {
                    config.target = args[i + 1].clone();
                    i += 2;
                } else {
                    return Err(CompilerError::runtime_error(
                        "Target option requires a value".to_string(),
                        None,
                        None,
                    ));
                }
            }
            "--runtime" | "-r" => {
                if i + 1 < args.len() {
                    config.runtime = parse_runtime_type(&args[i + 1])?;
                    i += 2;
                } else {
                    return Err(CompilerError::runtime_error(
                        "Runtime option requires a value".to_string(),
                        None,
                        None,
                    ));
                }
            }
            "--optimization" | "-O" => {
                if i + 1 < args.len() {
                    config.optimization = args[i + 1].clone();
                    i += 2;
                } else {
                    return Err(CompilerError::runtime_error(
                        "Optimization option requires a value".to_string(),
                        None,
                        None,
                    ));
                }
            }
            "--debug" | "-d" => {
                config.debug = true;
                i += 1;
            }
            "--verbose" | "-v" => {
                config.verbose = true;
                i += 1;
            }
            _ => {
                return Err(CompilerError::runtime_error(
                    format!("Unknown option: {}", args[i]),
                    None,
                    None,
                ));
            }
        }
    }

    Ok(config)
}

/// Parse run command arguments
fn parse_run_args(args: &[String]) -> Result<RunConfig, CompilerError> {
    if args.is_empty() {
        return Err(CompilerError::runtime_error(
            "No input file specified".to_string(),
            None,
            None,
        ));
    }

    let mut config = RunConfig {
        input_file: args[0].clone(),
        target: "auto".to_string(),
        runtime: RuntimeType::Auto,
        debug: false,
        verbose: false,
    };

    // Parse flags
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--target" | "-t" => {
                if i + 1 < args.len() {
                    config.target = args[i + 1].clone();
                    i += 2;
                } else {
                    return Err(CompilerError::runtime_error(
                        "Target option requires a value".to_string(),
                        None,
                        None,
                    ));
                }
            }
            "--runtime" | "-r" => {
                if i + 1 < args.len() {
                    config.runtime = parse_runtime_type(&args[i + 1])?;
                    i += 2;
                } else {
                    return Err(CompilerError::runtime_error(
                        "Runtime option requires a value".to_string(),
                        None,
                        None,
                    ));
                }
            }
            "--debug" | "-d" => {
                config.debug = true;
                i += 1;
            }
            "--verbose" | "-v" => {
                config.verbose = true;
                i += 1;
            }
            _ => {
                return Err(CompilerError::runtime_error(
                    format!("Unknown option: {}", args[i]),
                    None,
                    None,
                ));
            }
        }
    }

    Ok(config)
}

/// Parse runtime type from string
fn parse_runtime_type(runtime_str: &str) -> Result<RuntimeType, CompilerError> {
    match runtime_str.to_lowercase().as_str() {
        "wasmtime" => Ok(RuntimeType::Wasmtime),
        "wasmer" => Ok(RuntimeType::Wasmer),
        "auto" => Ok(RuntimeType::Auto),
        _ => Err(CompilerError::runtime_error(
            format!("Unknown runtime type: {runtime_str}. Available: wasmtime, wasmer, auto"),
            None,
            None,
        )),
    }
}

/// Compile with enhanced configuration
fn compile_with_config(config: &CompileConfig) -> Result<(), CompilerError> {
    if config.verbose {
        println!("🔧 Compile Configuration:");
        println!("   Input: {}", config.input_file);
        println!("   Output: {}", config.output_file);
        println!("   Target: {}", config.target);
        println!("   Runtime: {}", config.runtime);
        println!("   Optimization: {}", config.optimization);
        println!("   Debug: {}", config.debug);
    }

    // Get target configuration
    let target = if config.target == "auto" {
        TargetManager::auto_detect_target()
    } else {
        TargetManager::get_target(&config.target)?
    };

    // Create runtime configuration
    let mut runtime_config = TargetManager::get_recommended_runtime_config(&target);
    runtime_config.runtime_type = config.runtime;
    runtime_config.debug_info = config.debug;

    // Apply target-specific optimizations
    TargetOptimizer::optimize_for_target(&mut runtime_config, &target);

    // Validate configuration
    TargetManager::validate_target_runtime_compatibility(&target, &runtime_config)?;

    if config.verbose {
        println!("🎯 Using target: {}", target);
        println!("⚙️ Runtime config: {:?}", runtime_config);
    }

    // Perform compilation (use existing compile_file for now)
    compile_file(&config.input_file, &config.output_file)
}

/// Run with enhanced configuration
fn run_with_config(config: &RunConfig) -> Result<(), CompilerError> {
    if config.verbose {
        println!("🚀 Run Configuration:");
        println!("   Input: {}", config.input_file);
        println!("   Target: {}", config.target);
        println!("   Runtime: {}", config.runtime);
        println!("   Debug: {}", config.debug);
    }

    // Get target configuration
    let target = if config.target == "auto" {
        TargetManager::auto_detect_target()
    } else {
        TargetManager::get_target(&config.target)?
    };

    // Create runtime configuration
    let mut runtime_config = TargetManager::get_recommended_runtime_config(&target);
    runtime_config.runtime_type = config.runtime;
    runtime_config.debug_info = config.debug;

    // Apply target-specific optimizations
    TargetOptimizer::optimize_for_target(&mut runtime_config, &target);

    if config.verbose {
        println!("🎯 Using target: {}", target);
        println!("⚙️ Runtime config: {:?}", runtime_config);
    }

    // Perform run (use existing run_file for now)
    run_file(&config.input_file)
}

/// Handle targets subcommand
fn handle_targets_command(args: &[String]) -> Result<(), CompilerError> {
    if args.is_empty() || args[0] == "list" {
        list_targets()
    } else if args[0] == "info" && args.len() > 1 {
        show_target_info(&args[1])
    } else {
        eprintln!("❌ Unknown targets command. Available: list, info <target>");
        Ok(())
    }
}

/// List available targets
fn list_targets() -> Result<(), CompilerError> {
    println!("🎯 Available Compilation Targets:");
    println!("==================================");

    let targets = TargetManager::get_available_targets();
    for target in &targets {
        println!("  • {} - {}", target.name, target.description);
    }

    println!();
    println!("💡 Use 'cln targets info <target>' for detailed information");
    println!("💡 Use '--target <name>' flag to specify target during compilation");

    Ok(())
}

/// Show detailed target information
fn show_target_info(target_name: &str) -> Result<(), CompilerError> {
    let target = TargetManager::get_target(target_name)?;

    println!("🎯 Target Information: {}", target.name);
    println!("========================================");
    println!("Description: {}", target.description);
    println!("Type: {}", target.target_type);
    println!("Preferred Runtime: {}", target.runtime_preference);
    println!();

    println!("Capabilities:");
    println!(
        "  • Async Support: {}",
        if target.capabilities.async_support {
            "✅"
        } else {
            "❌"
        }
    );
    println!(
        "  • Threading: {}",
        if target.capabilities.threads_support {
            "✅"
        } else {
            "❌"
        }
    );
    println!(
        "  • SIMD: {}",
        if target.capabilities.simd_support {
            "✅"
        } else {
            "❌"
        }
    );
    println!(
        "  • Bulk Memory: {}",
        if target.capabilities.bulk_memory {
            "✅"
        } else {
            "❌"
        }
    );
    println!(
        "  • Reference Types: {}",
        if target.capabilities.reference_types {
            "✅"
        } else {
            "❌"
        }
    );
    println!(
        "  • WASI Support: {}",
        if target.capabilities.wasi_support {
            "✅"
        } else {
            "❌"
        }
    );

    if let Some(max_memory) = target.capabilities.max_memory_size {
        println!("  • Max Memory: {}MB", max_memory / (1024 * 1024));
    }

    println!();
    println!("Optimization Recommendations:");
    let recommendations = TargetManager::get_optimization_recommendations(&target);
    for rec in recommendations {
        println!("  • {rec}");
    }

    Ok(())
}

/// Handle runtime subcommand
fn handle_runtime_command(args: &[String]) -> Result<(), CompilerError> {
    if args.is_empty() || args[0] == "list" {
        list_runtimes()
    } else if args[0] == "detect" {
        detect_runtime()
    } else if args[0] == "benchmark" && args.len() > 1 {
        benchmark_runtimes(&args[1])
    } else {
        eprintln!("❌ Unknown runtime command. Available: list, detect, benchmark <file>");
        Ok(())
    }
}

/// List available runtimes
fn list_runtimes() -> Result<(), CompilerError> {
    println!("⚙️ Available WebAssembly Runtimes:");
    println!("===================================");

    let runtimes = RuntimeManager::list_available_runtimes();
    for runtime in &runtimes {
        let status = if runtime.available {
            "✅ Available"
        } else {
            "❌ Not Available"
        };
        println!("  • {} - {status}", runtime);

        if runtime.available {
            println!("    Features: {}", runtime.features.join(", "));
        }
        println!();
    }

    println!("💡 Use '--runtime <name>' flag to specify runtime during compilation/execution");

    Ok(())
}

/// Auto-detect best runtime
fn detect_runtime() -> Result<(), CompilerError> {
    println!("🔍 Detecting Best Runtime...");

    let config = RuntimeConfig::default();
    match RuntimeManager::select_runtime(&config) {
        Ok(runtime_type) => {
            println!("✅ Recommended runtime: {runtime_type}");

            let recommendations = RuntimeManager::get_runtime_recommendations(runtime_type);
            if !recommendations.is_empty() {
                println!("\n💡 Recommendations:");
                for rec in recommendations {
                    println!("  • {rec}");
                }
            }
        }
        Err(e) => {
            println!("❌ Runtime detection failed: {e}");
        }
    }

    Ok(())
}

/// Benchmark runtimes with a test file
fn benchmark_runtimes(file_path: &str) -> Result<(), CompilerError> {
    println!("🏁 Benchmarking Runtimes with: {file_path}");
    println!("===========================================");

    // Read and compile the test file
    let source = fs::read_to_string(file_path).map_err(|e| {
        CompilerError::runtime_error(
            format!("Failed to read file '{file_path}': {e}"),
            None,
            None,
        )
    })?;

    // Compile to WASM
    let wasm_bytes = clean_language_compiler::compile_with_file(&source, file_path)?;

    // Run benchmarks
    match RuntimeManager::benchmark_runtimes(&wasm_bytes) {
        Ok(benchmarks) => {
            if benchmarks.is_empty() {
                println!("⚠️ Benchmarking not yet implemented - coming soon!");
            } else {
                for benchmark in benchmarks {
                    println!("  {benchmark}");
                }
            }
        }
        Err(e) => {
            println!("❌ Benchmark failed: {e}");
        }
    }

    Ok(())
}

fn print_usage() {
    println!("🧹 Clean Language Compiler (cln) v{VERSION}");
    println!("Author: Ivan Pasco Lizarraga");
    println!("Website: https://www.cleanlanguage.dev");
    println!();
    println!("USAGE:");
    println!("    cln <COMMAND> [OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("    compile <input> [output]    Compile Clean source to WebAssembly");
    println!("    run <input>                 Compile and run a Clean program");
    println!("    parse <input>               Parse and validate syntax only");
    println!("    check <input>               Type check without compilation");
    println!("    targets <subcommand>        Manage compilation targets");
    println!("    runtime <subcommand>        Manage WebAssembly runtimes");
    println!("    version                     Show version information");
    println!("    help                        Show this help message");
    println!();
    println!("COMPILE/RUN OPTIONS:");
    println!("    --target, -t <target>       Target platform (web, nodejs, native, embedded, wasi, auto)");
    println!("    --runtime, -r <runtime>     WebAssembly runtime (wasmtime, wasmer, auto)");
    println!("    --optimization, -O <level>  Optimization level (development, production, size, speed, debug)");
    println!("    --debug, -d                 Include debug information");
    println!("    --verbose, -v               Verbose output");
    println!();
    println!("TARGET SUBCOMMANDS:");
    println!("    targets list                List all available targets");
    println!("    targets info <target>       Show detailed target information");
    println!();
    println!("RUNTIME SUBCOMMANDS:");
    println!("    runtime list                List available WebAssembly runtimes");
    println!("    runtime detect              Auto-detect best runtime for current system");
    println!("    runtime benchmark <file>    Benchmark runtimes with a test file");
    println!();
    println!("EXAMPLES:");
    println!("    cln compile hello.cln                           # Basic compilation");
    println!("    cln compile hello.cln --target web --debug      # Web target with debug info");
    println!(
        "    cln run app.cln --target nodejs --verbose       # Run on Node.js with verbose output"
    );
    println!("    cln targets list                                # List all targets");
    println!("    cln targets info web                            # Show web target details");
    println!("    cln runtime detect                              # Auto-detect best runtime");
    println!("    cln runtime benchmark test.cln                  # Benchmark with test file");
    println!();
    println!("For more information, visit: https://www.cleanlanguage.dev");
}

fn print_version() {
    println!("Clean Language Compiler v{VERSION}");
    println!("Author: Ivan Pasco Lizarraga");
    println!("Date: 17-07-2025");
    println!("Website: https://www.cleanlanguage.dev");
}

fn compile_file(input_file: &str, output_file: &str) -> Result<(), CompilerError> {
    println!("🔨 Compiling {input_file} → {output_file}");

    // Read the input file
    let source = read_source_file(input_file)?;

    // Parse the program
    let program = parse_source(&source, input_file)?;

    // Semantic analysis
    let mut semantic_analyzer = SemanticAnalyzer::new();
    let analyzed_program = semantic_analyzer.analyze(&program).map_err(|e| {
        display_error(&e, &source, input_file);
        e
    })?;

    // Code generation
    let mut code_generator = CodeGenerator::new();
    let wasm_binary = code_generator.generate(&analyzed_program)?;

    // Write the output file
    fs::write(output_file, wasm_binary).map_err(|e| {
        CompilerError::io_error(format!("Failed to write output file: {e}"), None, None)
    })?;

    println!("✅ Compilation successful! Generated {output_file}");
    Ok(())
}

fn run_file(input_file: &str) -> Result<(), CompilerError> {
    println!("🚀 Running {input_file}");

    // Check if it's a WASM file or Clean source
    if input_file.ends_with(".wasm") {
        execute_wasm_file(input_file)
    } else {
        // Compile first, then run
        let file_stem = input_file.trim_end_matches(".cln");
        let temp_wasm = format!("{file_stem}.temp.wasm");
        compile_file(input_file, &temp_wasm)?;
        let result = execute_wasm_file(&temp_wasm);

        // Clean up temporary file
        let _ = fs::remove_file(&temp_wasm);

        result
    }
}

fn parse_file(input_file: &str) -> Result<(), CompilerError> {
    println!("📝 Parsing {input_file}");

    let source = read_source_file(input_file)?;
    let _program = parse_source(&source, input_file)?;

    println!("✅ Parsing successful! Syntax is valid.");
    Ok(())
}

fn check_file(input_file: &str) -> Result<(), CompilerError> {
    println!("🔍 Type checking {input_file}");

    let source = read_source_file(input_file)?;
    let program = parse_source(&source, input_file)?;

    // Semantic analysis
    let mut semantic_analyzer = SemanticAnalyzer::new();
    let _analyzed_program = semantic_analyzer.analyze(&program).map_err(|e| {
        display_error(&e, &source, input_file);
        e
    })?;

    println!("✅ Type checking successful! All types are valid.");
    Ok(())
}

fn read_source_file(input_file: &str) -> Result<String, CompilerError> {
    fs::read_to_string(input_file).map_err(|e| {
        CompilerError::io_error(
            format!("Failed to read file '{input_file}': {e}"),
            None,
            None,
        )
    })
}

fn parse_source(
    source: &str,
    file_path: &str,
) -> Result<clean_language_compiler::ast::Program, CompilerError> {
    CleanParser::parse_program_with_file(source, file_path).map_err(|e| {
        display_error(&e, source, file_path);
        e
    })
}

fn execute_wasm_file(wasm_file: &str) -> Result<(), CompilerError> {
    println!("⚡ Executing {wasm_file}");
    println!("DEBUG: About to read WASM file...");

    // Read the WASM file
    let wasm_bytes = fs::read(wasm_file).map_err(|e| {
        CompilerError::io_error(format!("Failed to read WASM file: {e}"), None, None)
    })?;
    println!(
        "DEBUG: WASM file read successfully, {} bytes",
        wasm_bytes.len()
    );

    // Use synchronous execution with standardized wasmtime configuration
    println!("DEBUG: Starting WASM execution...");
    run_wasm_sync(&wasm_bytes)
}

fn run_wasm_sync(wasm_bytes: &[u8]) -> Result<(), CompilerError> {
    use wasmtime::{Linker, Module, Store};

    // Use full Clean Language wasmtime configuration for execution
    println!("DEBUG: Creating engine...");
    let engine =
        clean_language_compiler::runtime::wasmtime_config::CleanWasmtimeConfig::create_engine()?;

    // Create store
    let mut store = Store::new(&engine, ());

    // Create linker and register host functions FIRST
    let mut linker = Linker::new(&engine);
    println!("DEBUG: Registering host functions...");
    clean_language_compiler::runtime::host_functions::register_all_host_functions(&mut linker)
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to register host functions: {e}"),
                None,
                None,
            )
        })?;
    println!("DEBUG: Host functions registered successfully");

    // Create a module from the bytes
    println!("DEBUG: Creating module from WASM bytes...");
    println!("DEBUG: WASM bytes length: {}", wasm_bytes.len());
    let module = Module::new(&engine, wasm_bytes).map_err(|e| {
        println!("DEBUG: Module creation failed with error: {}", e);
        CompilerError::runtime_error(
            format!("Failed to create WebAssembly module: {e}"),
            None,
            None,
        )
    })?;
    println!("DEBUG: Module created successfully");

    // Instantiate the module
    println!("DEBUG: Instantiating module...");
    let instance = linker.instantiate(&mut store, &module).map_err(|e| {
        CompilerError::runtime_error(
            format!("Failed to instantiate WebAssembly module: {e}"),
            None,
            None,
        )
    })?;
    println!("DEBUG: Module instantiated successfully");

    // Call the start function
    if let Some(start_func) = instance.get_func(&mut store, "start") {
        start_func
            .call(&mut store, &[], &mut [])
            .map_err(|e| CompilerError::runtime_error(format!("Runtime error: {e}"), None, None))?;
    } else {
        return Err(CompilerError::runtime_error(
            "No 'start' function found in WebAssembly module".to_string(),
            None,
            None,
        ));
    }

    Ok(())
}

fn display_error(error: &CompilerError, _source: &str, file_path: &str) {
    eprintln!();
    eprintln!("💥 Compilation Error");
    eprintln!("📁 File: {file_path}");
    eprintln!();

    match error {
        CompilerError::Syntax { context } => {
            eprintln!("❌ Syntax Error: {message}", message = context.message);

            if let Some(location) = &context.location {
                eprintln!(
                    "📍 Location: Line {}, Column {}",
                    location.line, location.column
                );
            }

            if let Some(help) = &context.help {
                eprintln!("💡 Help: {help}");
            }

            if !context.suggestions.is_empty() {
                eprintln!("🔧 Suggestions:");
                for suggestion in &context.suggestions {
                    eprintln!("  • {suggestion}");
                }
            }
        }
        CompilerError::Type { context } => {
            eprintln!("❌ Type Error: {message}", message = context.message);

            if let Some(location) = &context.location {
                eprintln!(
                    "📍 Location: Line {}, Column {}",
                    location.line, location.column
                );
            }

            if let Some(help) = &context.help {
                eprintln!("💡 Help: {help}");
            }

            if !context.suggestions.is_empty() {
                eprintln!("🔧 Suggestions:");
                for suggestion in &context.suggestions {
                    eprintln!("  • {suggestion}");
                }
            }
        }
        _ => {
            eprintln!("❌ Error: {error}");
        }
    }

    eprintln!();
}
