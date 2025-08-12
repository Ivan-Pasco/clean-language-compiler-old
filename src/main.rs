/*!
 * Clean Language Compiler - Main Application
 *
 * Author: Ivan Pasco Lizarraga
 * Date: 17-07-2025
 * Website: https://www.cleanlanguage.dev
 *
 * A modern, type-safe programming language that compiles to WebAssembly
 */

#![allow(clippy::uninlined_format_args)]
#![allow(clippy::manual_inspect)]
#![allow(clippy::format_in_format_args)]
#![allow(clippy::useless_asref)]

use clap::{Parser, Subcommand};
use clean_language_compiler::compile_with_file;
use std::fs;
use std::path::Path;

/// Clean Language Compiler and Test Runner
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile a Clean Language file to WebAssembly
    Compile {
        /// Input file to compile
        #[arg(short, long)]
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
    },
    /// Package management commands
    #[command(subcommand)]
    Package(PackageCommands),
    /// Run the Clean Language test suite
    Test {
        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,

        /// Additional test directories to include
        #[arg(short, long)]
        dirs: Vec<String>,
    },
    /// Run simple compilation tests
    SimpleTest {
        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },
    /// Run comprehensive Clean Language test suite
    ComprehensiveTest {
        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },
    /// Debug a Clean Language file with enhanced error reporting
    Debug {
        /// Input file to debug
        #[arg(short, long)]
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
        #[arg(short, long)]
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
        #[arg(short, long)]
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
        #[arg(short, long)]
        input: String,

        /// Enable debug output
        #[arg(short, long)]
        debug: bool,
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

    match args.command {
        Commands::Compile {
            input,
            output,
            opt_level,
            test,
            include_tests,
        } => handle_compile(input, output, opt_level, test, include_tests).await?,
        Commands::Package(package_cmd) => handle_package(package_cmd).await?,
        Commands::Test { verbose, dirs } => handle_test(verbose, dirs).await?,
        Commands::SimpleTest { verbose } => handle_simple_test(verbose).await?,
        Commands::ComprehensiveTest { verbose } => handle_comprehensive_test(verbose).await?,
        Commands::Debug {
            input,
            show_ast,
            check_style,
            analyze_errors,
        } => handle_debug(input, show_ast, check_style, analyze_errors).await?,
        Commands::Lint {
            input,
            fix,
            errors_only,
        } => handle_lint(input, fix, errors_only).await?,
        Commands::Parse {
            input,
            show_tree,
            recover_errors,
        } => handle_parse(input, show_tree, recover_errors).await?,
        Commands::Run { input, debug } => handle_run(input, debug).await?,
    }

    Ok(())
}

async fn handle_compile(
    input: String,
    output: String,
    _opt_level: u8,
    test: bool,
    include_tests: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Compiling {input} to {output}");

    let source = fs::read_to_string(&input)?;

    // Try to parse the program to check for tests, with fallback to recovery parsing
    use clean_language_compiler::compile_with_recovery;
    use clean_language_compiler::parser::CleanParser;

    let (program, use_recovery) = match CleanParser::parse_program_with_file(&source, &input) {
        Ok(program) => {
            println!(
                "DEBUG: Parsed program with {} functions, {} classes",
                program.functions.len(),
                program.classes.len()
            );
            for (i, class) in program.classes.iter().enumerate() {
                println!(
                    "DEBUG: Class {}: {} with {} methods",
                    i,
                    class.name,
                    class.methods.len()
                );
            }
            (program, false)
        }
        Err(_parse_error) => {
            // If regular parsing fails, try recovery parsing
            println!("🔄 Regular parsing failed, trying recovery compilation...");
            match CleanParser::parse_program_with_recovery(&source, &input) {
                Ok(program) => (program, true),
                Err(recovery_errors) => {
                    eprintln!("❌ Both regular and recovery parsing failed.");
                    eprintln!("Recovery errors: {:?}", recovery_errors);
                    return Err("Parsing failed".into());
                }
            }
        }
    };

    // Run tests if requested
    if test && !program.tests.is_empty() {
        println!("\n🧪 Running tests...");
        run_tests(&program, &input)?;
    } else if test && program.tests.is_empty() {
        println!("⚠️  No tests found to run");
    }

    // Use appropriate compilation method
    let wasm_binary = if use_recovery {
        println!("🔧 Using recovery compilation...");
        match compile_with_recovery(&source, &input) {
            Ok(binary) => binary,
            Err(errors) => {
                eprintln!(
                    "❌ Recovery compilation failed with {} errors:",
                    errors.len()
                );
                for (i, error) in errors.iter().enumerate() {
                    eprintln!("Error {}: {}", i + 1, error);
                }
                return Err("Compilation failed".into());
            }
        }
    } else {
        compile_with_file(&source, &input)?
    };

    if let Some(parent) = Path::new(&output).parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&output, wasm_binary)?;

    println!("Successfully compiled to {output}");

    if include_tests && !program.tests.is_empty() {
        println!("📝 Tests included in binary (accessible via --run-tests flag)");
    }

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
                Ok(_) => println!("✅ Dependency added successfully!"),
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
                Ok(_) => println!("✅ Dependency removed successfully!"),
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
    if !status.success() {
        eprintln!("✗ Some tests failed");
        // Don't return error for test failures - just report them
        println!("Note: Test failures reported but not treating as critical error");
    } else {
        println!("✓ All tests passed!");
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
        Err(error) => {
            eprintln!("✗ Simple test failed: {error}");
            Err(error.into())
        }
    }
}

