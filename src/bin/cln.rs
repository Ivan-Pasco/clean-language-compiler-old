/*!
 * Clean Language Compiler - Command Line Interface
 *
 * Author: Ivan Pasco Lizarraga
 * Date: 17-07-2025
 * Website: https://www.cleanlanguage.dev
 *
 * A modern, type-safe programming language that compiles to WebAssembly
 */

use clean_language_compiler::codegen::CodeGenerator;
use clean_language_compiler::error::CompilerError;
use clean_language_compiler::parser::CleanParser;
use clean_language_compiler::semantic::SemanticAnalyzer;
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
            if args.len() < 3 {
                eprintln!("❌ Error: No input file specified.");
                print_usage();
                return Ok(());
            }

            let input_file = &args[2];
            let output_file = if args.len() >= 4 {
                args[3].clone()
            } else {
                // Generate output filename from input
                match Path::new(input_file).file_stem() {
                    Some(stem) => format!("{}.wasm", stem.to_string_lossy()),
                    None => format!("{input_file}.wasm"),
                }
            };

            compile_file(input_file, &output_file)
        }
        "run" => {
            if args.len() < 3 {
                eprintln!("❌ Error: No input file specified.");
                print_usage();
                return Ok(());
            }

            let input_file = &args[2];
            run_file(input_file)
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
    println!("    version                     Show version information");
    println!("    help                        Show this help message");
    println!();
    println!("EXAMPLES:");
    println!("    cln compile hello.cln               # Compiles to hello.wasm");
    println!("    cln compile hello.cln hello.wasm    # Compiles with custom output");
    println!("    cln run hello.cln                   # Compile and execute");
    println!("    cln parse hello.cln                 # Check syntax only");
    println!("    cln check hello.cln                 # Type checking only");
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

    // Read the WASM file
    let wasm_bytes = fs::read(wasm_file).map_err(|e| {
        CompilerError::io_error(format!("Failed to read WASM file: {e}"), None, None)
    })?;

    // Use the async runtime to execute
    if let Ok(rt) = tokio::runtime::Runtime::new() {
        rt.block_on(async {
            match clean_language_compiler::runtime::run_clean_program_async(&wasm_bytes).await {
                Ok(()) => {
                    println!("✅ Execution completed successfully!");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("❌ Execution failed: {e}");
                    Err(CompilerError::runtime_error(
                        format!("Failed to execute WASM: {e}"),
                        None,
                        None,
                    ))
                }
            }
        })
    } else {
        Err(CompilerError::runtime_error(
            "Failed to create async runtime".to_string(),
            None,
            None,
        ))
    }
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
            eprintln!("❌ Type Error: {}", context.message);

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
            eprintln!("❌ Error: {}", error);
        }
    }

    eprintln!();
}
