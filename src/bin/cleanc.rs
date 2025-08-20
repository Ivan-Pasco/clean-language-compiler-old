#![allow(clippy::single_component_path_imports)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::needless_borrow)]

use clean_language_compiler::codegen::CodeGenerator;
use clean_language_compiler::error::CompilerError;
use clean_language_compiler::parser::CleanParser;
use clean_language_compiler::semantic::SemanticAnalyzer;
use std::env;
use std::fs;
use std::io::Read;
use std::path::Path;

fn main() -> Result<(), CompilerError> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "compile" => {
            if args.len() < 3 {
                println!("Error: No input file specified.");
                print_usage();
                return Ok(());
            }

            let input_file = &args[2];
            let output_file = if args.len() >= 4 {
                args[3].clone()
            } else {
                // Remove the extension (e.g. ".cln") safely and append ".wasm"
                match Path::new(input_file).file_stem() {
                    Some(stem) => format!("{stem}.wasm", stem = stem.to_string_lossy()),
                    None => format!("{input_file}.wasm"), // fallback – should not happen
                }
            };

            compile_file(input_file, &output_file)?;
        }
        "run" => {
            if args.len() < 3 {
                println!("Error: No input file specified.");
                print_usage();
                return Ok(());
            }

            let input_file = &args[2];
            execute_file(input_file)?;
        }
        "help" => {
            print_usage();
        }
        _ => {
            println!("Unknown command: {command}", command = args[1]);
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Clean Language Compiler");
    println!("Usage:");
    println!(
        "  cleanc compile <input-file> [output-file]  # Compile a Clean program to WebAssembly"
    );
    println!("  cleanc run <input-file>                   # Compile and run a Clean program");
    println!("  cleanc help                              # Show this help message");
}

fn compile_file(input_file: &str, output_file: &str) -> Result<(), CompilerError> {
    println!("Compiling {input_file} to {output_file}...");

    // Read the input file
    let mut source = String::new();
    let mut file = fs::File::open(input_file)
        .map_err(|e| CompilerError::io_error(format!("Failed to open file: {e}"), None, None))?;
    file.read_to_string(&mut source)
        .map_err(|e| CompilerError::io_error(format!("Failed to read file: {e}"), None, None))?;

    // Debug: Print source code
    println!("Source code:\n{source}");

    // Parse the program with enhanced error reporting
    let program = match CleanParser::parse_program_with_file(&source, input_file) {
        Ok(p) => p,
        Err(e) => {
            display_enhanced_error(&e, &source, input_file);
            std::process::exit(1);
        }
    };

    // Debug print the parsed AST
    println!("Parsed AST: {program:#?}");

    // Semantic analysis with enhanced error reporting
    let mut semantic_analyzer = SemanticAnalyzer::new();
    let analyzed_program = match semantic_analyzer.analyze(&program) {
        Ok(p) => p,
        Err(e) => {
            display_enhanced_error(&e, &source, input_file);
            std::process::exit(1);
        }
    };

    // Code generation
    let mut code_generator = CodeGenerator::new();
    let wasm_binary = code_generator.generate(&analyzed_program)?;

    // Write the output file
    fs::write(output_file, wasm_binary).map_err(|e| {
        CompilerError::io_error(format!("Failed to write output file: {e}"), None, None)
    })?;

    println!("Compilation successful!");
    Ok(())
}

fn execute_file(input_file: &str) -> Result<(), CompilerError> {
    println!("Executing {input_file}...");

    // Check if the file exists
    if !Path::new(input_file).exists() {
        // If the input is a .cln file, compile it first
        if input_file.ends_with(".cln") {
            let wasm_file = format!(
                "{file_stem}.wasm",
                file_stem = input_file.trim_end_matches(".cln")
            );
            compile_file(input_file, &wasm_file)?;
            return execute_file(&wasm_file);
        } else {
            return Err(CompilerError::io_error(
                format!("File not found: {input_file}"),
                None,
                None,
            ));
        }
    }

    // If it's not a WASM file, try to compile it first
    if !input_file.ends_with(".wasm") {
        let wasm_file = format!("{input_file}.wasm");
        compile_file(input_file, &wasm_file)?;
        return execute_file(&wasm_file);
    }

    // Read the WASM file
    let wasm_bytes = fs::read(input_file).map_err(|e| {
        CompilerError::io_error(format!("Failed to read WASM file: {e}"), None, None)
    })?;

    // Use wasmtime to execute the WASM file
    println!("Running WASM file with wasmtime...");
    match run_wasm_with_wasmtime(&wasm_bytes) {
        Ok(_) => {
            println!("Execution completed successfully!");
            Ok(())
        }
        Err(e) => Err(CompilerError::runtime_error(
            format!("Failed to execute WASM: {e}"),
            None,
            None,
        )),
    }
}