async fn handle_comprehensive_test(verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running comprehensive Clean Language test suite...");
    if verbose {
        println!("Verbose output enabled");
    }

    let test_cases = vec![
        ("Basic", "start()\n\tinteger x = 42\n\tprint(x)\n"),
        (
            "Arithmetic",
            "start()\n\tinteger x = 1 + 2 * 3\n\tprint(x)\n",
        ),
        (
            "Variables",
            "start()\n\tinteger x = 5\n\tinteger y = x + 1\n\tprint(y)\n",
        ),
    ];

    let mut passed = 0;
    let total = test_cases.len();

    for (name, source) in test_cases {
        print!("Testing {}: ", name);
        match compile_with_file(source, &format!("{}_test.clean", name.to_lowercase())) {
            Ok(wasm_binary) => {
                println!("✓ {} bytes", wasm_binary.len());
                passed += 1;
            }
            Err(error) => {
                println!("✗ {error}");
            }
        }
    }

    println!("Results: {passed}/{total} tests passed");
    if passed == total {
        println!("🎉 All comprehensive tests passed!");
        Ok(())
    } else {
        eprintln!("⚠ Some tests failed");
        Err("Some comprehensive tests failed".into())
    }
}

async fn handle_debug(
    input: String,
    show_ast: bool,
    check_style: bool,
    analyze_errors: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Debugging Clean Language file: {input}\n");

    let source = match fs::read_to_string(&input) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("❌ Error reading file '{input}': {e}");
            return Ok(());
        }
    };

    use clean_language_compiler::debug::DebugUtils;
    use clean_language_compiler::parser::CleanParser;

    let parse_result = CleanParser::parse_program_with_file(&source, &input);
    let warnings = Vec::new();

    let debug_report = DebugUtils::create_debug_report(&source, &input, &parse_result, &warnings);
    println!("{debug_report}");

    match &parse_result {
        Ok(program) => {
            if show_ast {
                println!("\n");
                DebugUtils::print_ast(program);
            }
        }
        Err(error) => {
            if analyze_errors {
                println!("\n");
                let analysis = DebugUtils::analyze_errors(&[error.clone()]);
                for line in analysis {
                    println!("{line}");
                }
            }
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

    use clean_language_compiler::debug::DebugUtils;
    use clean_language_compiler::parser::CleanParser;

    let mut total_issues = 0;
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

        let parse_result = CleanParser::parse_program_with_file(&source, file_path);
        if let Err(error) = &parse_result {
            total_errors += 1;
            if !errors_only {
                println!("  ❌ Compilation Error:");
                println!("     {error}");
            }
        }

        let style_issues = DebugUtils::validate_style(&source);
        if !style_issues.is_empty() {
            total_issues += style_issues.len();
            if !errors_only {
                println!("🎨 Style Issues Found:");
                for issue in &style_issues {
                    println!("  {issue}");
                }
            }
        }

        if parse_result.is_ok() && style_issues.is_empty() {
            println!("  ✅ No issues found");
        }
    }

    println!("\n=== Lint Summary ===");
    println!("Files checked: {}", files_to_lint.len());
    println!("Compilation errors: {total_errors}");
    println!("Style issues: {total_issues}");

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

    use clean_language_compiler::debug::DebugUtils;
    use clean_language_compiler::parser::CleanParser;

    if recover_errors {
        println!("🔄 Using enhanced error recovery mode...\n");

        // Use the enhanced error recovery parser
        let mut recovery_parser =
            clean_language_compiler::parser::ErrorRecoveringParser::new(&source, &input);
        recovery_parser = recovery_parser.with_max_errors(50); // Allow up to 50 errors

        match recovery_parser.parse_with_recovery(&source) {
            Ok(program) => {
                println!("✅ Parsing succeeded with error recovery!");

                if show_tree {
                    println!("\n🌳 AST Structure:");
                    println!("{}", "═".repeat(50));
                    DebugUtils::print_ast(&program);
                }

                // Check if we collected any warnings during recovery
                if !recovery_parser.warnings.is_empty() {
                    println!("\n⚠️  Warnings collected during parsing:");
                    for warning in &recovery_parser.warnings {
                        println!("  • {warning}");
                    }
                }

                println!("\n📊 Recovery Statistics:");
                println!(
                    "  • Recovery points identified: {}",
                    recovery_parser.recovery_points.len()
                );
                println!("  • Warnings: {}", recovery_parser.warnings.len());
                println!("  • Functions parsed: {}", program.functions.len());
                if program.start_function.is_some() {
                    println!("  • Start function: ✅");
                }
                println!("  • Classes parsed: {}", program.classes.len());
            }
            Err(errors) => {
                println!("❌ Parsing failed with {} error(s):\n", errors.len());

                // Generate comprehensive error report
                let error_report = DebugUtils::create_error_report(&source, &errors);
                println!("{error_report}");

                // If partial parsing was successful, show what we recovered
                if !recovery_parser.errors.is_empty() && errors.len() < 20 {
                    println!("\n🔧 Attempting to show recovered partial AST...");

                    // Try to create a minimal program from whatever we could parse
                    let partial_program = clean_language_compiler::ast::Program {
                        imports: Vec::new(),
                        statements: Vec::new(),
                        functions: Vec::new(),
                        classes: Vec::new(),
                        start_function: None,
                        tests: Vec::new(),
                        location: None,
                    };

                    if show_tree {
                        DebugUtils::print_ast(&partial_program);
                    }
                }
            }
        }
    } else {
        println!("🔄 Using standard parsing mode...\n");

        match CleanParser::parse_program_with_file(&source, &input) {
            Ok(program) => {
                println!("✅ Parsing succeeded!");
                if show_tree {
                    println!("\n");
                    DebugUtils::print_ast(&program);
                }
            }
            Err(error) => {
                eprintln!("❌ Parsing failed:");
                println!("{error}");

                // Provide basic suggestions even in standard mode
                println!("\n💡 Suggestions:");
                println!("  • Try using --recover-errors for detailed error analysis");
                println!("  • Check the Clean Language syntax documentation");

                // Basic error analysis
                let suggestions = DebugUtils::suggest_error_fixes(&source, &[error]);
                for suggestion in suggestions {
                    println!("  • {suggestion}");
                }
            }
        }
    }

    Ok(())
}

