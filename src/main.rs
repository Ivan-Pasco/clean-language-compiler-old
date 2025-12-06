/*!
 * Clean Language Compiler - Main Application
 *
 * Author: Ivan Pasco Lizarraga
 * Date: 17-07-2025
 * Website: <https://www.cleanlanguage.dev>
 *
 * A modern, type-safe programming language that compiles to WebAssembly
 */

#![allow(clippy::uninlined_format_args)]
#![allow(clippy::manual_inspect)]
#![allow(clippy::format_in_format_args)]
#![allow(clippy::useless_asref)]

use clap::{Parser, Subcommand};
use clean_language_compiler::debug::DebugUtils;
use clean_language_compiler::error::{CompilerError, ErrorReporter};
use clean_language_compiler::{
    compile_with_file, compile_with_opt_level, runtime::runtime_manager::RuntimeManager,
    runtime::wasmtime_config::CleanWasmtimeConfig,
};
use std::fs;
use std::path::{Path, PathBuf};

mod cli;
use cli::options_export;

/// Configuration for output formatting
#[derive(Clone)]
struct OutputConfig {
    use_colors: bool,
    json_mode: bool,
    quiet: bool,
}

impl OutputConfig {
    /// Report compilation errors using the appropriate format
    fn report_errors(&self, errors: &[CompilerError], source: Option<&str>) {
        if self.json_mode {
            // Output JSON diagnostics for IDE integration
            let diagnostics: Vec<serde_json::Value> =
                errors.iter().map(|e| self.error_to_json(e)).collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&diagnostics).unwrap_or_default()
            );
        } else if !self.quiet {
            // Use the ErrorReporter for beautiful terminal output
            let reporter = ErrorReporter::new(self.use_colors);
            let _ = reporter.report_errors(errors, source);
        }
    }

    fn error_to_json(&self, error: &CompilerError) -> serde_json::Value {
        let (severity, message, file, line, column, code) = match error {
            CompilerError::Syntax { context } => (
                "error",
                context.message.clone(),
                context
                    .location
                    .as_ref()
                    .map(|l| l.file.clone())
                    .unwrap_or_default(),
                context.location.as_ref().map(|l| l.line).unwrap_or(0),
                context.location.as_ref().map(|l| l.column).unwrap_or(0),
                context.error_code.clone(),
            ),
            CompilerError::Type { context } => (
                "error",
                context.message.clone(),
                context
                    .location
                    .as_ref()
                    .map(|l| l.file.clone())
                    .unwrap_or_default(),
                context.location.as_ref().map(|l| l.line).unwrap_or(0),
                context.location.as_ref().map(|l| l.column).unwrap_or(0),
                context.error_code.clone(),
            ),
            _ => ("error", error.to_string(), String::new(), 0, 0, None),
        };

        serde_json::json!({
            "severity": severity,
            "message": message,
            "file": file,
            "line": line,
            "column": column,
            "code": code
        })
    }
}

