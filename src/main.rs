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
use clean_language_compiler::codegen::bridge_generator::{BridgeGenerator, BridgeTarget};
use clean_language_compiler::debug::DebugUtils;
use clean_language_compiler::error::{CompilerError, ErrorReporter};
use clean_language_compiler::{
    compile_with_file, runtime::runtime_manager::RuntimeManager,
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
        let (severity, message, location, code) = match error {
            CompilerError::Syntax { context } => (
                "error",
                context.message.clone(),
                context.location.clone(),
                context.error_code.clone(),
            ),
            CompilerError::Type { context } => (
                "error",
                context.message.clone(),
                context.location.clone(),
                context.error_code.clone(),
            ),
            _ => ("error", error.to_string(), None, None),
        };

        let file = location.as_ref().map(|l| l.file.as_str()).unwrap_or("");
        let line = location.as_ref().map(|l| l.line).unwrap_or(0);
        let column = location.as_ref().map(|l| l.column).unwrap_or(0);
        let byte_start = location.as_ref().and_then(|l| l.byte_start);
        let byte_end = location.as_ref().and_then(|l| l.byte_end);

        let mut json = serde_json::json!({
            "severity": severity,
            "message": message,
            "file": file,
            "line": line,
            "column": column,
            "code": code
        });

        // Include byte spans when available (for precise AI code edits)
        if let Some(bs) = byte_start {
            json["byte_start"] = serde_json::json!(bs);
        }
        if let Some(be) = byte_end {
            json["byte_end"] = serde_json::json!(be);
        }

        json
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

        /// Memory budget tier (embedded, minimal, standard, heavy, canvas)
        #[arg(long, default_value = None)]
        memory_tier: Option<String>,

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

        /// Target platform for bridge generation (browser, node, ios, android, server)
        #[arg(short, long, default_value = "server")]
        target: String,

        /// Memory budget tier (embedded, minimal, standard, heavy, canvas)
        #[arg(long, default_value = None)]
        memory_tier: Option<String>,

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

        /// Output AST as JSON
        #[arg(long)]
        ast_json: bool,
    },
    /// Run a Clean Language source file or WebAssembly binary
    Run {
        /// Input file to run (.cln source file or .wasm binary)
        input: String,

        /// Enable debug output
        #[arg(short, long)]
        debug: bool,
    },
    /// Start a development HTTP server for a Clean Language web application
    Serve {
        /// Input file to serve (.cln source file)
        input: String,

        /// Port to listen on (default: 3000)
        #[arg(short, long, default_value_t = 3000)]
        port: u16,

        /// Host to bind to (default: 0.0.0.0)
        #[arg(long, default_value = "0.0.0.0")]
        host: String,

        /// Enable debug output
        #[arg(short, long)]
        debug: bool,

        /// Watch for file changes and auto-reload
        #[arg(short, long)]
        watch: bool,
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
    /// Fast type-check a Clean Language file (no WASM generation)
    Check {
        /// Input file to type-check
        input: String,
    },
    /// Watch a directory for .cln file changes and type-check on save
    Watch {
        /// Directory or file to watch
        path: String,

        /// Output newline-delimited JSON events (for AI/IDE integration)
        #[arg(long)]
        json_stream: bool,
    },
    /// Start the MCP server for AI agent communication
    #[command(name = "mcp-server")]
    McpServer {},
    /// Generate MCP configuration JSON for AI agent integration
    #[command(name = "mcp-config")]
    McpConfig {
        /// Output format: claude-desktop, vscode, generic (default: generic)
        #[arg(short, long, default_value = "generic")]
        format: String,
    },
    /// Configure compiler settings (e.g., telemetry)
    #[command(subcommand)]
    Config(ConfigCommands),
    /// View fix status of reported compiler errors
    Fixes {
        /// Show fixes since this version
        #[arg(long)]
        since: Option<String>,

        /// Show only pending (unresolved) reports
        #[arg(long)]
        pending: bool,
    },
    /// Report a compiler error manually
    Report {
        /// Error code to report (e.g., SYN001)
        #[arg(long)]
        error: Option<String>,

        /// Minimal reproduction code
        #[arg(long)]
        code: Option<String>,

        /// Description of the issue
        #[arg(long)]
        description: Option<String>,
    },
    /// Telemetry maintenance (flush pending reports, retry unconfirmed reports)
    #[command(subcommand)]
    Telemetry(TelemetryCommands),
    /// Inspect and manage the local dev-mode error queue
    #[command(name = "dev-queue", subcommand)]
    DevQueue(DevQueueCommands),
}

