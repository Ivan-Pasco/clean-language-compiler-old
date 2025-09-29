#![allow(clippy::single_component_path_imports)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::needless_borrow)]

use clean_language_compiler::compile_with_file;
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
            let output_file = if args.len() >= 4 {
                args[3].clone()
            } else {
                // Remove the extension (e.g. ".cln") safely and append ".wasm"
                match Path::new(input_file).file_stem() {
                    Some(stem) => format!("{}.wasm", stem.to_string_lossy()),
                    None => format!("{}.wasm", input_file), // fallback – should not happen
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
            println!("Unknown command: {}", args[1]);
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Clean Language Compiler (7-stage pipeline)");
    println!("Usage:");
    println!(
        "  cleanc compile <input-file> [output-file]  # Compile a Clean program to WebAssembly"
    );
    println!("  cleanc run <input-file>                   # Compile and run a Clean program");
    println!("  cleanc help                              # Show this help message");
}

fn compile_file(input_file: &str, output_file: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Compiling {} to {}...", input_file, output_file);

    // Read the input file
    let source = fs::read_to_string(input_file)?;

    // Use the 7-stage pipeline for compilation
    let wasm_binary = match compile_with_file(&source, input_file) {
        Ok(binary) => binary,
        Err(errors) => {
            eprintln!("❌ Compilation failed with {} errors:", errors.len());
            for (i, error) in errors.iter().enumerate() {
                eprintln!("Error {}: {}", i + 1, error);
            }
            std::process::exit(1);
        }
    };

    // Write the output file
    fs::write(output_file, wasm_binary)?;

    println!(
        "✅ Compilation successful! Output written to {}",
        output_file
    );
    Ok(())
}

fn execute_file(input_file: &str) -> Result<(), Box<dyn std::error::Error>> {
    // For now, just compile to a temporary file and notify user
    let temp_output = format!("{}.wasm", input_file);
    compile_file(input_file, &temp_output)?;

    println!("Note: Direct execution not yet implemented.");
    println!(
        "You can run the compiled WASM file using: wasmtime {}",
        temp_output
    );

    Ok(())
}