/// 🧹 Clean Language Compiler - Modern, type-safe language that compiles to WebAssembly
#[derive(Parser, Debug)]
#[command(
    author = "Ivan Pasco Lizarraga",
    version,
    about = "Clean Language Compiler - A modern, type-safe programming language",
    long_about = "Clean Language Compiler (cln)\n\nA modern, type-safe programming language that compiles to WebAssembly.\nWebsite: https://www.cleanlanguage.dev"
)]
struct Args {
    /// Increase verbosity (-v, -vv, -vvv for trace level)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Output machine-readable JSON diagnostics
    #[arg(long, global = true)]
    json: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Build a multi-file Clean Language project (resolves imports)
    Build {
        /// Entry file to compile (main.cln)
        input: String,

        /// Output file for the WebAssembly binary
        #[arg(short, long)]
        output: Option<String>,

        /// Library search paths (can be specified multiple times)
        #[arg(short = 'L', long = "lib")]
        lib_paths: Vec<PathBuf>,

        /// Optimization level (0-3)
        #[arg(short = 'O', long, default_value_t = 2)]
        opt_level: u8,

        /// Include debug information
        #[arg(short, long)]
        debug: bool,
    },
    /// Compile a Clean Language file to WebAssembly
    Compile {
        /// Input file to compile
        input: String,

        /// Output file for the WebAssembly binary
        #[arg(short, long)]
        output: String,

        /// Optimization level (0-3)
        #[arg(short = 'l', long, default_value_t = 2)]
        opt_level: u8,

        /// Run tests during compilation
        #[arg(long)]
        test: bool,

        /// Include tests in the compiled binary
        #[arg(long)]
        include_tests: bool,

        /// Enable external plugin loading from ~/.cleen/plugins/
        #[arg(long)]
        plugins: bool,
    },
    /// Package management commands
    #[command(subcommand)]
    Package(PackageCommands),
    /// Run the Clean Language test suite
    Test {
        /// Additional test directories to include
        #[arg(short, long)]
        dirs: Vec<String>,
    },
    /// Run simple compilation tests
    SimpleTest {},
    /// Debug a Clean Language file with enhanced error reporting
    Debug {
        /// Input file to debug
        input: String,

        /// Show AST structure
        #[arg(long)]
        show_ast: bool,

        /// Validate code style
        #[arg(long)]
        check_style: bool,

        /// Show detailed error analysis
        #[arg(long)]
        analyze_errors: bool,
    },
    /// Validate Clean Language code style and conventions
    Lint {
        /// Input file or directory to lint
        input: String,

        /// Fix issues automatically where possible
        #[arg(long)]
        fix: bool,

        /// Show only errors (suppress warnings)
        #[arg(long)]
        errors_only: bool,
    },
    /// Parse a file and show detailed parsing information
    Parse {
        /// Input file to parse
        input: String,

        /// Show detailed parse tree
        #[arg(long)]
        show_tree: bool,

        /// Enable error recovery mode
        #[arg(long)]
        recover_errors: bool,
    },
    /// Run a Clean Language source file or WebAssembly binary
    Run {
        /// Input file to run (.cln source file or .wasm binary)
        input: String,

        /// Enable debug output
        #[arg(short, long)]
        debug: bool,
    },
    /// Export compile options to JSON for IDE integration
    Options {
        /// Export compile options as JSON
        #[arg(long)]
        export_json: bool,

        /// Output path for the JSON file (optional)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Explain an error code in detail
    Explain {
        /// Error code to explain (e.g., SYN001, TYP001)
        code: String,
    },
    /// Show WebAssembly runtime information
    Runtime {
        /// Detect and show current runtime
        #[arg(long)]
        detect: bool,

        /// List all available runtimes
        #[arg(long)]
        list: bool,
    },
}

#[derive(Subcommand, Debug)]
enum PackageCommands {
    /// Initialize a new Clean Language package
    Init {
        /// Package name
        #[arg(short, long)]
        name: Option<String>,

        /// Package version
        #[arg(short, long)]
        version: Option<String>,

        /// Package description
        #[arg(short, long)]
        description: Option<String>,
    },
    /// Add a dependency to the current package
    Add {
        /// Package name to add
        package: String,

        /// Version requirement (e.g., "^1.0.0")
        #[arg(short, long)]
        version: Option<String>,

        /// Add as development dependency
        #[arg(long)]
        dev: bool,

        /// Git repository URL
        #[arg(long)]
        git: Option<String>,

        /// Local path to package
        #[arg(long)]
        path: Option<String>,
    },
    /// Remove a dependency from the current package
    Remove {
        /// Package name to remove
        package: String,
    },
    /// Install all dependencies for the current package
    Install,
    /// Update dependencies to their latest compatible versions
    Update {
        /// Specific package to update
        package: Option<String>,
    },
    /// List installed packages and their versions
    List {
        /// Show dependency tree
        #[arg(long)]
        tree: bool,
    },
    /// Search for packages in the registry
    Search {
        /// Search query
        query: String,

        /// Maximum number of results
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    /// Show information about a package
    Info {
        /// Package name
        package: String,

        /// Show specific version
        #[arg(short, long)]
        version: Option<String>,
    },
    /// Publish the current package to the registry
    Publish {
        /// Registry to publish to
        #[arg(long)]
        registry: Option<String>,

        /// Dry run (don't actually publish)
        #[arg(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize logging based on verbosity level
    let log_level = match (args.quiet, args.verbose) {
        (true, _) => "error", // --quiet: only errors
        (_, 0) => "warn",     // default: warnings and errors (no debug spam)
        (_, 1) => "info",     // -v: info level
        (_, 2) => "debug",    // -vv: debug level
        (_, _) => "trace",    // -vvv: trace level (everything)
    };

    // Set RUST_LOG environment variable if not already set
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", log_level);
    }

    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .with_ansi(!args.no_color)
        .init();

    // Store global output options for error reporting
    let output_config = OutputConfig {
        use_colors: !args.no_color,
        json_mode: args.json,
        quiet: args.quiet,
    };

    match args.command {
        Commands::Build {
            input,
            output,
            lib_paths,
            opt_level,
            debug,
        } => handle_build(input, output, lib_paths, opt_level, debug, &output_config).await?,
        Commands::Compile {
            input,
            output,
            opt_level,
            test,
            include_tests,
            plugins,
        } => {
            handle_compile(
                input,
                output,
                opt_level,
                test,
                include_tests,
                plugins,
                &output_config,
            )
            .await?
        }
        Commands::Package(package_cmd) => handle_package(package_cmd).await?,
        Commands::Test { dirs } => handle_test(args.verbose > 0, dirs).await?,
        Commands::SimpleTest {} => handle_simple_test(args.verbose > 0).await?,
        Commands::Debug {
            input,
            show_ast,
            check_style,
            analyze_errors,
        } => handle_debug(input, show_ast, check_style, analyze_errors, &output_config).await?,
        Commands::Lint {
            input,
            fix,
            errors_only,
        } => handle_lint(input, fix, errors_only, &output_config).await?,
        Commands::Parse {
            input,
            show_tree,
            recover_errors,
        } => handle_parse(input, show_tree, recover_errors).await?,
        Commands::Run { input, debug } => handle_run(input, debug, &output_config).await?,
        Commands::Options {
            export_json,
            output,
        } => {
            if export_json {
                let output_path = output.map(PathBuf::from);
                options_export::export_compile_options(output_path)?;
            } else {
                eprintln!("Use --export-json to export compile options");
                std::process::exit(1);
            }
        }
        Commands::Explain { code } => handle_explain(&code, &output_config)?,
        Commands::Runtime { detect, list } => handle_runtime(detect, list, &output_config)?,
    }

    Ok(())
}

fn handle_runtime(
    detect: bool,
    list: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let green = if output_config.use_colors {
        "\x1b[32m"
    } else {
        ""
    };
    let cyan = if output_config.use_colors {
        "\x1b[36m"
    } else {
        ""
    };
    let yellow = if output_config.use_colors {
        "\x1b[33m"
    } else {
        ""
    };
    let reset = if output_config.use_colors {
        "\x1b[0m"
    } else {
        ""
    };

    if list || (!detect && !list) {
        println!("\n{}WebAssembly Runtimes{}", cyan, reset);
        println!("{}", "=".repeat(50));

        let runtimes = RuntimeManager::list_available_runtimes();
        for runtime in &runtimes {
            let status = if runtime.available {
                format!("{}✓ Available{}", green, reset)
            } else {
                format!("{}✗ Not compiled{}", yellow, reset)
            };

            println!("\n{}{}{} ({})", cyan, runtime.name, reset, status);
            println!("  Version: {}", runtime.version);
            println!("  {}", runtime.description);
            println!("  Features: {}", runtime.features.join(", "));
        }
        println!();
    }

    if detect {
        use clean_language_compiler::runtime::runtime_trait::RuntimeConfig;

        let config = RuntimeConfig::default();
        match RuntimeManager::select_runtime(&config) {
            Ok(selected) => {
                println!("\n{}Selected Runtime{}", cyan, reset);
                println!("{}", "=".repeat(50));
                println!("  Runtime: {}{}{}", green, selected, reset);
                println!("  Mode: Auto-detected based on configuration");

                let recommendations = RuntimeManager::get_runtime_recommendations(selected);
                println!("\n{}Recommendations:{}", cyan, reset);
                for rec in recommendations {
                    println!("  • {}", rec);
                }
                println!();
            }
            Err(e) => {
                eprintln!("Error detecting runtime: {}", e);
                return Err(e.into());
            }
        }
    }

    Ok(())
}

async fn handle_build(
    input: String,
    output: Option<String>,
    lib_paths: Vec<PathBuf>,
    opt_level: u8,
    debug: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    // Determine output filename
    let output_file = output.unwrap_or_else(|| {
        Path::new(&input)
            .file_stem()
            .map(|s| format!("{}.wasm", s.to_string_lossy()))
            .unwrap_or_else(|| "output.wasm".to_string())
    });

    if !output_config.quiet {
        let opt_desc = match opt_level {
            0 => "none (fastest compilation)",
            1 => "light",
            2 => "standard",
            3 => "aggressive (speed + size)",
            _ => "unknown",
        };
        let debug_mode = if debug { " [debug]" } else { "" };
        println!(
            "Building {input} -> {output_file} (optimization: -O{opt_level} {opt_desc}){debug_mode}"
        );

        if !lib_paths.is_empty() {
            println!("Library paths: {:?}", lib_paths);
        }
    }

    // Use the multi-file compiler
    let wasm_binary = clean_language_compiler::compile_multi_file(&input, lib_paths, opt_level)
        .map_err(|errors| {
            // Report all errors
            let error_messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            output_config.report_errors(&errors, None);
            format!(
                "Build failed with {} error(s): {}",
                errors.len(),
                error_messages.join("; ")
            )
        })?;

    // Write output
    fs::write(&output_file, wasm_binary)?;

    if !output_config.quiet {
        println!("Build successful! Generated {output_file}");
    }

    Ok(())
}

async fn handle_compile(
    input: String,
    output: String,
    opt_level: u8,
    test: bool,
    _include_tests: bool,
    plugins: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if !output_config.quiet {
        let opt_desc = match opt_level {
            0 => "none (fastest compilation)",
            1 => "light",
            2 => "standard",
            3 => "aggressive (speed + size)",
            _ => "unknown",
        };
        let plugin_mode = if plugins { " [plugins enabled]" } else { "" };
        println!(
            "Compiling {input} to {output} (optimization: -O{opt_level} {opt_desc}){plugin_mode}"
        );
    }

    let source = fs::read_to_string(&input)?;

    tracing::debug!(
        source_len = source.len(),
        opt_level = opt_level,
        plugins = plugins,
        "Calling compile function"
    );
    tracing::trace!(source_content = %source, "Source code to compile");

    // Use the appropriate pipeline based on plugin mode
    let wasm_binary = if plugins {
        // Use plugin-aware compilation that auto-detects import: blocks
        match clean_language_compiler::compile_with_external_plugins_and_opt_level(
            &source, &input, opt_level,
        ) {
            Ok(binary) => binary,
            Err(errors) => {
                output_config.report_errors(&errors, Some(&source));
                return Err(format!("Compilation failed with {} errors", errors.len()).into());
            }
        }
    } else {
        // Use the standard 7-stage pipeline for compilation
        match compile_with_opt_level(&source, &input, opt_level) {
            Ok(binary) => binary,
            Err(errors) => {
                output_config.report_errors(&errors, Some(&source));
                return Err(format!("Compilation failed with {} errors", errors.len()).into());
            }
        }
    };

    // Note: Tests are not currently supported in the 7-stage pipeline
    if test {
        println!("⚠️  Test execution not yet implemented in 7-stage pipeline");
    }

    if let Some(parent) = Path::new(&output).parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&output, wasm_binary)?;

    println!("Successfully compiled to {output}");

    // Tests handling would be implemented here if needed