#[derive(Subcommand, Debug)]
enum TelemetryCommands {
    /// Re-POST any locally-stored error reports that never received a fingerprint
    /// from the backend, plus drain the offline queue.
    Flush,
}

#[derive(Subcommand, Debug)]
enum DevQueueCommands {
    /// List all dev-mode errors captured locally, newest first.
    List {
        /// Include full error message (not just first 80 chars)
        #[arg(long)]
        full: bool,
    },
    /// Show one entry in detail.
    Show {
        /// Fingerprint prefix (4+ chars)
        fingerprint: String,
    },
    /// Print just the number of distinct fingerprints. Machine-readable.
    Count,
    /// Remove one entry by fingerprint prefix.
    Clear {
        /// Fingerprint prefix (4+ chars)
        fingerprint: String,
    },
    /// Remove every entry.
    #[command(name = "clear-all")]
    ClearAll,
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

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    /// Get a configuration value
    Get {
        /// Configuration key (e.g., 'telemetry')
        key: String,
    },
    /// Set a configuration value
    Set {
        /// Configuration key (e.g., 'telemetry')
        key: String,
        /// Value to set (e.g., 'on', 'off')
        value: String,
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

    // Telemetry: first-run prompt + version change checks (interactive commands only)
    if !args.quiet && !args.json {
        if let Commands::Compile { .. } | Commands::Build { .. } | Commands::Run { .. } =
            &args.command
        {
            clean_language_compiler::telemetry::maybe_prompt_telemetry();
            clean_language_compiler::telemetry::check_version_change();
        }
    }

    match args.command {
        Commands::Build {
            input,
            output,
            lib_paths,
            opt_level,
            memory_tier,
            debug,
        } => {
            handle_build(
                input,
                output,
                lib_paths,
                opt_level,
                memory_tier,
                debug,
                &output_config,
            )
            .await?
        }
        Commands::Compile {
            input,
            output,
            opt_level,
            target,
            memory_tier,
            test,
            include_tests,
            plugins,
        } => {
            handle_compile(
                input,
                output,
                opt_level,
                target,
                memory_tier,
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
            ast_json,
        } => {
            if ast_json {
                handle_ast_json(input, &output_config).await?
            } else {
                handle_parse(input, show_tree, recover_errors).await?
            }
        }
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
        Commands::Serve {
            input,
            port,
            host,
            debug,
            watch,
        } => handle_serve(input, port, host, debug, watch, &output_config).await?,
        Commands::Check { input } => handle_check(input, &output_config).await?,
        Commands::Watch { path, json_stream } => {
            handle_watch(path, json_stream, &output_config).await?
        }
        Commands::McpServer {} => clean_language_compiler::mcp::run_mcp_server().await?,
        Commands::McpConfig { format } => handle_mcp_config(&format)?,
        Commands::Config(config_cmd) => handle_config(config_cmd, &output_config)?,
        Commands::Fixes { since, pending } => handle_fixes(since, pending, &output_config)?,
        Commands::Report {
            error,
            code,
            description,
        } => handle_report(error, code, description, &output_config)?,
        Commands::Telemetry(cmd) => match cmd {
            TelemetryCommands::Flush => {
                clean_language_compiler::telemetry::flush_pending_telemetry(true);
            }
        },
        Commands::DevQueue(cmd) => handle_dev_queue(cmd)?,
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

async fn handle_serve(
    input: String,
    port: u16,
    host: String,
    debug: bool,
    _watch: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::process::Command;

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
    let reset = if output_config.use_colors {
        "\x1b[0m"
    } else {
        ""
    };

    // Step 1: Compile the input file to WASM
    let wasm_output = format!("/tmp/cln-serve-{}.wasm", std::process::id());

    if !output_config.quiet {
        println!("{}Compiling{} {} to WASM...", cyan, reset, input);
    }

    // Compile with plugins enabled (server target for serve command)
    let compile_result = handle_compile(
        input.clone(),
        wasm_output.clone(),
        2,                    // opt_level
        "server".to_string(), // target - no bridge needed for serve
        None,                 // memory_tier - use target default
        false,                // test
        false,                // include_tests
        true,                 // plugins enabled
        output_config,
    )
    .await;

    if let Err(e) = compile_result {
        eprintln!("Compilation failed: {}", e);
        return Err(e);
    }

    // Step 2: Find clean-server
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let server_paths = [
        format!("{}/.cleen/server/1.0.0/clean-server", home),
        "/usr/local/bin/clean-server".to_string(),
    ];

    let server_path = server_paths
        .iter()
        .find(|p| std::path::Path::new(p).exists());

    let server_path = match server_path {
        Some(p) => p.clone(),
        None => {
            eprintln!("Error: clean-server not found. Install it with:");
            eprintln!("   cleen server install");
            return Err("clean-server not found".into());
        }
    };

    if !output_config.quiet {
        println!(
            "{}Starting server{} on http://{}:{}",
            green, reset, host, port
        );
    }

    // Step 3: Run clean-server with the compiled WASM
    let mut cmd = Command::new(&server_path);
    cmd.arg(&wasm_output)
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg(&host);

    if debug {
        cmd.arg("--verbose");
    }

    let status = cmd.status()?;

    // Clean up temp file
    let _ = std::fs::remove_file(&wasm_output);

    if !status.success() {
        return Err(format!("Server exited with status: {}", status).into());
    }

    Ok(())
}

async fn handle_watch(
    path: String,
    json_stream: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc;

    let watch_path = std::path::Path::new(&path);
    if !watch_path.exists() {
        return Err(format!("Path does not exist: {}", path).into());
    }

    if !json_stream && !output_config.quiet {
        eprintln!("Watching {} for .cln file changes...", path);
        eprintln!("Press Ctrl+C to stop.");
    }

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(watch_path.as_ref(), RecursiveMode::Recursive)?;

    // Type-check a single .cln file and output result
    let check_file = |file_path: &std::path::Path| {
        let file_str = file_path.to_string_lossy().to_string();
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                if json_stream {
                    println!(
                        "{}",
                        serde_json::json!({
                            "event": "error",
                            "file": file_str,
                            "message": format!("Failed to read: {}", e)
                        })
                    );
                } else {
                    eprintln!("Error reading {}: {}", file_str, e);
                }
                return;
            }
        };

        let start = std::time::Instant::now();
        match clean_language_compiler::type_check(&source, &file_str) {
            Ok(result) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                if json_stream {
                    println!(
                        "{}",
                        serde_json::json!({
                            "event": "check",
                            "file": file_str,
                            "success": true,
                            "functions": result.function_count,
                            "types": result.type_count,
                            "duration_ms": duration_ms,
                            "diagnostics": []
                        })
                    );
                } else if !output_config.quiet {
                    let green = if output_config.use_colors {
                        "\x1b[32m"
                    } else {
                        ""
                    };
                    let reset = if output_config.use_colors {
                        "\x1b[0m"
                    } else {
                        ""
                    };
                    eprintln!("{}OK{} {} ({}ms)", green, reset, file_str, duration_ms);
                }
            }
            Err(errors) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                if json_stream {
                    let diagnostics: Vec<serde_json::Value> = errors
                        .iter()
                        .map(|e| output_config.error_to_json(e))
                        .collect();
                    println!(
                        "{}",
                        serde_json::json!({
                            "event": "check",
                            "file": file_str,
                            "success": false,
                            "duration_ms": duration_ms,
                            "diagnostics": diagnostics
                        })
                    );
                } else {
                    output_config.report_errors(&errors, Some(&source));
                }
            }
        }
    };

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    for path in &event.paths {
                        if path.extension().is_some_and(|ext| ext == "cln") {
                            check_file(path);
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                if json_stream {
                    println!(
                        "{}",
                        serde_json::json!({
                            "event": "error",
                            "message": format!("Watch error: {}", e)
                        })
                    );
                } else {
                    eprintln!("Watch error: {}", e);
                }
            }
            Err(e) => {
                return Err(format!("Channel error: {}", e).into());
            }
        }
    }
}