// Function to run a WebAssembly module with wasmtime
fn run_wasm_with_wasmtime(wasm_bytes: &[u8]) -> Result<(), CompilerError> {
    // For now, skip async runtime to focus on getting basic execution working
    println!("🚀 Executing WebAssembly with synchronous runtime...");
    run_wasm_sync(wasm_bytes)
}

// Synchronous WebAssembly execution (fallback)
#[allow(unused_mut)]
fn run_wasm_sync(wasm_bytes: &[u8]) -> Result<(), CompilerError> {
    use wasmtime::{Linker, Module, Store, Val};

    // Use minimal Clean Language wasmtime configuration for execution
    let engine = clean_language_compiler::runtime::wasmtime_config::CleanWasmtimeConfig::create_minimal_engine()?;

    // Create a module from the bytes
    let module = Module::new(&engine, wasm_bytes).map_err(|e| {
        CompilerError::runtime_error(
            format!("Failed to create WebAssembly module: {e}"),
            None,
            None,
        )
    })?;

    // Create a store
    let mut store = Store::new(&engine, ());

    // Create a linker
    let mut linker = Linker::new(&engine);

    // Add all host functions using centralized registry
    clean_language_compiler::runtime::host_functions::register_all_host_functions(&mut linker)?;

    let instance = linker.instantiate(&mut store, &module).map_err(|e| {
        CompilerError::runtime_error(
            format!("Failed to instantiate WebAssembly module: {e}"),
            None,
            None,
        )
    })?;

    // Try to get the start function
    if let Some(start) = instance.get_func(&mut store, "start") {
        // Check if the function takes no parameters
        let start_type = start.ty(&store);
        let results_len = start_type.results().len();

        // Create a buffer to store return values
        let mut results = vec![Val::I32(0); results_len];

        // Call the start function
        start.call(&mut store, &[], &mut results).map_err(|e| {
            CompilerError::runtime_error(format!("Failed to call start function: {e}"), None, None)
        })?;

        println!("Program executed successfully!");

        // If there are return values, print them
        if !results.is_empty() {
            println!("Return value: {:?}", results[0]);
        }

        return Ok(());
    }

    // If no start function, look for an _start function as fallback
    if let Some(start) = instance.get_func(&mut store, "_start") {
        // Check if the function takes no parameters
        let start_type = start.ty(&store);
        let results_len = start_type.results().len();

        // Create a buffer to store return values
        let mut results = vec![Val::I32(0); results_len];

        // Call the start function
        start.call(&mut store, &[], &mut results).map_err(|e| {
            CompilerError::runtime_error(format!("Failed to call _start function: {e}"), None, None)
        })?;

        println!("Program executed successfully!");

        // If there are return values, print them
        if !results.is_empty() {
            println!("Return value: {:?}", results[0]);
        }

        return Ok(());
    }

    // No suitable entry point found
    Err(CompilerError::runtime_error(
        "No suitable entry function found in the WebAssembly module",
        Some("The module should export a 'start' function with no parameters".to_string()),
        None,
    ))
}
/// Display enhanced error information with source snippets and suggestions
fn display_enhanced_error(error: &CompilerError, _source: &str, file_path: &str) {
    // ErrorUtils import removed as it's unused

    eprintln!("\n🚨 Compilation Error 🚨");
    eprintln!("File: {file_path}");
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

            if let Some(snippet) = &context.source_snippet {
                eprintln!("\n📝 Source Context:");
                eprintln!("{snippet}");
            }

            if let Some(help) = &context.help {
                eprintln!("💡 Help: {help}");
            }

            if !context.suggestions.is_empty() {
                eprintln!("\n🔧 Suggestions:");
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
                eprintln!("\n🔧 Suggestions:");
                for suggestion in &context.suggestions {
                    eprintln!("  • {suggestion}");
                }
            }
        }
        _ => {
            eprintln!("❌ Error: {error}");
        }
    }

    eprintln!("\n📚 For more help, check the Clean Language documentation.");
}