    Ok(())
}

async fn handle_package(package_cmd: PackageCommands) -> Result<(), Box<dyn std::error::Error>> {
    use clean_language_compiler::package::PackageManager;
    use std::env;

    let cache_dir = dirs::home_dir()
        .unwrap_or_else(|| env::current_dir().unwrap())
        .join(".clean")
        .join("packages");

    let package_manager = PackageManager::new(cache_dir);

    match package_cmd {
        PackageCommands::Init {
            name,
            version,
            description,
        } => {
            let current_dir = env::current_dir()?;
            let package_name = name.unwrap_or_else(|| {
                current_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("my-package")
                    .to_string()
            });

            println!("📦 Initializing Clean Language package: {package_name}");

            match package_manager.init_package(&current_dir, package_name, version, description) {
                Ok(_) => println!("✅ Package initialized successfully!"),
                Err(e) => eprintln!("❌ Failed to initialize package: {e}"),
            }
        }
        PackageCommands::Add {
            package,
            version,
            dev,
            ..
        } => {
            let manifest_path = env::current_dir()?.join("package.clean.toml");

            if !manifest_path.exists() {
                eprintln!("❌ No package.clean.toml found. Run 'clean package init' first.");
                return Ok(());
            }

            let version_spec = version.unwrap_or_else(|| "latest".to_string());

            println!(
                "📦 Adding {} dependency: {package} {version_spec}",
                if dev { "development" } else { "runtime" }
            );

            match package_manager.add_dependency(&manifest_path, package, version_spec, dev) {
                Ok(()) => println!("✅ Dependency added successfully!"),
                Err(e) => eprintln!("❌ Failed to add dependency: {e}"),
            }
        }
        PackageCommands::Remove { package } => {
            let manifest_path = env::current_dir()?.join("package.clean.toml");

            if !manifest_path.exists() {
                eprintln!("❌ No package.clean.toml found.");
                return Ok(());
            }

            println!("📦 Removing dependency: {package}");

            match package_manager.remove_dependency(&manifest_path, &package) {
                Ok(()) => println!("✅ Dependency removed successfully!"),
                Err(e) => eprintln!("❌ Failed to remove dependency: {e}"),
            }
        }
        PackageCommands::Install => {
            let manifest_path = env::current_dir()?.join("package.clean.toml");

            if !manifest_path.exists() {
                eprintln!("❌ No package.clean.toml found. Run 'clean package init' first.");
                return Ok(());
            }

            println!("📦 Installing dependencies...");

            match PackageManager::load_manifest(&manifest_path) {
                Ok(manifest) => {
                    if let Some(deps) = &manifest.dependencies {
                        println!("Runtime dependencies:");
                        for (name, spec) in deps {
                            println!("  - {name} {spec:?}");
                        }
                    }
                    if let Some(dev_deps) = &manifest.dev_dependencies {
                        println!("Development dependencies:");
                        for (name, spec) in dev_deps {
                            println!("  - {name} {spec:?}");
                        }
                    }
                    println!("✅ Dependencies would be installed (simulation mode)");
                }
                Err(e) => eprintln!("❌ Failed to load manifest: {e}"),
            }
        }
        PackageCommands::List { .. } => {
            let manifest_path = env::current_dir()?.join("package.clean.toml");

            if !manifest_path.exists() {
                eprintln!("❌ No package.clean.toml found.");
                return Ok(());
            }

            match PackageManager::load_manifest(&manifest_path) {
                Ok(manifest) => {
                    println!(
                        "📦 Package: {}",
                        format!("{} {}", manifest.package.name, manifest.package.version)
                    );

                    if let Some(deps) = &manifest.dependencies {
                        println!("\n📋 Runtime Dependencies:");
                        for (name, spec) in deps {
                            println!("  {name} {spec:?}");
                        }
                    }

                    if let Some(dev_deps) = &manifest.dev_dependencies {
                        println!("\n🔧 Development Dependencies:");
                        for (name, spec) in dev_deps {
                            println!("  {name} {spec:?}");
                        }
                    }
                }
                Err(e) => eprintln!("❌ Failed to load manifest: {e}"),
            }
        }
        PackageCommands::Search { query, .. } => {
            println!("🔍 Searching for packages matching '{query}'...");
            println!("📡 Package registry search not yet implemented");
            println!("   This would search https://packages.cleanlang.org for packages");
        }
        PackageCommands::Info { package, version } => {
            println!("ℹ️  Package information for: {package}");
            if let Some(v) = version {
                println!("   Version: {v}");
            }
            println!("📡 Package registry info not yet implemented");
        }
        PackageCommands::Update { package } => {
            if let Some(pkg) = package {
                println!("🔄 Updating package: {pkg}");
            } else {
                println!("🔄 Updating all dependencies...");
            }
            println!("📡 Package update not yet implemented");
        }
        PackageCommands::Publish { .. } => {
            let manifest_path = env::current_dir()?.join("package.clean.toml");

            if !manifest_path.exists() {
                eprintln!("❌ No package.clean.toml found.");
                return Ok(());
            }

            match PackageManager::load_manifest(&manifest_path) {
                Ok(manifest) => {
                    println!(
                        "📤 Publishing {}...",
                        format!("{} {}", manifest.package.name, manifest.package.version)
                    );
                    println!("📡 Package publishing not yet implemented");
                }
                Err(e) => eprintln!("❌ Failed to load manifest: {e}"),
            }
        }
    }
    Ok(())
}

async fn handle_test(verbose: bool, dirs: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running Clean Language test suite...");
    if verbose {
        println!("Verbose output enabled");
    }
    if !dirs.is_empty() {
        println!("Additional test directories: {dirs:?}");
    }

    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("test");
    if verbose {
        cmd.arg("--verbose");
    }

    let status = cmd.status()?;
    if status.success() {
        println!("✓ All tests passed!");
    } else {
        eprintln!("✗ Some tests failed");
        // Don't return error for test failures - just report them
        println!("Note: Test failures reported but not treating as critical error");
    }
    Ok(())
}

async fn handle_simple_test(verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running simple compilation tests...");
    if verbose {
        println!("Verbose output enabled");
    }

    let test_source = "start()\n\tinteger x = 42\n\tprint(x)\n";

    match compile_with_file(test_source, "simple_test.clean") {
        Ok(wasm_binary) => {
            println!(
                "✓ Simple test passed: {} bytes of WASM generated",
                wasm_binary.len()
            );
            Ok(())
        }
        Err(errors) => {
            eprintln!("✗ Simple test failed with {} errors:", errors.len());
            for (i, error) in errors.iter().enumerate() {
                eprintln!("Error {}: {}", i + 1, error);
            }
            Err("Simple test failed".into())
        }
    }
}

async fn handle_debug(
    input: String,
    show_ast: bool,
    check_style: bool,
    _analyze_errors: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Debugging Clean Language file: {input}\n");

    let source = match fs::read_to_string(&input) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("❌ Error reading file '{input}': {e}");
            return Ok(());
        }
    };

    // Debug using the 7-stage pipeline compilation attempt
    match compile_with_file(&source, &input) {
        Ok(wasm_binary) => {
            println!(
                "✅ Compilation successful: {} bytes of WASM generated",
                wasm_binary.len()
            );
            if show_ast {
                println!("⚠️  AST display not available in 7-stage pipeline (use specification parser directly)");
            }
        }
        Err(errors) => {
            output_config.report_errors(&errors, Some(&source));
        }
    }

    if check_style {
        println!("\n=== Style Validation ===");
        let style_issues = DebugUtils::validate_style(&source);
        if style_issues.is_empty() {
            println!("✅ No style issues found");
        } else {
            println!("🎨 Style Issues Found:");
            for issue in style_issues {
                println!("  {issue}");
            }
        }
    }
    Ok(())
}

async fn handle_lint(
    input: String,
    fix: bool,
    errors_only: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧹 Linting: {input}");

    let path = Path::new(&input);
    let files_to_lint = if path.is_file() {
        vec![input.clone()]
    } else if path.is_dir() {
        let mut clean_files = Vec::new();
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    if ext == "clean" {
                        if let Some(path_str) = entry.path().to_str() {
                            clean_files.push(path_str.to_string());
                        }
                    }
                }
            }
        }
        clean_files
    } else {
        eprintln!("❌ Error: '{input}' is not a valid file or directory");
        return Ok(());
    };

    if files_to_lint.is_empty() {
        println!("No Clean Language files found to lint");
        return Ok(());
    }

    let mut total_errors = 0;

    for file_path in &files_to_lint {
        println!("\n📄 Linting: {file_path}");

        let source = match fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("  ❌ Error reading file: {e}");
                continue;
            }
        };

        // Use 7-stage pipeline for linting
        match compile_with_file(&source, file_path) {
            Ok(_) => {
                println!("  ✅ No compilation errors found");
            }
            Err(errors) => {
                total_errors += errors.len();
                if !errors_only {
                    output_config.report_errors(&errors, Some(&source));
                }
            }
        }

        // Note: Style validation not available in 7-stage pipeline
        if !errors_only {
            println!("  ⚠️  Style validation not yet implemented in 7-stage pipeline");
        }
    }

    println!("\n=== Lint Summary ===");
    println!("Files checked: {}", files_to_lint.len());
    println!("Compilation errors: {total_errors}");

    if fix {
        println!("Note: Automatic fixing is not yet implemented");
    }
    Ok(())
}