async fn handle_check(
    input: String,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(&input)
        .map_err(|e| format!("Failed to read '{}': {}", input, e))?;

    let start_time = std::time::Instant::now();

    match clean_language_compiler::type_check(&source, &input) {
        Ok(result) => {
            let duration = start_time.elapsed();

            if output_config.json_mode {
                let json = serde_json::json!({
                    "success": result.success,
                    "file": input,
                    "functions": result.function_count,
                    "types": result.type_count,
                    "duration_ms": duration.as_millis() as u64,
                    "diagnostics": []
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                );
            } else if !output_config.quiet {
                let green = if output_config.use_colors {
                    "\x1b[32m"
                } else {
                    ""
                };
                let reset = if output_config.use_colors {
                    "\x1b[0m"
                } else {
                    ""
                };
                println!(
                    "{}OK{} {} ({} functions, {} types, {}ms)",
                    green,
                    reset,
                    input,
                    result.function_count,
                    result.type_count,
                    duration.as_millis()
                );
            }
        }
        Err(errors) => {
            if output_config.json_mode {
                let diagnostics: Vec<serde_json::Value> = errors
                    .iter()
                    .map(|e| output_config.error_to_json(e))
                    .collect();
                let json = serde_json::json!({
                    "success": false,
                    "file": input,
                    "diagnostics": diagnostics,
                    "duration_ms": start_time.elapsed().as_millis() as u64,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                );
            } else {
                output_config.report_errors(&errors, Some(&source));
            }
            return Err(format!(
                "Type check failed with {} error{}",
                errors.len(),
                if errors.len() == 1 { "" } else { "s" }
            )
            .into());
        }
    }

    Ok(())
}

async fn handle_build(
    input: String,
    output: Option<String>,
    lib_paths: Vec<PathBuf>,
    opt_level: u8,
    memory_tier_str: Option<String>,
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

    // Parse explicit CLI tier (if any) and compute target default
    let explicit_tier = parse_memory_tier_flag(memory_tier_str.as_deref())?;
    let target_default = clean_language_compiler::MemoryTier::default_for_target("auto");

    // Use the multi-file compiler with memory tier
    let wasm_binary = clean_language_compiler::compile_multi_file_with_memory_tier(
        &input,
        lib_paths,
        opt_level,
        explicit_tier,
        target_default,
    )
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
    target: String,
    memory_tier_str: Option<String>,
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
        let target_info = if target != "server" {
            format!(" [target: {}]", target)
        } else {
            String::new()
        };
        println!(
            "Compiling {input} to {output} (optimization: -O{opt_level} {opt_desc}){plugin_mode}{target_info}"
        );
    }

    // Get the input file path and its directory for search paths
    let input_path = Path::new(&input);
    let search_paths = if let Some(parent) = input_path.parent() {
        if parent.as_os_str().is_empty() {
            vec![PathBuf::from(".")]
        } else {
            vec![parent.to_path_buf()]
        }
    } else {
        vec![PathBuf::from(".")]
    };

    tracing::debug!(
        input = %input,
        opt_level = opt_level,
        plugins = plugins,
        target = %target,
        search_paths = ?search_paths,
        "Starting multi-file compilation"
    );

    // Parse explicit CLI tier (if any) and compute target default
    let explicit_tier = parse_memory_tier_flag(memory_tier_str.as_deref())?;
    let target_default = clean_language_compiler::MemoryTier::default_for_target(&target);

    // Use multi-file compilation to support file path imports
    // This automatically handles `import "path/to/file.cln"` syntax
    // as well as module imports like `import Math`
    let _ = plugins; // Ignore the flag for now, multi-file compilation handles imports
    let wasm_binary = match clean_language_compiler::compile_multi_file_with_memory_tier(
        input_path,
        search_paths,
        opt_level,
        explicit_tier,
        target_default,
    ) {
        Ok(binary) => binary,
        Err(errors) => {
            let source = fs::read_to_string(&input).unwrap_or_default();
            output_config.report_errors(&errors, Some(&source));
            // Auto-report compile failure if telemetry is enabled
            clean_language_compiler::telemetry::report_compile_failure(&errors, &input);
            return Err(format!("Compilation failed with {} errors", errors.len()).into());
        }
    };

    // Note: Tests are not currently supported in the 7-stage pipeline
    if test {
        println!("⚠️  Test execution not yet implemented in 7-stage pipeline");
    }

    if let Some(parent) = Path::new(&output).parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&output, &wasm_binary)?;

    println!("Successfully compiled to {output}");

    // Generate bridge files based on target
    let bridge_target = match target.to_lowercase().as_str() {
        "browser" | "web" => Some(BridgeTarget::Browser),
        "node" | "nodejs" => Some(BridgeTarget::Node),
        "ios" | "macos" | "apple" => Some(BridgeTarget::iOS),
        "android" => Some(BridgeTarget::Android),
        "server" | "wasi" | "native" => None, // Server/native targets don't need bridge files
        _ => {
            if !output_config.quiet {
                println!(
                    "⚠️  Unknown target '{}', skipping bridge generation",
                    target
                );
            }
            None
        }
    };

    if let Some(bridge_target) = bridge_target {
        let output_path = Path::new(&output);
        let output_dir = output_path.parent().unwrap_or(Path::new("."));
        let wasm_filename = output_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "output.wasm".to_string());

        let generator = BridgeGenerator::new(bridge_target, output_dir, &wasm_filename);
        match generator.generate() {
            Ok(result) => {
                if !output_config.quiet && !result.generated_files.is_empty() {
                    println!("Generated {} bridge files:", bridge_target.name());
                    for file in &result.generated_files {
                        println!("  → {}", file.display());
                    }
                }
            }
            Err(e) => {
                eprintln!("⚠️  Failed to generate bridge files: {}", e);
            }
        }
    }

    Ok(())
}

