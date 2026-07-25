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
#![allow(deprecated)]

use clean_language_compiler::codegen::bridge_generator::{BridgeGenerator, BridgeTarget};
use clean_language_compiler::error::CompilerError;
use clean_language_compiler::parser::CleanParser;
use clean_language_compiler::runtime::runtime_manager::RuntimeManager;
use clean_language_compiler::runtime::runtime_trait::{
    OptimizationLevel, RuntimeConfig, RuntimeType,
};
use clean_language_compiler::targets::{OptimizationProfile, TargetManager, TargetOptimizer};
use std::env;
use std::fs;
use std::path::Path;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> Result<(), CompilerError> {
    let args: Vec<String> = env::args().collect();

    // Initialize logging early (can be overridden by RUST_LOG environment variable)
    // Default to "warn" to keep output clean, use --verbose for "debug"
    let log_level = if args.contains(&"--verbose".to_string()) || args.contains(&"-v".to_string()) {
        "debug"
    } else {
        "warn"
    };
    clean_language_compiler::init_logging(log_level);

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "build" => {
            let build_args = parse_build_args(&args[2..])?;
            build_with_config(&build_args)
        }
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
    optimization_level: OptimizationLevel,
    optimization_profile: Option<String>,
    memory_tier: Option<clean_language_compiler::MemoryTier>,
    debug: bool,
    verbose: bool,
    /// `--enable-legacy-json-wasm` — fall back to the pre-Delivery-2 pure-
    /// WASM JSON stdlib path in `src/stdlib/json_class.rs`. Default (flag
    /// unset) as of 0.33.139 / [P4c] is bridge dispatch through
    /// `_json_decode_v2` / `_json_encode_v2` / `_json_encode_pretty_v2`.
    /// Opt-out escape hatch during the Delivery-2 soak window; removed in
    /// [P5]. See JSON_MIGRATION_DELIVERY2.md.
    enable_legacy_json_wasm: bool,
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

/// Configuration for build command (multi-file compilation)
#[derive(Debug)]
struct BuildConfig {
    input_file: String,
    output_file: String,
    search_paths: Vec<std::path::PathBuf>,
    optimization_level: u8,
    debug: bool,
    verbose: bool,
}

/// Parse build command arguments (for multi-file compilation)
fn parse_build_args(args: &[String]) -> Result<BuildConfig, CompilerError> {
    if args.is_empty() {
        return Err(CompilerError::runtime_error(
            "No input file specified".to_string(),
            None,
            None,
        ));
    }

    let mut config = BuildConfig {
        input_file: args[0].clone(),
        output_file: String::new(),
        search_paths: Vec::new(),
        optimization_level: 2, // Default: -O2
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
            "--include" | "-I" => {
                if i + 1 < args.len() {
                    config
                        .search_paths
                        .push(std::path::PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    return Err(CompilerError::runtime_error(
                        "Include path option requires a value".to_string(),
                        None,
                        None,
                    ));
                }
            }
            "-O0" => {
                config.optimization_level = 0;
                i += 1;
            }
            "-O1" => {
                config.optimization_level = 1;
                i += 1;
            }
            "-O2" => {
                config.optimization_level = 2;
                i += 1;
            }
            "-O3" => {
                config.optimization_level = 3;
                i += 1;
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

/// Build with multi-file compilation
fn build_with_config(config: &BuildConfig) -> Result<(), CompilerError> {
    if config.verbose {
        println!("🔨 Building {} → {}", config.input_file, config.output_file);
        if !config.search_paths.is_empty() {
            println!("   Search paths: {:?}", config.search_paths);
        }
    }

    // Use multi-file compilation
    let wasm_bytes = clean_language_compiler::compile_multi_file(
        &config.input_file,
        config.search_paths.clone(),
        config.optimization_level,
    )
    .map_err(|errors| {
        for error in &errors {
            eprintln!("❌ Error: {}", error);
        }
        errors.into_iter().next().unwrap_or_else(|| {
            CompilerError::runtime_error(
                "Compilation failed with unknown error".to_string(),
                None,
                None,
            )
        })
    })?;

    // Write output
    fs::write(&config.output_file, wasm_bytes).map_err(|e| {
        CompilerError::io_error(format!("Failed to write output file: {e}"), None, None)
    })?;

    println!(
        "✅ Build successful! Generated {} ({} bytes)",
        config.output_file,
        fs::metadata(&config.output_file)
            .map(|m| m.len())
            .unwrap_or(0)
    );

    Ok(())
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
        optimization_level: OptimizationLevel::Speed, // Default: -O2 equivalent
        optimization_profile: None,
        memory_tier: None,
        debug: false,
        verbose: false,
        enable_legacy_json_wasm: false,
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
            // Standard optimization levels: -O0, -O1, -O2, -O3
            "-O0" => {
                config.optimization_level = OptimizationLevel::None;
                i += 1;
            }
            "-O1" => {
                config.optimization_level = OptimizationLevel::Speed;
                i += 1;
            }
            "-O2" => {
                config.optimization_level = OptimizationLevel::Speed;
                i += 1;
            }
            "-O3" => {
                config.optimization_level = OptimizationLevel::SpeedAndSize;
                i += 1;
            }
            // Optimization profile: --optimization <profile>
            "--optimization" | "-O" => {
                if i + 1 < args.len() {
                    let profile_or_level = &args[i + 1];
                    // Check if it's a numeric level (0-3)
                    match profile_or_level.as_str() {
                        "0" | "none" => config.optimization_level = OptimizationLevel::None,
                        "1" | "speed" => config.optimization_level = OptimizationLevel::Speed,
                        "2" => config.optimization_level = OptimizationLevel::Speed,
                        "3" | "size" | "aggressive" => {
                            config.optimization_level = OptimizationLevel::SpeedAndSize
                        }
                        // Profile names
                        "development" | "dev" => {
                            config.optimization_profile = Some("development".to_string())
                        }
                        "production" | "prod" => {
                            config.optimization_profile = Some("production".to_string())
                        }
                        "debug" => {
                            config.optimization_profile = Some("debug".to_string());
                            config.debug = true;
                        }
                        other => {
                            return Err(CompilerError::runtime_error(
                                format!("Unknown optimization level or profile: '{}'. Valid values: 0-3, none, speed, size, development, production, debug", other),
                                None,
                                None,
                            ));
                        }
                    }
                    i += 2;
                } else {
                    return Err(CompilerError::runtime_error(
                        "Optimization option requires a value (0-3, none, speed, size, development, production, debug)".to_string(),
                        None,
                        None,
                    ));
                }
            }
            "--memory-tier" => {
                if i + 1 < args.len() {
                    config.memory_tier = Some(
                        clean_language_compiler::MemoryTier::from_str(&args[i + 1]).ok_or_else(|| {
                            CompilerError::runtime_error(
                                format!(
                                    "Unknown memory tier: '{}'. Valid values: embedded, minimal, standard, heavy, canvas",
                                    args[i + 1]
                                ),
                                None,
                                None,
                            )
                        })?,
                    );
                    i += 2;
                } else {
                    return Err(CompilerError::runtime_error(
                        "Memory tier option requires a value (embedded, minimal, standard, heavy, canvas)".to_string(),
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
            // [P4c] Real opt-out escape hatch — falls back to the pre-
            // Delivery-2 pure-WASM JSON stdlib path in
            // src/stdlib/json_class.rs. Bridge dispatch (through
            // _json_decode_v2 / _json_encode_v2 / _json_encode_pretty_v2) is
            // the default as of 0.33.139. This flag will be removed in [P5].
            // See foundation/docs/governance/JSON_MIGRATION_DELIVERY2.md.
            "--enable-legacy-json-wasm" => {
                config.enable_legacy_json_wasm = true;
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
    // Format optimization info for display
    let opt_info = if let Some(ref profile) = config.optimization_profile {
        format!("profile: {}", profile)
    } else {
        format!("{:?}", config.optimization_level)
    };

    // Resolve memory tier: explicit flag > target-based default > standard
    let memory_tier = config
        .memory_tier
        .unwrap_or_else(|| clean_language_compiler::MemoryTier::default_for_target(&config.target));

    if config.verbose {
        println!("🔧 Compile Configuration:");
        println!("   Input: {}", config.input_file);
        println!("   Output: {}", config.output_file);
        println!("   Target: {}", config.target);
        println!("   Memory tier: {}", memory_tier);
        println!("   Runtime: {}", config.runtime);
        println!("   Optimization: {}", opt_info);
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

    // Apply optimization settings
    if let Some(ref profile_name) = config.optimization_profile {
        // Apply named profile
        let profile = match profile_name.as_str() {
            "development" | "dev" => OptimizationProfile::development(),
            "production" | "prod" => OptimizationProfile::production(),
            "size" => OptimizationProfile::size_optimized(),
            "speed" => OptimizationProfile::speed_optimized(),
            "debug" => OptimizationProfile::debug(),
            _ => OptimizationProfile::production(), // fallback
        };
        TargetOptimizer::apply_optimization_profile(&mut runtime_config, &profile);

        if config.verbose {
            println!("📊 Applied optimization profile: {}", profile);
        }
    } else {
        // Apply direct optimization level
        runtime_config.optimization_level = config.optimization_level;
    }

    // Apply target-specific optimizations (may override some settings)
    TargetOptimizer::optimize_for_target(&mut runtime_config, &target);

    // Validate configuration
    TargetManager::validate_target_runtime_compatibility(&target, &runtime_config)?;

    // Check for optimization warnings
    let warnings = TargetOptimizer::validate_optimization_settings(&target, &runtime_config);
    for warning in &warnings {
        eprintln!("⚠️ {}", warning);
    }

    if config.verbose {
        println!("🎯 Using target: {}", target);
        println!("⚙️ Runtime config: {:?}", runtime_config);
    }

    // Convert OptimizationLevel to u8 for compiler API
    let opt_level = match runtime_config.optimization_level {
        OptimizationLevel::None => 0,
        OptimizationLevel::Speed => 2,
        OptimizationLevel::SpeedAndSize => 3,
    };

    // Publish the JSON stdlib dispatch selector on the current thread so
    // the MIR builder and codegen bridge resolver see it during
    // compilation. As of 0.33.139 / [P4c] the default is bridge (ON);
    // --enable-legacy-json-wasm opts out to the pre-Delivery-2 pure-WASM
    // path in src/stdlib/json_class.rs. See
    // foundation/docs/governance/JSON_MIGRATION_DELIVERY2.md.
    clean_language_compiler::set_enable_json_bridge_override(!config.enable_legacy_json_wasm);

    // Perform compilation with external plugin support
    compile_file_with_opt_and_tier(
        &config.input_file,
        &config.output_file,
        opt_level,
        memory_tier,
    )?;

    // Reset the flag so it doesn't leak into subsequent compiles sharing
    // this thread (relevant when the CLI is embedded in test harnesses).
    clean_language_compiler::set_enable_json_bridge_override(true);

    // Generate bridge files based on target
    let bridge_target = match config.target.to_lowercase().as_str() {
        "browser" | "web" => Some(BridgeTarget::Browser),
        "node" | "nodejs" => Some(BridgeTarget::Node),
        "ios" | "macos" | "apple" => Some(BridgeTarget::iOS),
        "android" => Some(BridgeTarget::Android),
        "server" | "wasi" | "native" | "auto" => None, // Server/native targets don't need bridge files
        _ => None,
    };

    if let Some(bridge_target) = bridge_target {
        // Get output directory and wasm filename
        let output_path = Path::new(&config.output_file);
        let output_dir = output_path.parent().unwrap_or(Path::new("."));
        let wasm_filename = output_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "output.wasm".to_string());

        let generator = BridgeGenerator::new(bridge_target, output_dir, &wasm_filename);
        let result = generator.generate()?;

        if config.verbose || !result.generated_files.is_empty() {
            println!("🌉 Generated {} bridge files:", bridge_target.name());
            for file in &result.generated_files {
                println!("   → {}", file.display());
            }
        }
    }

    Ok(())
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

    // Compile with external plugin support
    // This automatically detects `import:` blocks and loads plugins from ~/.cleen/plugins/
    let wasm_bytes =
        clean_language_compiler::compile_with_external_plugins_and_opt_level(&source, file_path, 2)
            .map_err(|errors| {
                if let Some(first_error) = errors.first() {
                    first_error.clone()
                } else {
                    CompilerError::runtime_error(
                        "Compilation failed with unknown error".to_string(),
                        None,
                        None,
                    )
                }
            })?;

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
    println!("    build <input> [output]      Build multi-file project (resolves imports)");
    println!("    compile <input> [output]    Compile single Clean source to WebAssembly");
    println!("    run <input>                 Compile and run a Clean program");
    println!("    parse <input>               Parse and validate syntax only");
    println!("    check <input>               Type check without compilation");
    println!("    targets <subcommand>        Manage compilation targets");
    println!("    runtime <subcommand>        Manage WebAssembly runtimes");
    println!("    version                     Show version information");
    println!("    help                        Show this help message");
    println!();
    println!("BUILD OPTIONS:");
    println!("    --lib, -L <path>            Add library search path (can be repeated)");
    println!("    -O0, -O1, -O2, -O3          Optimization level (default: -O2)");
    println!("    --debug, -d                 Include debug information");
    println!("    --verbose, -v               Verbose output");
    println!();
    println!("COMPILE/RUN OPTIONS:");
    println!("    --target, -t <target>       Target platform (web, nodejs, native, embedded, wasi, auto)");
    println!("    --runtime, -r <runtime>     WebAssembly runtime (wasmtime, wasmer, auto)");
    println!("    --debug, -d                 Include debug information");
    println!("    --verbose, -v               Verbose output");
    println!("    --enable-legacy-json-wasm   Fall back to the pre-Delivery-2 pure-WASM JSON");
    println!("                                stdlib path (legacy). Use during Delivery-2 soak if");
    println!("                                the bridge path misbehaves in production. Default");
    println!("                                (flag unset) as of 0.33.139: bridge dispatch via");
    println!("                                _json_decode_v2/_json_encode_v2/_json_encode_pretty_v2.");
    println!("                                This flag will be removed in [P5].");
    println!();
    println!("OPTIMIZATION FLAGS:");
    println!("    -O0                         No optimization (fastest compilation)");
    println!("    -O1, -O2                    Speed optimization (default)");
    println!("    -O3                         Aggressive optimization (speed + size)");
    println!("    --optimization, -O <level>  Optimization level/profile:");
    println!("                                  0, 1, 2, 3    - Numeric levels");
    println!("                                  none          - No optimization");
    println!("                                  speed         - Optimize for speed");
    println!("                                  size          - Optimize for size");
    println!("                                  development   - Fast compilation, debug info");
    println!("                                  production    - Balanced optimization");
    println!("                                  debug         - Maximum debug info");
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
    println!("    cln build main.cln                              # Build multi-file project");
    println!("    cln build main.cln app.wasm -L ./lib            # Build with custom lib path");
    println!(
        "    cln compile hello.cln                           # Basic compilation (default -O2)"
    );
    println!("    cln compile hello.cln -O3                       # Aggressive optimization");
    println!("    cln compile hello.cln -O0 --debug               # Debug build, no optimization");
    println!("    cln compile hello.cln -O production             # Production profile");
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
    compile_file_with_opt(input_file, output_file, 2) // Default to -O2
}

fn compile_file_with_opt(
    input_file: &str,
    output_file: &str,
    opt_level: u8,
) -> Result<(), CompilerError> {
    compile_file_with_opt_and_tier(
        input_file,
        output_file,
        opt_level,
        clean_language_compiler::MemoryTier::Standard,
    )
}

fn compile_file_with_opt_and_tier(
    input_file: &str,
    output_file: &str,
    opt_level: u8,
    memory_tier: clean_language_compiler::MemoryTier,
) -> Result<(), CompilerError> {
    println!("🔨 Compiling {input_file} → {output_file}");

    // Get the input file path and its directory for search paths
    let input_path = std::path::Path::new(input_file);
    let search_paths = if let Some(parent) = input_path.parent() {
        if parent.as_os_str().is_empty() {
            vec![std::path::PathBuf::from(".")]
        } else {
            vec![parent.to_path_buf()]
        }
    } else {
        vec![std::path::PathBuf::from(".")]
    };

    // Use multi-file compilation to support file path imports
    // This automatically handles `import "path/to/file.cln"` syntax
    // as well as module imports like `import Math`
    let wasm_binary = clean_language_compiler::compile_multi_file_with_memory_tier(
        input_path,
        search_paths,
        opt_level,
        Some(memory_tier),
        memory_tier,
    )
    .map_err(|errors| {
        let source = fs::read_to_string(input_file).unwrap_or_default();
        for error in &errors {
            display_error(error, &source, input_file);
        }
        errors.into_iter().next().unwrap()
    })?;

    // Write the output file
    fs::write(output_file, wasm_binary).map_err(|e| {
        CompilerError::io_error(format!("Failed to write output file: {e}"), None, None)
    })?;

    // Stamp plugin.toml with the compiler version when compiling a plugin.
    // Look for plugin.toml next to the output file or one directory up.
    stamp_plugin_toml_if_present(output_file);

    println!("✅ Compilation successful! Generated {output_file}");
    Ok(())
}

/// After a successful compile, if a plugin.toml is found adjacent to the output
/// WASM, stamp `built_with_compiler` into its [build] section.  This lets the
/// loader detect stale plugins at load time.
fn stamp_plugin_toml_if_present(output_file: &str) {
    let out_path = std::path::Path::new(output_file);
    let candidates = [
        out_path.parent().and_then(|p| Some(p.join("plugin.toml"))),
        out_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| Some(p.join("plugin.toml"))),
    ];

    for candidate in candidates.iter().flatten() {
        if candidate.exists() {
            if let Err(e) = write_build_stamp(candidate) {
                eprintln!("warning: could not stamp {}: {}", candidate.display(), e);
            } else {
                eprintln!(
                    "[Plugin Build] Stamped {} with compiler {}",
                    candidate.display(),
                    env!("CARGO_PKG_VERSION")
                );
            }
            return;
        }
    }
}

/// Writes or replaces `built_with_compiler = "<ver>"` inside the [build] section
/// of a plugin.toml without disturbing any other content or comments.
fn write_build_stamp(toml_path: &std::path::Path) -> std::io::Result<()> {
    let content = fs::read_to_string(toml_path)?;
    let version = env!("CARGO_PKG_VERSION");
    let stamp_line = format!("built_with_compiler = \"{}\"", version);

    let updated = if content.contains("built_with_compiler") {
        // Replace existing stamp line wherever it appears
        content
            .lines()
            .map(|l| {
                if l.trim_start().starts_with("built_with_compiler") {
                    stamp_line.clone()
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else if content.contains("[build]") {
        // Insert after [build] header
        content
            .lines()
            .flat_map(|l| {
                if l.trim() == "[build]" {
                    vec![l.to_string(), stamp_line.clone()]
                } else {
                    vec![l.to_string()]
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        // Append a new [build] section at the end
        format!("{}\n\n[build]\n{}\n", content.trim_end(), stamp_line)
    };

    fs::write(toml_path, updated)
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

    // Use the plugin-aware type-check path so that bridge functions declared in any
    // `plugins:` block (e.g. `frame.canvas`) are registered before name resolution.
    // This matches the full compile pipeline and ensures calls like `_canvas_init`
    // or `_canvas_get_delta_time` are recognised as valid external functions.
    clean_language_compiler::type_check_with_external_plugins(&source, input_file).map_err(
        |errors| {
            for error in &errors {
                display_error(error, &source, input_file);
            }
            // Display a summary line that matches the format users expect
            let error_count = errors.len();
            let mut category_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for e in &errors {
                let category = match e {
                    clean_language_compiler::error::CompilerError::ValidationError { .. } => {
                        "validation"
                    }
                    clean_language_compiler::error::CompilerError::TypeError { .. } => "type",
                    clean_language_compiler::error::CompilerError::ParseError { .. } => "parse",
                    clean_language_compiler::error::CompilerError::LexError(_) => "lex",
                    clean_language_compiler::error::CompilerError::PluginError { .. } => "plugin",
                    _ => "other",
                };
                *category_counts.entry(category.to_string()).or_insert(0) += 1;
            }
            let summary_parts: Vec<String> = category_counts
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect();
            eprintln!(
                "\nSummary: {} error{} found\n    {}",
                error_count,
                if error_count == 1 { "" } else { "s" },
                summary_parts.join("\n    ")
            );
            errors.into_iter().next().unwrap()
        },
    )?;

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
    let runner = find_clean_runner()?;

    let status = std::process::Command::new(&runner)
        .arg(wasm_file)
        .status()
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to launch clean-runner '{runner}': {e}"),
                None,
                None,
            )
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(CompilerError::runtime_error(
            format!(
                "clean-runner exited with code {}",
                status.code().unwrap_or(-1)
            ),
            None,
            None,
        ))
    }
}

fn find_clean_runner() -> Result<String, CompilerError> {
    // 1. cleen-managed location: ~/.cleen/clean-runner/<version>/clean-runner
    if let Some(home) = std::env::var_os("HOME") {
        let base = std::path::Path::new(&home)
            .join(".cleen")
            .join("clean-runner");
        if base.exists() {
            // Pick the highest semver directory present
            if let Ok(entries) = std::fs::read_dir(&base) {
                let mut versions: Vec<String> = entries
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect();
                versions.sort_by(|a, b| {
                    let parse = |s: &str| -> (u32, u32, u32) {
                        let mut parts = s.trim_start_matches('v').split('.');
                        let major = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        let minor = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        let patch = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                        (major, minor, patch)
                    };
                    parse(b).cmp(&parse(a))
                });
                for ver in &versions {
                    let candidate = base.join(ver).join("clean-runner");
                    if candidate.exists() {
                        return Ok(candidate.to_string_lossy().into_owned());
                    }
                    #[cfg(windows)]
                    {
                        let win = base.join(ver).join("clean-runner.exe");
                        if win.exists() {
                            return Ok(win.to_string_lossy().into_owned());
                        }
                    }
                }
            }
        }
    }

    // 2. Fall back to PATH
    if which_clean_runner().is_some() {
        return Ok("clean-runner".to_string());
    }

    Err(CompilerError::runtime_error(
        "clean-runner not found. Install it with: cleen runner install latest".to_string(),
        None,
        None,
    ))
}

fn which_clean_runner() -> Option<()> {
    std::process::Command::new("clean-runner")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| ())
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