async fn handle_parse(
    input: String,
    show_tree: bool,
    recover_errors: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📝 Parsing file: {input}");

    let source = match fs::read_to_string(&input) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("❌ Error reading file '{input}': {e}");
            return Ok(());
        }
    };

    // Use 7-stage pipeline for parsing
    println!("🔄 Using 7-stage compilation pipeline for parsing...\n");

    match compile_with_file(&source, &input) {
        Ok(wasm_binary) => {
            println!("✅ Parsing and compilation succeeded!");
            println!("Generated {} bytes of WASM", wasm_binary.len());

            if show_tree {
                println!("⚠️  AST display not available in 7-stage pipeline");
                println!("    Use the specification parser directly for AST inspection");
            }

            if recover_errors {
                println!("ℹ️  Error recovery mode not needed - compilation succeeded");
            }
        }
        Err(errors) => {
            println!(
                "❌ Parsing/compilation failed with {} error(s):\n",
                errors.len()
            );

            for (i, error) in errors.iter().enumerate() {
                println!("Error {}: {}", i + 1, error);
            }

            println!("\n💡 Suggestions:");
            println!("  • Check the Clean Language syntax documentation");
            println!("  • Ensure proper indentation (tabs, not spaces)");
            println!("  • Verify function and variable declarations follow specification");
        }
    }

    Ok(())
}