/// Parse `--memory-tier` CLI flag value into `Option<MemoryTier>`.
///
/// Returns `Ok(Some(tier))` if explicitly provided, `Ok(None)` if absent,
/// or `Err` if the provided string is not a valid tier name.
fn parse_memory_tier_flag(
    tier_str: Option<&str>,
) -> Result<Option<clean_language_compiler::MemoryTier>, Box<dyn std::error::Error>> {
    match tier_str {
        Some(s) => {
            let tier = clean_language_compiler::MemoryTier::from_str(s).ok_or_else(|| {
                format!(
                    "Unknown memory tier '{}'. Valid values: embedded, minimal, standard, heavy, canvas",
                    s
                )
            })?;
            Ok(Some(tier))
        }
        None => Ok(None),
    }
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

    let test_source = "start:\n\tinteger x = 42\n\tprint(x)\n";

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

async fn handle_ast_json(
    input: String,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(&input)
        .map_err(|e| format!("Failed to read '{}': {}", input, e))?;

    match clean_language_compiler::parse_to_ast(&source, &input) {
        Ok(ast) => {
            println!("{}", serde_json::to_string_pretty(&ast).unwrap_or_default());
        }
        Err(errors) => {
            output_config.report_errors(&errors, Some(&source));
            return Err(format!(
                "Parse failed with {} error{}",
                errors.len(),
                if errors.len() == 1 { "" } else { "s" }
            )
            .into());
        }
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
    use wasmtime::{Linker, Module, Store};

    // Create engine and store using minimal configuration for direct execution
    let engine = CleanWasmtimeConfig::create_minimal_engine()?;
    let mut store = Store::new(&engine, ());

    // Create module
    let module = Module::new(&engine, &wasm_bytes)?;

    // Create linker and register all host functions using centralized registry
    let mut linker = Linker::new(&engine);
    clean_language_compiler::runtime::host_functions::register_all_host_functions(&mut linker)?;

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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
                      start:",
            fix: "Add at least one statement to the function body:\n\
                  myFunc()\n\
                  \treturn 0\n\n\
                  Or use 'pass' for empty functions that will be implemented later.",
        },
        "SYN009" => ErrorExplanation {
            title: "Unexpected end of file",
            description: "The file ended unexpectedly while the parser was expecting more content.\n\
                         This usually means an unclosed block or incomplete statement.",
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
                      \tx = someComplexExpression  // Type unclear",
            fix: "Provide an explicit type annotation:\n\
                  integer x = someComplexExpression\n\n\
                  Or simplify the expression so the type is clear.",
        },
        "TYP008" => ErrorExplanation {
            title: "Undefined method",
            description: "A method was called on a type that doesn't have that method.\n\
                         Check the available methods for the type you're using.",
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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
            example: "start:\n\
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

fn handle_mcp_config(format: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Find the cln binary path
    let cln_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "cln".to_string());

    match format {
        "claude-desktop" | "claude" => {
            let config = serde_json::json!({
                "mcpServers": {
                    "clean-language": {
                        "command": cln_path,
                        "args": ["mcp-server"]
                    }
                }
            });
            println!("{}", serde_json::to_string_pretty(&config)?);
            eprintln!();
            eprintln!("Add the JSON above to your claude_desktop_config.json file.");
            eprintln!("Location: ~/Library/Application Support/Claude/claude_desktop_config.json");
        }
        "vscode" | "cursor" => {
            let config = serde_json::json!({
                "mcp": {
                    "servers": {
                        "clean-language": {
                            "command": cln_path,
                            "args": ["mcp-server"]
                        }
                    }
                }
            });
            println!("{}", serde_json::to_string_pretty(&config)?);
            eprintln!();
            eprintln!(
                "Add the JSON above to your VS Code/Cursor settings.json or .vscode/mcp.json"
            );
        }
        "claude-code" => {
            let config = serde_json::json!({
                "mcpServers": {
                    "clean-language": {
                        "command": cln_path,
                        "args": ["mcp-server"]
                    }
                }
            });
            println!("{}", serde_json::to_string_pretty(&config)?);
            eprintln!();
            eprintln!(
                "Add the JSON above to your .claude/settings.json or project .mcp.json file."
            );
        }
        _ => {
            // Generic format with full instructions
            let config = serde_json::json!({
                "server": {
                    "name": "clean-language",
                    "command": cln_path,
                    "args": ["mcp-server"],
                    "transport": "stdio"
                },
                "instructions": {
                    "step_1": "Start the MCP server: cln mcp-server",
                    "step_2": "The server reads JSON-RPC from stdin, writes to stdout",
                    "step_3": "First call: tools/call get_quick_reference to learn the language",
                    "step_4": "Use 'check' for fast type-checking, 'compile' for WASM output"
                },
                "available_tools": [
                    "get_quick_reference", "check", "compile", "parse",
                    "diagnostics", "explain_error", "list_functions",
                    "list_types", "list_builtins", "get_specification",
                    "list_error_codes", "list_plugins"
                ]
            });
            println!("{}", serde_json::to_string_pretty(&config)?);
            eprintln!();
            eprintln!("Formats: cln mcp-config --format claude-desktop|vscode|claude-code|generic");
        }
    }

    Ok(())
}

// ============================================================================
// Config, Fixes, and Report command handlers
// ============================================================================

fn handle_config(
    cmd: ConfigCommands,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    use clean_language_compiler::telemetry::TelemetryConfig;

    match cmd {
        ConfigCommands::Get { key } => match key.as_str() {
            "telemetry" => {
                let config = TelemetryConfig::load();
                if output_config.json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&config).unwrap_or_else(|_| "{}".to_string())
                    );
                } else {
                    println!("Telemetry: {}", if config.enabled { "on" } else { "off" });
                    println!("Consent level: {}", config.consent_level);
                    println!("Anonymous ID: {}", config.anonymous_id);
                    println!(
                        "Email: {}",
                        config.contact_email.as_deref().unwrap_or("(not set)")
                    );
                }
            }
            "email" => {
                let config = TelemetryConfig::load();
                println!("{}", config.contact_email.as_deref().unwrap_or("(not set)"));
            }
            other => {
                eprintln!("Unknown config key: {}", other);
                eprintln!("Available keys: telemetry, email");
                std::process::exit(1);
            }
        },
        ConfigCommands::Set { key, value } => match key.as_str() {
            "telemetry" => {
                let mut config = TelemetryConfig::load();
                config.enabled = matches!(value.as_str(), "on" | "true" | "yes" | "1");
                config.save()?;
                println!(
                    "Telemetry {}",
                    if config.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );
            }
            "email" => {
                let mut config = TelemetryConfig::load();
                if value.is_empty() || value == "off" || value == "none" || value == "clear" {
                    config.contact_email = None;
                    config.save()?;
                    println!("Email cleared");
                } else {
                    config.contact_email = Some(value);
                    config.save()?;
                    println!("Email saved. You'll be notified when reported bugs are fixed.");
                }
            }
            other => {
                eprintln!("Unknown config key: {}", other);
                eprintln!("Available keys: telemetry, email");
                std::process::exit(1);
            }
        },
    }
    Ok(())
}

