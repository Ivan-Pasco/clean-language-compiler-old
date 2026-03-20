#![allow(clippy::single_component_path_imports)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::needless_borrow)]

use clean_language_compiler::{
    compile_with_external_plugins, compile_with_target, CompilationTarget,
};
use std::env;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

            // Parse optional arguments
            let mut output_file = None;
            let mut target = CompilationTarget::Server; // Default to Server

            let mut i = 3;
            while i < args.len() {
                if args[i].starts_with("--target=") {
                    let target_str = args[i].trim_start_matches("--target=");
                    target = match CompilationTarget::from_str(target_str) {
                        Some(t) => t,
                        None => {
                            eprintln!("Error: Unknown target '{}'. Valid targets: server, plugin, standalone", target_str);
                            std::process::exit(1);
                        }
                    };
                } else if !args[i].starts_with("--") && output_file.is_none() {
                    output_file = Some(args[i].clone());
                }
                i += 1;
            }

            // Default output file if not specified
            let output_file =
                output_file.unwrap_or_else(|| match Path::new(input_file).file_stem() {
                    Some(stem) => format!("{}.wasm", stem.to_string_lossy()),
                    None => format!("{}.wasm", input_file),
                });

            compile_file(input_file, &output_file, target)?;
        }
        "run" => {
            if args.len() < 3 {
                println!("Error: No input file specified.");
                print_usage();
                return Ok(());
            }

            let input_file = &args[2];
            let temp_output = format!("{}.wasm", input_file);
            compile_file(input_file, &temp_output, CompilationTarget::Server)?;
            println!(
                "Compiled to {}. Run it with: wasmtime {}",
                temp_output, temp_output
            );
        }
        "help" => {
            print_usage();
        }
        _ => {
            println!("Unknown command: {}", args[1]);
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Clean Language Compiler (7-stage pipeline)");
    println!("Usage:");
    println!("  cleanc compile <input-file> [output-file] [--target=<target>]");
    println!("                                           # Compile a Clean program to WebAssembly");
    println!("  cleanc run <input-file>                  # Compile and run a Clean program");
    println!("  cleanc help                              # Show this help message");
    println!();
    println!("Compilation targets:");
    println!("  --target=server     Include all HTTP server imports (default)");
    println!("  --target=plugin     Minimal imports for WASM plugins (excludes server functions)");
    println!("  --target=standalone Standard CLI/library compilation");
}

fn compile_file(
    input_file: &str,
    output_file: &str,
    target: CompilationTarget,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Compiling {} to {} (target: {:?})...",
        input_file, output_file, target
    );

    // Read the input file
    let source = fs::read_to_string(input_file)?;

    // Use target-aware compilation if not Server, otherwise use plugin-aware compilation
    let wasm_binary = if target == CompilationTarget::Server {
        // Server target: use full pipeline with plugin support
        match compile_with_external_plugins(&source, input_file) {
            Ok(binary) => binary,
            Err(errors) => {
                eprintln!("❌ Compilation failed with {} errors:", errors.len());
                for (i, error) in errors.iter().enumerate() {
                    eprintln!("Error {}: {}", i + 1, error);
                }
                std::process::exit(1);
            }
        }
    } else {
        // Plugin/Standalone target: use target-aware compilation
        match compile_with_target(&source, input_file, target, 2) {
            Ok(binary) => binary,
            Err(errors) => {
                eprintln!("❌ Compilation failed with {} errors:", errors.len());
                for (i, error) in errors.iter().enumerate() {
                    eprintln!("Error {}: {}", i + 1, error);
                }
                std::process::exit(1);
            }
        }
    };

    // Write the output file
    fs::write(output_file, &wasm_binary)?;

    println!(
        "✅ Compilation successful! Output written to {} ({} bytes)",
        output_file,
        wasm_binary.len()
    );
    Ok(())
}