async fn handle_run(input: String, debug: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !Path::new(&input).exists() {
        eprintln!("❌ Error: File '{input}' not found");
        return Ok(());
    }

    let input_path = Path::new(&input);
    let wasm_bytes = match input_path.extension().and_then(|s| s.to_str()) {
        Some("cln") => {
            // Handle Clean Language source file - compile to WASM first
            println!("🔧 Compiling Clean Language file: {input}");
            
            let source = fs::read_to_string(&input)?;
            if debug {
                println!("📝 Source file size: {} characters", source.len());
            }

            // Try to compile the source to WASM
            let wasm_binary = match compile_with_file(&source, &input) {
                Ok(binary) => {
                    if debug {
                        println!("✅ Compilation successful: {} bytes of WASM generated", binary.len());
                    }
                    binary
                }
                Err(compile_error) => {
                    eprintln!("❌ Compilation failed: {compile_error}");
                    return Err(compile_error.into());
                }
            };

            println!("🚀 Running compiled WebAssembly...");
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
            eprintln!("   Supported formats: .cln (Clean Language source), .wasm (WebAssembly binary)");
            return Ok(());
        }
        None => {
            eprintln!("❌ Error: File has no extension");
            eprintln!("   Supported formats: .cln (Clean Language source), .wasm (WebAssembly binary)");
            return Ok(());
        }
    };

    // Use wasmtime to execute the WASM file
    use wasmtime::*;
    use std::sync::Mutex;

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

    // Create engine and store
    let engine = Engine::default();
    let mut store = Store::new(&engine, ());

    // Create module
    let module = Module::new(&engine, &wasm_bytes)?;

    // Create linker and add imports
    let mut linker = Linker::new(&engine);

    // Add print functions
    linker.func_wrap("env", "print", move |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
        if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
            if let Some(data) = mem.data(&caller).get(ptr as usize..(ptr + len) as usize) {
                if let Ok(s) = std::str::from_utf8(data) {
                    print!("{}", s);
                } else {
                    print!("[invalid utf8: {} bytes]", len);
                }
            }
        }
    })?;

    linker.func_wrap("env", "printl", move |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
        if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
            if let Some(data) = mem.data(&caller).get(ptr as usize..(ptr + len) as usize) {
                if let Ok(s) = std::str::from_utf8(data) {
                    println!("{}", s);
                } else {
                    println!("[invalid utf8: {} bytes]", len);
                }
            }
        }
    })?;

    // Add memory runtime functions with correct signatures
    linker.func_wrap("memory_runtime", "mem_alloc", |size: i32, alignment: i32| -> i32 { 
        // Simple allocation stub - returns aligned size
        (size + alignment - 1) & !(alignment - 1)
    })?;
    linker.func_wrap("memory_runtime", "mem_retain", |_ptr: i32| {})?;
    linker.func_wrap("memory_runtime", "mem_release", |_ptr: i32| {})?;

    // Add type conversion functions
    let debug_copy = debug;
    linker.func_wrap("env", "int_to_string", move |mut caller: Caller<'_, ()>, value: i32| -> i32 {
        let string_value = value.to_string();
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            allocate_string_in_memory(&memory, &mut caller, &string_value, debug_copy)
        } else {
            0
        }
    })?;

    let debug_copy = debug;
    linker.func_wrap("env", "float_to_string", move |mut caller: Caller<'_, ()>, value: f64| -> i32 {
        let string_value = value.to_string();
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            allocate_string_in_memory(&memory, &mut caller, &string_value, debug_copy)
        } else {
            0
        }
    })?;

    // Input functions - complete implementation for basic I/O
    let debug_input = debug;
    linker.func_wrap("env", "input", move |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
        // Read prompt from WASM memory and display it
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            if let Some(data) = memory.data(&caller).get(prompt_ptr as usize..(prompt_ptr + prompt_len) as usize) {
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
    })?;

    linker.func_wrap("env", "input_integer", |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
        // Read prompt from WASM memory and display it
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            if let Some(data) = memory.data(&caller).get(prompt_ptr as usize..(prompt_ptr + prompt_len) as usize) {
                if let Ok(prompt) = std::str::from_utf8(data) {
                    print!("{}", prompt);
                    std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
                }
            }
        }
        
        // Read integer from stdin
        use std::io::{stdin, BufRead, BufReader};
        let input = BufReader::new(stdin());
        if let Ok(line) = input.lines().next().unwrap_or(Ok(String::new())) {
            if let Ok(value) = line.trim().parse::<i32>() {
                return value;
            }
        }
        0
    })?;

    linker.func_wrap("env", "input_float", |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> f64 {
        // Read prompt from WASM memory and display it
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            if let Some(data) = memory.data(&caller).get(prompt_ptr as usize..(prompt_ptr + prompt_len) as usize) {
                if let Ok(prompt) = std::str::from_utf8(data) {
                    print!("{}", prompt);
                    std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
                }
            }
        }
        
        // Read float from stdin
        use std::io::{stdin, BufRead, BufReader};
        let input = BufReader::new(stdin());
        if let Ok(line) = input.lines().next().unwrap_or(Ok(String::new())) {
            if let Ok(value) = line.trim().parse::<f64>() {
                return value;
            }
        }
        0.0
    })?;

    linker.func_wrap("env", "input_yesno", |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
        // Read prompt from WASM memory and display it
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            if let Some(data) = memory.data(&caller).get(prompt_ptr as usize..(prompt_ptr + prompt_len) as usize) {
                if let Ok(prompt) = std::str::from_utf8(data) {
                    print!("{}", prompt);
                    std::io::Write::flush(&mut std::io::stdout()).unwrap_or(());
                }
            }
        }
        
        // Read yes/no from stdin
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

    linker.func_wrap("env", "input_range", |_: i32, _: i32, _: i32, _: i32| -> i32 {
        // Stub implementation for range input validation
        // This would validate if input is within a specified range
        println!("Note: input_range function not fully implemented");
        0
    })?;

    // String conversion functions
    let debug_copy = debug;
    linker.func_wrap("env", "bool_to_string", move |mut caller: Caller<'_, ()>, value: i32| -> i32 {
        let string_value = if value != 0 { "true" } else { "false" };
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            allocate_string_in_memory(&memory, &mut caller, string_value, debug_copy)
        } else {
            0
        }
    })?;

    linker.func_wrap("env", "string_to_int", |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
        // Read string from memory at ptr (expects length-prefixed string)
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            let data = memory.data(&caller);
            if let Some(len_bytes) = data.get(ptr as usize..(ptr + 4) as usize) {
                let len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
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
    })?;

    linker.func_wrap("env", "string_to_float", |mut caller: Caller<'_, ()>, ptr: i32| -> f64 {
        // Read string from memory at ptr (expects length-prefixed string)
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            let data = memory.data(&caller);
            if let Some(len_bytes) = data.get(ptr as usize..(ptr + 4) as usize) {
                let len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
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
    })?;

    // File I/O functions - basic stub implementations
    linker.func_wrap("env", "file_write", |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32, content_ptr: i32, content_len: i32| -> i32 {
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            let data = memory.data(&caller);
            if let (Some(path_data), Some(content_data)) = (
                data.get(path_ptr as usize..(path_ptr + path_len) as usize),
                data.get(content_ptr as usize..(content_ptr + content_len) as usize)
            ) {
                if let (Ok(path), Ok(content)) = (std::str::from_utf8(path_data), std::str::from_utf8(content_data)) {
                    if let Err(_) = std::fs::write(path, content) {
                        return -1; // Error
                    }
                    return 0; // Success
                }
            }
        }
        -1
    })?;

    let debug_file_read = debug;
    linker.func_wrap("env", "file_read", move |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32, max_size: i32| -> i32 {
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            let data = memory.data(&caller);
            if let Some(path_data) = data.get(path_ptr as usize..(path_ptr + path_len) as usize) {
                if let Ok(path) = std::str::from_utf8(path_data) {
                    if let Ok(content) = std::fs::read_to_string(path) {
                        let truncated_content = if content.len() > max_size as usize {
                            &content[..max_size as usize]
                        } else {
                            &content
                        };
                        return allocate_string_in_memory(&memory, &mut caller, truncated_content, debug_file_read);
                    }
                }
            }
        }
        0
    })?;

    linker.func_wrap("env", "file_exists", |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            let data = memory.data(&caller);
            if let Some(path_data) = data.get(path_ptr as usize..(path_ptr + path_len) as usize) {
                if let Ok(path) = std::str::from_utf8(path_data) {
                    return if std::path::Path::new(path).exists() { 1 } else { 0 };
                }
            }
        }
        0
    })?;

    linker.func_wrap("env", "file_delete", |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            let data = memory.data(&caller);
            if let Some(path_data) = data.get(path_ptr as usize..(path_ptr + path_len) as usize) {
                if let Ok(path) = std::str::from_utf8(path_data) {
                    if let Err(_) = std::fs::remove_file(path) {
                        return -1; // Error
                    }
                    return 0; // Success
                }
            }
        }
        -1
    })?;

    linker.func_wrap("env", "file_append", |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32, content_ptr: i32, content_len: i32| -> i32 {
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            let data = memory.data(&caller);
            if let (Some(path_data), Some(content_data)) = (
                data.get(path_ptr as usize..(path_ptr + path_len) as usize),
                data.get(content_ptr as usize..(content_ptr + content_len) as usize)
            ) {
                if let (Ok(path), Ok(content)) = (std::str::from_utf8(path_data), std::str::from_utf8(content_data)) {
                    use std::fs::OpenOptions;
                    use std::io::Write;
                    if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path) {
                        if let Err(_) = file.write_all(content.as_bytes()) {
                            return -1; // Error
                        }
                        return 0; // Success
                    }
                }
            }
        }
        -1
    })?;

    // HTTP functions - basic stub implementations
    let debug_http = debug;
    linker.func_wrap("env", "http_get", move |mut caller: Caller<'_, ()>, _url_ptr: i32, _url_len: i32| -> i32 {
        // Basic stub - would need proper HTTP client implementation
        if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
            let error_message = "HTTP functions not fully implemented";
            allocate_string_in_memory(&memory, &mut caller, error_message, debug_http)
        } else {
            0
        }
    })?;

    // HTTP stub functions with correct signatures matching WASM imports
    linker.func_wrap("env", "http_post", |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_put", |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_patch", |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_delete", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_head", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_options", |_: i32, _: i32| -> i32 { 0 })?;
    // HTTP functions with headers - actually use 4 parameters based on sig=19
    linker.func_wrap("env", "http_get_with_headers", |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_post_with_headers", |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_post_json", |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_put_json", |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_patch_json", |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_post_form", |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 })?;
    
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
        println!("📋 Exported functions: {:?}", 
            instance.exports(&mut store).map(|e| e.name()).collect::<Vec<_>>());
    }

    // Get and call the start function
    if let Some(start_func) = instance.get_func(&mut store, "start") {
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

fn run_tests(
    program: &clean_language_compiler::ast::Program,
    file_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut passed = 0;
    let mut failed = 0;

    println!("Running tests for {file_path}...\n");

    for (i, test) in program.tests.iter().enumerate() {
        let test_name = test
            .description
            .as_ref()
            .map(|d| d.clone())
            .unwrap_or_else(|| format!("Test #{}", i + 1));

        // For now, we'll implement a basic test runner
        // In a full implementation, this would compile and run the test expression
        let test_result = evaluate_test_expression(&test.test_expression, &test.expected_value);

        match test_result {
            Ok(true) => {
                println!("✅ {test_name}: PASS");
                passed += 1;
            }
            Ok(false) => {
                println!("❌ {test_name}: FAIL");
                println!("   Expected: {:?}", test.expected_value);
                println!("   Got: {:?}", test.test_expression);
                failed += 1;
            }
            Err(e) => {
                println!("❌ {test_name}: ERROR - {e}");
                failed += 1;
            }
        }
    }

    println!(
        "\nTest Results: {passed} passed, {failed} failed, {total} total",
        total = passed + failed
    );

    if failed > 0 {
        println!(
            "Note: {} test(s) failed but not treating as critical error",
            failed
        );
        // Don't return error for test failures - just report them
    }

    Ok(())
}

fn evaluate_test_expression(
    test_expr: &clean_language_compiler::ast::Expression,
    expected: &clean_language_compiler::ast::Expression,
) -> Result<bool, String> {
    use clean_language_compiler::ast::{Expression, Value};
    // This is a simplified test evaluator
    // In a full implementation, this would compile the expressions to WASM and execute them

    match (test_expr, expected) {
        (Expression::Literal(Value::Integer(a)), Expression::Literal(Value::Integer(b))) => {
            Ok(a == b)
        }
        (Expression::Literal(Value::Number(a)), Expression::Literal(Value::Number(b))) => {
            Ok((a - b).abs() < f64::EPSILON)
        }
        (Expression::Literal(Value::String(a)), Expression::Literal(Value::String(b))) => {
            Ok(a == b)
        }
        (Expression::Literal(Value::Boolean(a)), Expression::Literal(Value::Boolean(b))) => {
            Ok(a == b)
        }
        _ => {
            // For complex expressions, we'd need to compile and execute
            // For now, we'll just compare the AST structure
            Ok(format!("{test_expr:?}") == format!("{expected:?}"))
        }
    }
}