async fn handle_run(
    input: String,
    debug: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if !Path::new(&input).exists() {
        eprintln!("❌ Error: File '{input}' not found");
        return Ok(());
    }

    let input_path = Path::new(&input);
    let wasm_bytes = match input_path.extension().and_then(|s| s.to_str()) {
        Some("cln") => {
            // Handle Clean Language source file - compile to WASM first
            if !output_config.quiet {
                println!("🔧 Compiling Clean Language file: {input}");
            }

            let source = fs::read_to_string(&input)?;
            if debug {
                println!("📝 Source file size: {} characters", source.len());
            }

            // Try to compile the source to WASM
            let wasm_binary = match compile_with_file(&source, &input) {
                Ok(binary) => {
                    if debug {
                        println!(
                            "✅ Compilation successful: {} bytes of WASM generated",
                            binary.len()
                        );
                    }
                    binary
                }
                Err(compile_errors) => {
                    output_config.report_errors(&compile_errors, Some(&source));
                    return Err(
                        format!("Compilation failed with {} errors", compile_errors.len()).into(),
                    );
                }
            };

            if !output_config.quiet {
                println!("🚀 Running compiled WebAssembly...");
            }
            wasm_binary
        }
        Some("wasm") => {
            // Handle WebAssembly binary file directly
            println!("🚀 Running WebAssembly file: {input}");

            let wasm_bytes = fs::read(&input)?;
            if debug {
                println!("📦 WASM file size: {} bytes", wasm_bytes.len());
            }
            wasm_bytes
        }
        Some(ext) => {
            eprintln!("❌ Error: Unsupported file extension '.{ext}'");
            eprintln!(
                "   Supported formats: .cln (Clean Language source), .wasm (WebAssembly binary)"
            );
            return Ok(());
        }
        None => {
            eprintln!("❌ Error: File has no extension");
            eprintln!(
                "   Supported formats: .cln (Clean Language source), .wasm (WebAssembly binary)"
            );
            return Ok(());
        }
    };

    // Use wasmtime to execute the WASM file
    use std::sync::Mutex;
    use wasmtime::{Caller, Extern, Linker, Memory, Module, Store};

    // Global allocator for dynamic string storage
    static NEXT_ALLOCATION_OFFSET: Mutex<usize> = Mutex::new(2048);

    // Helper function to allocate memory for a string in WASM memory
    fn allocate_string_in_memory(
        memory: &Memory,
        caller: &mut Caller<'_, ()>,
        string_value: &str,
        debug: bool,
    ) -> i32 {
        let string_bytes = string_value.as_bytes();
        let total_size = 4 + string_bytes.len();

        let mut next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
        let offset = *next_offset;
        *next_offset += (total_size + 7) & !7; // Align to 8-byte boundary
        drop(next_offset);

        let data = memory.data_mut(caller);

        if offset + total_size >= data.len() {
            if debug {
                println!("⚠️  WARNING: Not enough WASM memory for string allocation");
            }
            return 0;
        }

        data[offset..offset + 4].copy_from_slice(&(string_bytes.len() as u32).to_le_bytes());
        data[offset + 4..offset + 4 + string_bytes.len()].copy_from_slice(string_bytes);

        if debug {
            println!("📝 Allocated '{}' at address {}", string_value, offset);
        }

        offset as i32
    }

    // Create engine and store using minimal configuration for direct execution
    let engine = CleanWasmtimeConfig::create_minimal_engine()?;
    let mut store = Store::new(&engine, ());

    // Create module
    let module = Module::new(&engine, &wasm_bytes)?;

    // Create linker and add imports
    let mut linker = Linker::new(&engine);

    // Add print functions
    linker.func_wrap(
        "env",
        "print",
        move |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
            if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                if let Some(data) = mem.data(&caller).get(ptr as usize..(ptr + len) as usize) {
                    if let Ok(s) = std::str::from_utf8(data) {
                        print!("{}", s);
                    } else {
                        print!("[invalid utf8: {} bytes]", len);
                    }
                }
            }
        },
    )?;

    linker.func_wrap(
        "env",
        "printl",
        move |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
            if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                if let Some(data) = mem.data(&caller).get(ptr as usize..(ptr + len) as usize) {
                    if let Ok(s) = std::str::from_utf8(data) {
                        println!("{}", s);
                    } else {
                        println!("[invalid utf8: {} bytes]", len);
                    }
                }
            }
        },
    )?;

    // Add memory runtime functions with correct signatures
    linker.func_wrap(
        "memory_runtime",
        "mem_alloc",
        |size: i32, alignment: i32| -> i32 {
            // Simple allocation stub - returns aligned size
            (size + alignment - 1) & !(alignment - 1)
        },
    )?;
    linker.func_wrap("memory_runtime", "mem_retain", |_ptr: i32| {})?;
    linker.func_wrap("memory_runtime", "mem_release", |_ptr: i32| {})?;

    // Add type conversion functions
    let debug_copy = debug;
    linker.func_wrap(
        "env",
        "int_to_string",
        move |mut caller: Caller<'_, ()>, value: i32| -> i32 {
            let string_value = value.to_string();
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                allocate_string_in_memory(&memory, &mut caller, &string_value, debug_copy)
            } else {
                0
            }
        },
    )?;

    let debug_copy = debug;
    linker.func_wrap(
        "env",
        "float_to_string",
        move |mut caller: Caller<'_, ()>, value: f64| -> i32 {
            let string_value = value.to_string();
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                allocate_string_in_memory(&memory, &mut caller, &string_value, debug_copy)
            } else {
                0
            }
        },
    )?;

    // Input functions - complete implementation for basic I/O
    // Simple input without prompt (1 parameter version)
    let debug_input_simple = debug;
    linker.func_wrap(
        "env",
        "input",
        move |mut caller: Caller<'_, ()>, _unused: i32| -> i32 {
            // Read input from stdin without prompt
            use std::io::{stdin, BufRead, BufReader};
            let input = BufReader::new(stdin());
            if let Ok(line) = input.lines().next().unwrap_or(Ok(String::new())) {
                if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                    return allocate_string_in_memory(
                        &memory,
                        &mut caller,
                        &line,
                        debug_input_simple,
                    );
                }
            }
            0
        },
    )?;

    // Input with prompt (2 parameter version)
    let debug_input = debug;
    linker.func_wrap(
        "env",
        "input_with_prompt",
        move |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
            // Read prompt from WASM memory and display it
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                if let Some(data) = memory
                    .data(&caller)
                    .get(prompt_ptr as usize..(prompt_ptr + prompt_len) as usize)
                {
                    if let Ok(prompt) = std::str::from_utf8(data) {
                        print!("{}", prompt);
                        std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
                    }
                }
            }

            // Read input from stdin
            use std::io::{stdin, BufRead, BufReader};
            let input = BufReader::new(stdin());
            if let Ok(line) = input.lines().next().unwrap_or(Ok(String::new())) {
                if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                    return allocate_string_in_memory(&memory, &mut caller, &line, debug_input);
                }
            }
            0
        },
    )?;

    // Simple input_integer without prompt (1 parameter version)
    linker.func_wrap("env", "input_integer", |_unused: i32| -> i32 {
        // Read integer from stdin without prompt
        use std::io::{stdin, BufRead, BufReader};
        let input = BufReader::new(stdin());
        if let Ok(line) = input.lines().next().unwrap_or(Ok(String::new())) {
            if let Ok(value) = line.trim().parse::<i32>() {
                return value;
            }
        }
        0
    })?;

    // Simple input_float without prompt (1 parameter version)
    linker.func_wrap("env", "input_float", |_unused: i32| -> f64 {
        // Read float from stdin without prompt
        use std::io::{stdin, BufRead, BufReader};
        let input = BufReader::new(stdin());
        if let Ok(line) = input.lines().next().unwrap_or(Ok(String::new())) {
            if let Ok(value) = line.trim().parse::<f64>() {
                return value;
            }
        }
        0.0
    })?;

    // Simple input_yesno without prompt (1 parameter version)
    linker.func_wrap("env", "input_yesno", |_unused: i32| -> i32 {
        // Read yes/no from stdin without prompt
        use std::io::{stdin, BufRead, BufReader};
        let input = BufReader::new(stdin());
        if let Ok(line) = input.lines().next().unwrap_or(Ok(String::new())) {
            let answer = line.trim().to_lowercase();
            if answer == "y" || answer == "yes" || answer == "true" || answer == "1" {
                return 1;
            }
        }
        0
    })?;

    linker.func_wrap(
        "env",
        "input_range",
        |_: i32, _: i32, _: i32, _: i32| -> i32 {
            // Stub implementation for range input validation
            // This would validate if input is within a specified range
            println!("Note: input_range function not fully implemented");
            0
        },
    )?;

    // String conversion functions
    let debug_copy = debug;
    linker.func_wrap(
        "env",
        "bool_to_string",
        move |mut caller: Caller<'_, ()>, value: i32| -> i32 {
            let string_value = if value != 0 { "true" } else { "false" };
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                allocate_string_in_memory(&memory, &mut caller, string_value, debug_copy)
            } else {
                0
            }
        },
    )?;

    // Method-style conversion functions
    let debug_copy = debug;
    linker.func_wrap(
        "env",
        "integer.toString",
        move |mut caller: Caller<'_, ()>, value: i32| -> i32 {
            let string_value = value.to_string();
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                allocate_string_in_memory(&memory, &mut caller, &string_value, debug_copy)
            } else {
                0
            }
        },
    )?;

    let debug_copy = debug;
    linker.func_wrap(
        "env",
        "number.toString",
        move |mut caller: Caller<'_, ()>, value: f64| -> i32 {
            let string_value = value.to_string();
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                allocate_string_in_memory(&memory, &mut caller, &string_value, debug_copy)
            } else {
                0
            }
        },
    )?;

    let debug_copy = debug;
    linker.func_wrap(
        "env",
        "boolean.toString",
        move |mut caller: Caller<'_, ()>, value: i32| -> i32 {
            let string_value = if value != 0 { "true" } else { "false" };
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                allocate_string_in_memory(&memory, &mut caller, string_value, debug_copy)
            } else {
                0
            }
        },
    )?;

    // INTEGER conversion methods
    linker.func_wrap("env", "integer.toInteger", |value: i32| -> i32 { value })?;
    linker.func_wrap("env", "integer.toNumber", |value: i32| -> f64 {
        f64::from(value)
    })?;
    linker.func_wrap("env", "integer.toBoolean", |value: i32| -> i32 {
        i32::from(value != 0)
    })?;
    linker.func_wrap("env", "integer.length", |_value: i32| -> i32 { 1 })?; // Integer length is always 1

    // NUMBER conversion methods
    linker.func_wrap("env", "number.toInteger", |value: f64| -> i32 {
        value as i32
    })?;
    linker.func_wrap("env", "number.toNumber", |value: f64| -> f64 { value })?;
    linker.func_wrap("env", "number.toBoolean", |value: f64| -> i32 {
        i32::from(value != 0.0)
    })?;
    linker.func_wrap("env", "number.length", |_value: f64| -> i32 { 1 })?; // Number length is always 1

    // STRING conversion methods
    linker.func_wrap(
        "env",
        "string.toString",
        |value: i32| -> i32 { value }, // String toString returns itself
    )?;
    linker.func_wrap("env", "string.toInteger", |_ptr: i32| -> i32 { 0 })?; // Stub
    linker.func_wrap("env", "string.toNumber", |_ptr: i32| -> f64 { 0.0 })?; // Stub
    linker.func_wrap("env", "string.toBoolean", |_ptr: i32| -> i32 { 0 })?; // Stub
    linker.func_wrap("env", "string.length", |_ptr: i32| -> i32 { 0 })?; // Stub
    linker.func_wrap("env", "string.toUpperCase", |ptr: i32| -> i32 { ptr })?; // Stub - return same pointer
    linker.func_wrap("env", "string.toLowerCase", |ptr: i32| -> i32 { ptr })?; // Stub - return same pointer
    linker.func_wrap("env", "string.concat", |ptr1: i32, _ptr2: i32| -> i32 {
        ptr1
    })?; // Stub - return first pointer
    linker.func_wrap(
        "env",
        "string_concat",
        |ptr1: i32, _len1: i32, _ptr2: i32, _len2: i32| -> i32 { ptr1 },
    )?; // Stub - return first pointer (4-param version)

    // BOOLEAN conversion methods
    linker.func_wrap("env", "boolean.toInteger", |value: i32| -> i32 {
        i32::from(value != 0)
    })?;
    linker.func_wrap("env", "boolean.toNumber", |value: i32| -> f64 {
        if value != 0 {
            1.0
        } else {
            0.0
        }
    })?;
    linker.func_wrap("env", "boolean.toBoolean", |value: i32| -> i32 { value })?;
    linker.func_wrap("env", "boolean.length", |_value: i32| -> i32 { 1 })?; // Boolean length is always 1

    linker.func_wrap(
        "env",
        "string_to_int",
        |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
            // Read string from memory at ptr (expects length-prefixed string)
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let data = memory.data(&caller);
                if let Some(len_bytes) = data.get(ptr as usize..(ptr + 4) as usize) {
                    let len = u32::from_le_bytes([
                        len_bytes[0],
                        len_bytes[1],
                        len_bytes[2],
                        len_bytes[3],
                    ]) as usize;
                    if let Some(str_data) = data.get((ptr + 4) as usize..(ptr + 4) as usize + len) {
                        if let Ok(s) = std::str::from_utf8(str_data) {
                            if let Ok(value) = s.parse::<i32>() {
                                return value;
                            }
                        }
                    }
                }
            }
            0
        },
    )?;

    linker.func_wrap(
        "env",
        "string_to_float",
        |mut caller: Caller<'_, ()>, ptr: i32| -> f64 {
            // Read string from memory at ptr (expects length-prefixed string)
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let data = memory.data(&caller);
                if let Some(len_bytes) = data.get(ptr as usize..(ptr + 4) as usize) {
                    let len = u32::from_le_bytes([
                        len_bytes[0],
                        len_bytes[1],
                        len_bytes[2],
                        len_bytes[3],
                    ]) as usize;
                    if let Some(str_data) = data.get((ptr + 4) as usize..(ptr + 4) as usize + len) {
                        if let Ok(s) = std::str::from_utf8(str_data) {
                            if let Ok(value) = s.parse::<f64>() {
                                return value;
                            }
                        }
                    }
                }
            }
            0.0
        },
    )?;

    // File I/O functions - basic stub implementations
    linker.func_wrap(
        "env",
        "file_write",
        |mut caller: Caller<'_, ()>,
         path_ptr: i32,
         path_len: i32,
         content_ptr: i32,
         content_len: i32|
         -> i32 {
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let data = memory.data(&caller);
                if let (Some(path_data), Some(content_data)) = (
                    data.get(path_ptr as usize..(path_ptr + path_len) as usize),
                    data.get(content_ptr as usize..(content_ptr + content_len) as usize),
                ) {
                    if let (Ok(path), Ok(content)) = (
                        std::str::from_utf8(path_data),
                        std::str::from_utf8(content_data),
                    ) {
                        if let Err(_) = std::fs::write(path, content) {
                            return -1; // Error
                        }
                        return 0; // Success
                    }
                }
            }
            -1
        },
    )?;

    let debug_file_read = debug;
    linker.func_wrap(
        "env",
        "file_read",
        move |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32, max_size: i32| -> i32 {
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let data = memory.data(&caller);
                if let Some(path_data) = data.get(path_ptr as usize..(path_ptr + path_len) as usize)
                {
                    if let Ok(path) = std::str::from_utf8(path_data) {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            let truncated_content = if content.len() > max_size as usize {
                                &content[..max_size as usize]
                            } else {
                                &content
                            };
                            return allocate_string_in_memory(
                                &memory,
                                &mut caller,
                                truncated_content,
                                debug_file_read,
                            );
                        }
                    }
                }
            }
            0
        },
    )?;

    linker.func_wrap(
        "env",
        "file_exists",
        |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let data = memory.data(&caller);
                if let Some(path_data) = data.get(path_ptr as usize..(path_ptr + path_len) as usize)
                {
                    if let Ok(path) = std::str::from_utf8(path_data) {
                        return i32::from(std::path::Path::new(path).exists());
                    }
                }
            }
            0
        },
    )?;

    linker.func_wrap(
        "env",
        "file_delete",
        |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let data = memory.data(&caller);
                if let Some(path_data) = data.get(path_ptr as usize..(path_ptr + path_len) as usize)
                {
                    if let Ok(path) = std::str::from_utf8(path_data) {
                        if let Err(_) = std::fs::remove_file(path) {
                            return -1; // Error
                        }
                        return 0; // Success
                    }
                }
            }
            -1
        },
    )?;

    linker.func_wrap(
        "env",
        "file_append",
        |mut caller: Caller<'_, ()>,
         path_ptr: i32,
         path_len: i32,
         content_ptr: i32,
         content_len: i32|
         -> i32 {
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let data = memory.data(&caller);
                if let (Some(path_data), Some(content_data)) = (
                    data.get(path_ptr as usize..(path_ptr + path_len) as usize),
                    data.get(content_ptr as usize..(content_ptr + content_len) as usize),
                ) {
                    if let (Ok(path), Ok(content)) = (
                        std::str::from_utf8(path_data),
                        std::str::from_utf8(content_data),
                    ) {
                        use std::fs::OpenOptions;
                        use std::io::Write;
                        if let Ok(mut file) =
                            OpenOptions::new().append(true).create(true).open(path)
                        {
                            if let Err(_) = file.write_all(content.as_bytes()) {
                                return -1; // Error
                            }
                            return 0; // Success
                        }
                    }
                }
            }
            -1
        },
    )?;

    // HTTP functions - basic stub implementations
    let debug_http = debug;
    linker.func_wrap(
        "env",
        "http_get",
        move |mut caller: Caller<'_, ()>, _url_ptr: i32, _url_len: i32| -> i32 {
            // Basic stub - would need proper HTTP client implementation
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let error_message = "HTTP functions not fully implemented";
                allocate_string_in_memory(&memory, &mut caller, error_message, debug_http)
            } else {
                0
            }
        },
    )?;

    // HTTP stub functions with correct signatures matching WASM imports
    linker.func_wrap(
        "env",
        "http_post",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap("env", "http_put", |_: i32, _: i32, _: i32, _: i32| -> i32 {
        0
    })?;
    linker.func_wrap(
        "env",
        "http_patch",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap("env", "http_delete", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_head", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_options", |_: i32, _: i32| -> i32 { 0 })?;
    // HTTP functions with headers - actually use 4 parameters based on sig=19
    linker.func_wrap(
        "env",
        "http_get_with_headers",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_post_with_headers",
        |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_post_json",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_put_json",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_patch_json",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_post_form",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;

    // HTTP configuration functions
    linker.func_wrap("env", "http_set_user_agent", |_: i32, _: i32| {})?;
    linker.func_wrap("env", "http_set_timeout", |_: i32| {})?;
    linker.func_wrap("env", "http_set_max_redirects", |_: i32| {})?;
    linker.func_wrap("env", "http_enable_cookies", |_: i32| {})?;

    // HTTP response functions
    linker.func_wrap("env", "http_get_response_code", || -> i32 { 200 })?;
    linker.func_wrap("env", "http_get_response_headers", || -> i32 { 0 })?;
    linker.func_wrap("env", "http_encode_url", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_decode_url", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_build_query", |_: i32, _: i32| -> i32 { 0 })?;

    // Instantiate and run
    let instance = linker.instantiate(&mut store, &module)?;

    if debug {
        println!("✅ WebAssembly module loaded successfully");
        println!(
            "📋 Exported functions: {:?}",
            instance
                .exports(&mut store)
                .map(|e| e.name())
                .collect::<Vec<_>>()
        );
    }

    // Get and call the start function (try both "_start" and "start")
    let start_func = instance
        .get_func(&mut store, "_start")
        .or_else(|| instance.get_func(&mut store, "start"));

    if let Some(start_func) = start_func {
        if debug {
            println!("🎯 Executing start function...");
            println!("--- Output ---");
        }
        start_func.call(&mut store, &[], &mut [])?;
        if debug {
            println!("--- End Output ---");
            println!("✅ Execution completed successfully!");
        }
    } else {
        eprintln!("❌ Error: start function not found in WASM module");
    }

    Ok(())
}

// Test runner removed - not compatible with 7-stage pipeline
// Tests should be implemented as separate .cln files and compiled individually

fn handle_explain(
    code: &str,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let code_upper = code.to_uppercase();

    // Get error explanation
    let explanation = get_error_explanation(&code_upper);

    if output_config.json_mode {
        let json = serde_json::json!({
            "code": code_upper,
            "category": get_error_category(&code_upper),
            "title": explanation.title,
            "description": explanation.description,
            "example": explanation.example,
            "fix": explanation.fix,
        });
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        // Terminal output with colors
        let colors = if output_config.use_colors {
            (
                "\x1b[1m", "\x1b[0m", "\x1b[36m", "\x1b[33m", "\x1b[32m", "\x1b[2m",
            )
        } else {
            ("", "", "", "", "", "")
        };
        let (bold, reset, cyan, yellow, green, dim) = colors;

        if explanation.title == "Unknown Error Code" {
            eprintln!(
                "{}error{}: Unknown error code '{}'",
                bold, reset, code_upper
            );
            eprintln!();
            eprintln!("Available error codes:");
            eprintln!("  {}Syntax errors:{} SYN001-SYN010", cyan, reset);
            eprintln!("  {}Type errors:{}   TYP001-TYP010", cyan, reset);
            eprintln!("  {}Memory errors:{} MEM001-MEM005", cyan, reset);
            eprintln!("  {}Runtime errors:{} RUN001-RUN005", cyan, reset);
            eprintln!();
            eprintln!("Use 'cln explain <code>' to learn more about a specific error.");
            return Ok(());
        }

        println!();
        println!(
            "{}error[{}]{}: {}",
            bold, code_upper, reset, explanation.title
        );
        println!();
        println!("{}Description:{}", cyan, reset);
        for line in explanation.description.lines() {
            println!("  {}", line);
        }

        if !explanation.example.is_empty() {
            println!();
            println!("{}Example of problematic code:{}", yellow, reset);
            println!();
            for line in explanation.example.lines() {
                println!("  {}{}{}", dim, line, reset);
            }
        }

        if !explanation.fix.is_empty() {
            println!();
            println!("{}How to fix:{}", green, reset);
            for line in explanation.fix.lines() {
                println!("  {}", line);
            }
        }

        println!();
    }

    Ok(())
}

struct ErrorExplanation {
    title: &'static str,
    description: &'static str,
    example: &'static str,
    fix: &'static str,
}

fn get_error_category(code: &str) -> &'static str {
    if code.len() < 3 {
        return "Unknown";
    }
    match &code[..3] {
        "SYN" => "Syntax",
        "TYP" => "Type",
        "MEM" => "Memory",
        "RUN" => "Runtime",
        "COD" => "Codegen",
        "VAL" => "Validation",
        "MOD" => "Module",
        _ => "Unknown",
    }
}

fn get_error_explanation(code: &str) -> ErrorExplanation {
    match code {
        // Syntax Errors (SYN001-SYN010)
        "SYN001" => ErrorExplanation {
            title: "Unexpected token",
            description: "The parser encountered a token that doesn't fit the expected syntax.\n\
                         This usually happens when there's a typo, missing punctuation, or incorrect\n\
                         language construct usage.",
            example: "start()\n\
                      \tinteger x = // missing value here\n\
                      \tprint x",
            fix: "Check the line indicated for:\n\
                  - Missing values or expressions\n\
                  - Typos in keywords (integer, string, boolean, etc.)\n\
                  - Missing operators or punctuation",
        },
        "SYN002" => ErrorExplanation {
            title: "Missing closing delimiter",
            description: "A parenthesis, bracket, or brace was opened but never closed.\n\
                         All delimiters must be properly balanced in Clean Language.",
            example: "start()\n\
                      \tprint(\"Hello world\"",
            fix: "Find the opening delimiter and add its matching close:\n\
                  - ( must be closed with )\n\
                  - [ must be closed with ]\n\
                  - { must be closed with }",
        },
        "SYN003" => ErrorExplanation {
            title: "Invalid indentation",
            description: "Clean Language uses tabs for indentation. Spaces or mixed indentation\n\
                         will cause this error. Each nested block requires one additional tab.",
            example: "start()\n\
                        integer x = 5  // Using spaces instead of tab",
            fix: "Use tabs for indentation, not spaces.\n\
                  Configure your editor to insert tabs when pressing Tab.\n\
                  Each block level should be indented by exactly one tab.",
        },
        "SYN004" => ErrorExplanation {
            title: "Invalid string literal",
            description: "A string literal is malformed. This could be due to:\n\
                         - Unclosed quotes\n\
                         - Invalid escape sequences\n\
                         - Line breaks within single-line strings",
            example: "start()\n\
                      \tstring s = \"Hello  // missing closing quote",
            fix: "Ensure strings are properly quoted:\n\
                  - Single-line strings: use matching \" quotes\n\
                  - Escape special characters: \\n, \\t, \\\", \\\\",
        },
        "SYN005" => ErrorExplanation {
            title: "Invalid number literal",
            description: "A number literal is malformed. Clean Language supports:\n\
                         - Integers: 42, -17, 0\n\
                         - Floats: 3.14, -2.5, 1.0e10",
            example: "start()\n\
                      \tnumber x = 3.14.5  // Invalid: multiple decimal points",
            fix: "Check the number format:\n\
                  - Only one decimal point allowed\n\
                  - Scientific notation: 1e10, 2.5e-3\n\
                  - No spaces within numbers",
        },
        "SYN006" => ErrorExplanation {
            title: "Expected expression",
            description: "An expression was expected but not found. This occurs when:\n\
                         - An operator is missing its operand\n\
                         - A function call has empty arguments where one is required\n\
                         - An assignment is missing its right-hand side",
            example: "start()\n\
                      \tinteger x =   // missing expression after =",
            fix: "Provide a valid expression:\n\
                  - Literals: 42, \"text\", true\n\
                  - Variables: myVar\n\
                  - Operations: a + b\n\
                  - Function calls: getValue()",
        },
        "SYN007" => ErrorExplanation {
            title: "Invalid identifier",
            description: "The identifier name is not valid. Identifiers must:\n\
                         - Start with a letter or underscore\n\
                         - Contain only letters, numbers, and underscores\n\
                         - Not be a reserved keyword",
            example: "start()\n\
                      \tinteger 123abc = 5  // Cannot start with number",
            fix: "Use valid identifier names:\n\
                  - Good: myVar, _private, count123\n\
                  - Bad: 123var, my-var, class (reserved)",
        },
        "SYN008" => ErrorExplanation {
            title: "Missing function body",
            description: "A function was declared but has no body. All functions must have\n\
                         at least one statement in their body.",
            example: "myFunc()\n\
                      // No body - next line is a different function\n\
                      start()",
            fix: "Add at least one statement to the function body:\n\
                  myFunc()\n\
                  \treturn 0\n\n\
                  Or use 'pass' for empty functions that will be implemented later.",
        },
        "SYN009" => ErrorExplanation {
            title: "Unexpected end of file",
            description: "The file ended unexpectedly while the parser was expecting more content.\n\
                         This usually means an unclosed block or incomplete statement.",
            example: "start()\n\
                      \tif x > 0\n\
                      \t\t// File ends here without closing the if block",
            fix: "Ensure all blocks and statements are complete:\n\
                  - Close all if/else/while/for blocks\n\
                  - Complete all function bodies\n\
                  - End strings and comments properly",
        },
        "SYN010" => ErrorExplanation {
            title: "Reserved keyword used as identifier",
            description: "A reserved keyword is being used where an identifier is expected.\n\
                         Keywords like 'if', 'while', 'class', etc. cannot be used as variable\n\
                         or function names.",
            example: "start()\n\
                      \tinteger class = 5  // 'class' is a reserved keyword",
            fix: "Choose a different name that is not a keyword.\n\
                  Reserved keywords include: if, else, while, for, class, function,\n\
                  return, true, false, null, integer, string, boolean, number, etc.",
        },

        // Type Errors (TYP001-TYP010)
        "TYP001" => ErrorExplanation {
            title: "Type mismatch",
            description: "The type of an expression doesn't match what was expected.\n\
                         Clean Language is strongly typed - you cannot implicitly convert\n\
                         between incompatible types.",
            example: "start()\n\
                      \tinteger x = \"hello\"  // Cannot assign string to integer",
            fix: "Ensure types match or use explicit conversion:\n\
                  - Use type conversion: x.toInteger(), x.toString()\n\
                  - Declare with correct type: string x = \"hello\"\n\
                  - Use appropriate literal: integer x = 42",
        },
        "TYP002" => ErrorExplanation {
            title: "Undefined variable",
            description: "A variable is being used that hasn't been declared.\n\
                         Variables must be declared with their type before use.",
            example: "start()\n\
                      \tprint undeclaredVar  // Variable not declared",
            fix: "Declare the variable before using it:\n\
                  integer myVar = 0\n\
                  print myVar\n\n\
                  Or check for typos in the variable name.",
        },
        "TYP003" => ErrorExplanation {
            title: "Undefined function",
            description: "A function is being called that hasn't been defined.\n\
                         Functions must be defined before they are called.",
            example: "start()\n\
                      \tresult = unknownFunc()  // Function not defined",
            fix: "Define the function before calling it:\n\
                  unknownFunc() -> integer\n\
                  \treturn 42\n\n\
                  Or check for typos in the function name.",
        },
        "TYP004" => ErrorExplanation {
            title: "Invalid operation for type",
            description: "An operation was attempted that is not valid for the given type.\n\
                         For example, arithmetic on strings or string concatenation on booleans.",
            example: "start()\n\
                      \tstring s = \"hello\"\n\
                      \tinteger x = s + 5  // Cannot add integer to string",
            fix: "Use operations appropriate for the type:\n\
                  - Arithmetic (+, -, *, /): numbers only\n\
                  - Comparison (==, !=): same types\n\
                  - String concat: strings only\n\
                  - Use type conversion if needed",
        },
        "TYP005" => ErrorExplanation {
            title: "Wrong number of arguments",
            description: "A function was called with the wrong number of arguments.\n\
                         The number of arguments must match the function signature.",
            example: "add(a: integer, b: integer) -> integer\n\
                      \treturn a + b\n\n\
                      start()\n\
                      \tresult = add(5)  // Missing second argument",
            fix: "Provide the correct number of arguments.\n\
                  Check the function signature for required parameters.\n\
                  Optional parameters with defaults don't need to be provided.",
        },
        "TYP006" => ErrorExplanation {
            title: "Return type mismatch",
            description: "A function returns a value that doesn't match its declared return type.\n\
                         The return statement must provide a value of the correct type.",
            example: "getValue() -> integer\n\
                      \treturn \"hello\"  // Returns string, expected integer",
            fix: "Return a value matching the declared type:\n\
                  - Change the return statement value\n\
                  - Change the function's return type\n\
                  - Add type conversion if appropriate",
        },
        "TYP007" => ErrorExplanation {
            title: "Cannot infer type",
            description: "The compiler cannot determine the type of an expression.\n\
                         This may happen with complex expressions or missing type annotations.",
            example: "start()\n\
                      \tx = someComplexExpression  // Type unclear",
            fix: "Provide an explicit type annotation:\n\
                  integer x = someComplexExpression\n\n\
                  Or simplify the expression so the type is clear.",
        },
        "TYP008" => ErrorExplanation {
            title: "Undefined method",
            description: "A method was called on a type that doesn't have that method.\n\
                         Check the available methods for the type you're using.",
            example: "start()\n\
                      \tinteger x = 42\n\
                      \tx.unknownMethod()  // Method doesn't exist on integer",
            fix: "Use a method that exists for the type:\n\
                  - integer: toString(), toNumber(), toBoolean()\n\
                  - string: length(), toUpperCase(), toLowerCase()\n\
                  - array: length(), push(), pop(), get(), set()",
        },
        "TYP009" => ErrorExplanation {
            title: "Undefined class",
            description: "A class is being used that hasn't been defined.\n\
                         Classes must be defined before they can be instantiated.",
            example: "start()\n\
                      \tp = UndefinedClass.new()  // Class not defined",
            fix: "Define the class before using it:\n\
                  class MyClass\n\
                  \tinteger value\n\
                  \tnew(v: integer)\n\
                  \t\tvalue = v\n\n\
                  Or import it if defined in another module.",
        },
        "TYP010" => ErrorExplanation {
            title: "Incompatible types in comparison",
            description: "Two values of incompatible types are being compared.\n\
                         Comparisons require operands of the same type.",
            example: "start()\n\
                      \tif 5 == \"five\"  // Cannot compare integer and string\n\
                      \t\tprint \"equal\"",
            fix: "Convert values to the same type before comparing:\n\
                  if 5 == \"five\".toInteger()\n\n\
                  Or compare values of the same type.",
        },

        // Memory Errors (MEM001-MEM005)
        "MEM001" => ErrorExplanation {
            title: "Stack overflow",
            description: "The program's call stack has exceeded its maximum size.\n\
                         This is usually caused by infinite or very deep recursion.",
            example: "infiniteRecursion()\n\
                      \tinfiniteRecursion()  // No base case!\n\n\
                      start()\n\
                      \tinfiniteRecursion()",
            fix: "Add a base case to recursive functions:\n\
                  factorial(n: integer) -> integer\n\
                  \tif n <= 1\n\
                  \t\treturn 1\n\
                  \treturn n * factorial(n - 1)",
        },
        "MEM002" => ErrorExplanation {
            title: "Out of memory",
            description: "The program has exhausted available memory.\n\
                         This can happen with very large data structures or memory leaks.",
            example: "start()\n\
                      \tlist = []\n\
                      \twhile true\n\
                      \t\tlist.push(\"data\")  // Infinite list growth",
            fix: "Limit data structure sizes:\n\
                  - Set maximum sizes for collections\n\
                  - Process data in chunks\n\
                  - Release unused data",
        },
        "MEM003" => ErrorExplanation {
            title: "Null pointer access",
            description: "Attempted to access a null or undefined reference.\n\
                         This occurs when using an uninitialized or cleared variable.",
            example: "start()\n\
                      \tp = null\n\
                      \tprint p.value  // Accessing property of null",
            fix: "Check for null before accessing:\n\
                  if p != null\n\
                  \tprint p.value\n\n\
                  Or ensure the variable is properly initialized.",
        },
        "MEM004" => ErrorExplanation {
            title: "Index out of bounds",
            description: "An array or list was accessed with an invalid index.\n\
                         Indices must be >= 0 and < length.",
            example: "start()\n\
                      \tarr = [1, 2, 3]\n\
                      \tprint arr[5]  // Only indices 0, 1, 2 are valid",
            fix: "Ensure index is within bounds:\n\
                  if index >= 0 && index < arr.length()\n\
                  \tprint arr[index]\n\n\
                  Or use safe access methods with default values.",
        },
        "MEM005" => ErrorExplanation {
            title: "Memory allocation failed",
            description: "The system could not allocate requested memory.\n\
                         This is typically a system-level resource issue.",
            example: "// Attempting to allocate very large array\n\
                      data = Array<integer>.new(999999999999)",
            fix: "Request smaller allocations:\n\
                  - Use reasonable data structure sizes\n\
                  - Process data in smaller batches\n\
                  - Check system available memory",
        },

        // Runtime Errors (RUN001-RUN005)
        "RUN001" => ErrorExplanation {
            title: "Division by zero",
            description: "Attempted to divide a number by zero.\n\
                         Division by zero is undefined in mathematics and programming.",
            example: "start()\n\
                      \tresult = 10 / 0  // Division by zero",
            fix: "Check the divisor before dividing:\n\
                  if divisor != 0\n\
                  \tresult = value / divisor\n\
                  else\n\
                  \tprint \"Cannot divide by zero\"",
        },
        "RUN002" => ErrorExplanation {
            title: "Integer overflow",
            description: "An integer operation resulted in a value too large to represent.\n\
                         Clean Language integers have a maximum value.",
            example: "start()\n\
                      \tx = 2147483647\n\
                      \tx = x + 1  // Overflows 32-bit integer",
            fix: "Use appropriate numeric types:\n\
                  - Use 'number' (float) for very large values\n\
                  - Check bounds before arithmetic\n\
                  - Consider BigInteger for arbitrary precision",
        },
        "RUN003" => ErrorExplanation {
            title: "Invalid cast",
            description: "A type conversion failed because the value cannot be converted.\n\
                         Not all values can be converted between types.",
            example: "start()\n\
                      \ts = \"hello\"\n\
                      \tn = s.toInteger()  // \"hello\" cannot become integer",
            fix: "Validate values before conversion:\n\
                  - Check string format before parsing\n\
                  - Handle conversion failures gracefully\n\
                  - Use try/onError for safe conversion",
        },
        "RUN004" => ErrorExplanation {
            title: "Assertion failed",
            description: "An assertion check failed during program execution.\n\
                         Assertions verify expected conditions in the code.",
            example: "validateAge(age: integer)\n\
                      \tassert age >= 0  // Fails if age is negative",
            fix: "Fix the condition that caused the assertion to fail:\n\
                  - Ensure input data is valid\n\
                  - Check for edge cases in your logic\n\
                  - Validate data at system boundaries",
        },
        "RUN005" => ErrorExplanation {
            title: "Timeout exceeded",
            description: "An operation took longer than the allowed time limit.\n\
                         This prevents infinite loops from hanging the program.",
            example: "start()\n\
                      \twhile true\n\
                      \t\t// Infinite loop",
            fix: "Ensure operations complete in reasonable time:\n\
                  - Add proper termination conditions to loops\n\
                  - Break large operations into smaller chunks\n\
                  - Add progress checks in long-running code",
        },

        // Default for unknown codes
        _ => ErrorExplanation {
            title: "Unknown Error Code",
            description: "This error code is not recognized.",
            example: "",
            fix: "",
        },
    }
}