fn handle_dev_queue(cmd: DevQueueCommands) -> Result<(), Box<dyn std::error::Error>> {
    use clean_language_compiler::telemetry::dev_queue;
    match cmd {
        DevQueueCommands::Count => {
            println!("{}", dev_queue::count());
        }
        DevQueueCommands::List { full } => {
            let mut entries = dev_queue::load();
            if entries.is_empty() {
                println!("Dev queue is empty.");
                return Ok(());
            }
            // Newest first by last_seen_at
            entries.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at));
            println!(
                "{:<16}  {:<8}  {:>5}  {:<19}  {}",
                "fingerprint", "code", "×", "last seen (UTC)", "message"
            );
            println!("{}", "-".repeat(96));
            for e in &entries {
                let msg = if full {
                    e.message.clone()
                } else {
                    let mut s = e.message.replace('\n', " ");
                    if s.len() > 80 {
                        s.truncate(77);
                        s.push_str("...");
                    }
                    s
                };
                let ts = e.last_seen_at.trim_end_matches('Z').replace('T', " ");
                println!(
                    "{:<16}  {:<8}  {:>5}  {:<19}  {}",
                    e.fingerprint, e.error_code, e.occurrences, ts, msg
                );
            }
            println!();
            println!(
                "{} distinct error(s) in dev queue. Show one: `cln dev-queue show <prefix>`",
                entries.len()
            );
        }
        DevQueueCommands::Show { fingerprint } => {
            let entries = dev_queue::load();
            let matches: Vec<_> = entries
                .iter()
                .filter(|e| e.fingerprint.starts_with(&fingerprint))
                .collect();
            match matches.len() {
                0 => {
                    eprintln!("No entry matching prefix '{}'", fingerprint);
                    std::process::exit(1);
                }
                1 => {
                    let e = matches[0];
                    println!("Fingerprint:     {}", e.fingerprint);
                    println!("Error code:      {}", e.error_code);
                    println!("Component:       {}", e.component);
                    println!("Occurrences:     {}", e.occurrences);
                    println!("First seen:      {}", e.first_seen_at);
                    println!("Last seen:       {}", e.last_seen_at);
                    println!("Compiler:        {}", e.compiler_version);
                    println!("Dev reason:      {}", e.dev_reason);
                    if let Some(ref f) = e.file_context {
                        println!("Source:          {}", f);
                    }
                    println!();
                    println!("Message:");
                    println!("{}", e.message);
                }
                n => {
                    eprintln!("Ambiguous prefix '{}' matches {} entries:", fingerprint, n);
                    for e in &matches {
                        eprintln!("  {}  {}  ×{}", e.fingerprint, e.error_code, e.occurrences);
                    }
                    std::process::exit(1);
                }
            }
        }
        DevQueueCommands::Clear { fingerprint } => match dev_queue::clear_by_prefix(&fingerprint) {
            Ok(true) => println!("Removed entry matching '{}'.", fingerprint),
            Ok(false) => {
                eprintln!("No entry matching '{}'", fingerprint);
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        DevQueueCommands::ClearAll => match dev_queue::clear_all() {
            Ok(n) => println!("Cleared {} entries from dev queue.", n),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
    }
    Ok(())
}

fn handle_fixes(
    since: Option<String>,
    pending_only: bool,
    output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    use clean_language_compiler::telemetry::{ReportStatus, ReportStore};

    let store = ReportStore::load();
    let reports = store.get_all_reports();

    if reports.is_empty() {
        if output_config.json_mode {
            println!("{{\"reports\": []}}");
        } else {
            println!("No reported errors tracked on this machine.");
            println!("Use the MCP report_error tool or `cln report` to report compiler bugs.");
        }
        return Ok(());
    }

    // Apply filters
    let filtered: Vec<_> = reports
        .iter()
        .filter(|r| {
            if pending_only {
                return r.status != ReportStatus::Resolved;
            }
            if let Some(ref ver) = since {
                return r.compiler_version.as_str() >= ver.as_str();
            }
            true
        })
        .collect();

    if output_config.json_mode {
        let json_reports: Vec<serde_json::Value> = filtered
            .iter()
            .map(|r| {
                serde_json::json!({
                    "report_id": r.report_id,
                    "error_code": r.error_code,
                    "summary": r.summary,
                    "status": r.status.to_string(),
                    "resolved_in": r.resolved_in,
                    "reported_at": r.reported_at.to_rfc3339()
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_reports)?);
        return Ok(());
    }

    println!("Your reported errors — Fix Status");
    println!("{}", "\u{2501}".repeat(50));
    println!();

    let mut resolved_count = 0;
    for r in &filtered {
        let symbol = match r.status {
            ReportStatus::Resolved => {
                resolved_count += 1;
                "\u{2713}"
            }
            ReportStatus::InProgress | ReportStatus::Acknowledged => "\u{25F7}",
            ReportStatus::WontFix => "\u{2717}",
            ReportStatus::Reported => "\u{25CB}",
        };

        let status_detail = match &r.status {
            ReportStatus::Resolved => r
                .resolved_in
                .as_ref()
                .map(|v| format!("Fixed in {}", v))
                .unwrap_or_else(|| "Fixed".to_string()),
            ReportStatus::InProgress => "In progress".to_string(),
            ReportStatus::Acknowledged => "Acknowledged".to_string(),
            ReportStatus::WontFix => "Won't fix".to_string(),
            ReportStatus::Reported => {
                let days = (chrono::Utc::now() - r.reported_at).num_days();
                if days == 0 {
                    "Reported today".to_string()
                } else if days == 1 {
                    "Reported yesterday".to_string()
                } else {
                    format!("Reported {} days ago", days)
                }
            }
        };

        println!(
            "{} {:<8} {:<40} {}",
            symbol,
            r.error_code,
            truncate_string(&r.summary, 40),
            status_detail
        );
    }

    println!();
    println!(
        "You've reported {} errors. {} have been fixed.",
        filtered.len(),
        resolved_count
    );

    Ok(())
}

fn handle_report(
    error: Option<String>,
    code: Option<String>,
    description: Option<String>,
    _output_config: &OutputConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    use clean_language_compiler::telemetry::{
        submit_report, ErrorReport, ReportError, ReportStore,
    };

    let error_code = error.unwrap_or_else(|| "USR001".to_string());
    let message = description.unwrap_or_else(|| "Manually reported error".to_string());

    // Generate report ID
    let report_id = {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let a: u32 = rng.gen();
        let b: u16 = rng.gen();
        let c: u16 = (rng.gen::<u16>() & 0x0FFF) | 0x4000;
        let d: u16 = (rng.gen::<u16>() & 0x3FFF) | 0x8000;
        let e: u64 = rng.gen::<u64>() & 0xFFFF_FFFF_FFFF;
        format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", a, b, c, d, e)
    };

    let mut report = ErrorReport::new(
        report_id.clone(),
        ReportError {
            code: error_code.clone(),
            category: "unknown".to_string(),
            component: "compiler".to_string(),
            subsystem: None,
            severity: "bug".to_string(),
            message: message.clone(),
            file_context: None,
        },
        "manual",
        "error_with_code",
    );

    if let Some(ref repro_code) = code {
        report.reproduction = Some(
            clean_language_compiler::telemetry::report::ReportReproduction {
                minimal_code: Some(repro_code.clone()),
                expected_behavior: None,
                actual_behavior: None,
                spec_reference: None,
            },
        );
    }

    // Attach stored email if available
    {
        let config = clean_language_compiler::telemetry::TelemetryConfig::load();
        if let Some(ref email) = config.contact_email {
            report.user.anonymous = false;
            report.user.contact = Some(email.clone());
        }
    }

    // Dev-mode gate: if this `cln report` invocation is happening inside a
    // component source tree, record to the local dev queue instead of
    // publishing. Override with CLEEN_TELEMETRY_FORCE=publish.
    let dev_ctx = clean_language_compiler::telemetry::detect_dev_context_for_component(
        &report.error.component,
    );
    if dev_ctx.is_dev() {
        let entry = clean_language_compiler::telemetry::dev_queue::entry_from(
            &dev_ctx,
            &report.error.code,
            &report.error.component,
            &report.error.message,
            report.error.file_context.as_deref(),
            clean_language_compiler::VERSION,
        );
        let outcome = clean_language_compiler::telemetry::dev_queue::append(entry);
        println!(
            "Recorded locally in dev queue (not uploaded).\nReason: {}\nComponent: {}\nError: {} \u{00d7}{} (fingerprint {})",
            dev_ctx.reason().unwrap_or("dev"),
            report.error.component,
            report.error.code,
            outcome.occurrences,
            outcome.fingerprint,
        );
        println!("View: cln dev-queue list  |  Override: CLEEN_TELEMETRY_FORCE=publish");
        return Ok(());
    }

    // Track locally
    let mut store = ReportStore::load();
    store.add_report(&report);
    let _ = store.save();

    // Submit
    let result = submit_report(&report);

    match result {
        clean_language_compiler::telemetry::SubmitResult::AlreadyFixed {
            fixed_in_version,
            fix_description,
            message,
            ..
        } => {
            println!("{}", message);
            println!("Fixed in: v{}", fixed_in_version);
            if let Some(desc) = fix_description {
                println!("Fix: {}", desc);
            }
            println!("Update with: cleen install latest");
        }
        clean_language_compiler::telemetry::SubmitResult::Known {
            occurrences,
            current_status,
            message,
            ..
        } => {
            println!("{}", message);
            println!("Status: {} ({} reports)", current_status, occurrences);
        }
        clean_language_compiler::telemetry::SubmitResult::Submitted {
            report_id,
            fingerprint,
            tracking_url,
        } => {
            println!("Error report submitted successfully!");
            println!("Report ID: {}", report_id);
            if let Some(fp) = fingerprint {
                println!("Fingerprint: {}", fp);
            }
            println!("Tracking: {}", tracking_url);
        }
        clean_language_compiler::telemetry::SubmitResult::Queued {
            report_id,
            local_path,
        } => {
            println!("Report saved locally (backend not yet available).");
            println!("Report ID: {}", report_id);
            println!("Local path: {}", local_path);
            println!("It will be sent when the error reporting service is available.");
        }
        clean_language_compiler::telemetry::SubmitResult::RateLimited {
            report_id,
            local_path,
            retry_after_seconds,
        } => {
            println!("Report saved locally — backend is rate-limiting submissions.");
            println!("Report ID: {}", report_id);
            println!("Local path: {}", local_path);
            println!("Retry will be attempted after {}s.", retry_after_seconds);
        }
        clean_language_compiler::telemetry::SubmitResult::Error { message } => {
            eprintln!("Failed to save report: {}", message);
        }
    }

    println!();
    println!("Thank you for helping improve Clean Language!");

    Ok(())
}

/// Truncate a string to max_len, adding "..." if truncated
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
