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
    // Try to use the enhanced async runtime first
    if let Ok(rt) = tokio::runtime::Runtime::new() {
        return rt.block_on(async {
            match clean_language_compiler::runtime::run_clean_program_async(wasm_bytes).await {
                Ok(()) => Ok(()),
                Err(e) => {
                    // Show the actual error before falling back
                    println!("⚠️  Async runtime failed with error: {e}");
                    println!("   Falling back to synchronous execution");
                    run_wasm_sync(wasm_bytes)
                }
            }
        });
    }

    // Fallback to synchronous execution
    run_wasm_sync(wasm_bytes)
}

// Synchronous WebAssembly execution (fallback)
#[allow(unused_mut)]
fn run_wasm_sync(wasm_bytes: &[u8]) -> Result<(), CompilerError> {
    use wasmtime::{Caller, Config, Engine, Linker, Module, Store, Val};

    // Use default configuration - simpler and more compatible
    let config = Config::default();

    // Create the engine
    let engine = Engine::new(&config).map_err(|e| {
        CompilerError::runtime_error(
            format!("Failed to create WebAssembly engine: {e}"),
            None,
            None,
        )
    })?;

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

    // Add print functions to the linker

    // print(strPtr: i32, strLen: i32) -> void
    linker
        .func_wrap(
            "env",
            "print",
            |mut caller: Caller<'_, ()>, str_ptr: i32, str_len: i32| {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if str_ptr >= 0 && str_len >= 0 {
                            let start = str_ptr as usize;
                            let len = str_len as usize;

                            if start + len <= data.len() {
                                if let Ok(string) = std::str::from_utf8(&data[start..start + len]) {
                                    print!("{}", string);
                                } else {
                                    print!("[invalid UTF-8]");
                                }
                            } else {
                                print!("[out of bounds]");
                            }
                        } else {
                            print!("[invalid parameters]");
                        }
                    }
                }
                Ok(())
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create print function: {e}"),
                None,
                None,
            )
        })?;

    // printl(strPtr: i32, strLen: i32) -> void
    linker
        .func_wrap(
            "env",
            "printl",
            |mut caller: Caller<'_, ()>, str_ptr: i32, str_len: i32| {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if str_ptr >= 0 && str_len >= 0 {
                            let start = str_ptr as usize;
                            let len = str_len as usize;

                            if start + len <= data.len() {
                                if let Ok(string) = std::str::from_utf8(&data[start..start + len]) {
                                    println!("{string}");
                                } else {
                                    println!("[invalid UTF-8]");
                                }
                            } else {
                                println!("[out of bounds]");
                            }
                        } else {
                            println!("[invalid parameters]");
                        }
                    }
                }
                Ok(())
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create printl function: {e}"),
                None,
                None,
            )
        })?;

    // print_simple(value: i32) -> void
    linker
        .func_wrap(
            "env",
            "print_simple",
            |_caller: Caller<'_, ()>, value: i32| {
                print!("{}", value);
                Ok(())
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create print_simple function: {e}"),
                None,
                None,
            )
        })?;

    // printl_simple(value: i32) -> void
    linker
        .func_wrap(
            "env",
            "printl_simple",
            |_caller: Caller<'_, ()>, value: i32| {
                println!("{value}");
                Ok(())
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create printl_simple function: {e}"),
                None,
                None,
            )
        })?;

    // Add HTTP functions
    linker
        .func_wrap(
            "env",
            "http_get",
            |_caller: Caller<'_, ()>, _url_ptr: i32, _url_len: i32| -> i32 {
                println!("[HTTP GET] Mock response");
                0 // Return mock string pointer
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create http_get function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "http_post",
            |_caller: Caller<'_, ()>,
             _url_ptr: i32,
             _url_len: i32,
             _body_ptr: i32,
             _body_len: i32|
             -> i32 {
                println!("[HTTP POST] Mock response");
                0 // Return mock string pointer
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create http_post function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "http_put",
            |_caller: Caller<'_, ()>,
             _url_ptr: i32,
             _url_len: i32,
             _body_ptr: i32,
             _body_len: i32|
             -> i32 {
                println!("[HTTP PUT] Mock response");
                0 // Return mock string pointer
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create http_put function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "http_patch",
            |_caller: Caller<'_, ()>,
             _url_ptr: i32,
             _url_len: i32,
             _body_ptr: i32,
             _body_len: i32|
             -> i32 {
                println!("[HTTP PATCH] Mock response");
                0 // Return mock string pointer
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create http_patch function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "http_delete",
            |_caller: Caller<'_, ()>, _url_ptr: i32, _url_len: i32| -> i32 {
                println!("[HTTP DELETE] Mock response");
                0 // Return mock string pointer
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create http_delete function: {e}"),
                None,
                None,
            )
        })?;

    // Add type conversion functions
    linker
        .func_wrap(
            "env",
            "int_to_string",
            |mut caller: Caller<'_, ()>, value: i32| -> i32 {
                let string_value = value.to_string();

                // Get memory to store the string
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let mut data = memory.data_mut(&mut caller);

                        // Simple string storage: length (4 bytes) + string data
                        let string_bytes = string_value.as_bytes();
                        let total_size = 4 + string_bytes.len();

                        // Find a place to store the string (simple allocation at end of used memory)
                        let mut offset = 1024; // Start after initial memory
                        while offset + total_size < data.len() {
                            // Check if this area is free (all zeros)
                            let is_free = data[offset..offset + total_size].iter().all(|&b| b == 0);
                            if is_free {
                                break;
                            }
                            offset += 32; // Move in 32-byte chunks
                        }

                        if offset + total_size < data.len() {
                            // Store length
                            data[offset..offset + 4]
                                .copy_from_slice(&(string_bytes.len() as u32).to_le_bytes());
                            // Store string data
                            data[offset + 4..offset + 4 + string_bytes.len()]
                                .copy_from_slice(string_bytes);
                            return offset as i32;
                        }
                    }
                }

                0 // Return null pointer on failure
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create int_to_string function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "float_to_string",
            |mut caller: Caller<'_, ()>, value: f64| -> i32 {
                let string_value = value.to_string();

                // Get memory to store the string
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let mut data = memory.data_mut(&mut caller);

                        // Simple string storage: length (4 bytes) + string data
                        let string_bytes = string_value.as_bytes();
                        let total_size = 4 + string_bytes.len();

                        // Find a place to store the string
                        let mut offset = 1024;
                        while offset + total_size < data.len() {
                            let is_free = data[offset..offset + total_size].iter().all(|&b| b == 0);
                            if is_free {
                                break;
                            }
                            offset += 32;
                        }

                        if offset + total_size < data.len() {
                            // Store length
                            data[offset..offset + 4]
                                .copy_from_slice(&(string_bytes.len() as u32).to_le_bytes());
                            // Store string data
                            data[offset + 4..offset + 4 + string_bytes.len()]
                                .copy_from_slice(string_bytes);
                            return offset as i32;
                        }
                    }
                }

                0 // Return null pointer on failure
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create float_to_string function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "bool_to_string",
            |mut caller: Caller<'_, ()>, value: i32| -> i32 {
                let string_value = if value != 0 { "true" } else { "false" };

                // Get memory to store the string
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let mut data = memory.data_mut(&mut caller);

                        let string_bytes = string_value.as_bytes();
                        let total_size = 4 + string_bytes.len();

                        let mut offset = 1024;
                        while offset + total_size < data.len() {
                            let is_free = data[offset..offset + total_size].iter().all(|&b| b == 0);
                            if is_free {
                                break;
                            }
                            offset += 32;
                        }

                        if offset + total_size < data.len() {
                            data[offset..offset + 4]
                                .copy_from_slice(&(string_bytes.len() as u32).to_le_bytes());
                            data[offset + 4..offset + 4 + string_bytes.len()]
                                .copy_from_slice(string_bytes);
                            return offset as i32;
                        }
                    }
                }

                0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create bool_to_string function: {e}"),
                None,
                None,
            )
        })?;

    // String parsing functions
    linker
        .func_wrap(
            "env",
            "string_to_int",
            |mut caller: Caller<'_, ()>, str_ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        if str_ptr >= 0 && (str_ptr as usize) + 4 < data.len() {
                            // Read string length
                            let len_bytes = &data[str_ptr as usize..str_ptr as usize + 4];
                            let str_len = u32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]) as usize;

                            if str_ptr as usize + 4 + str_len < data.len() {
                                // Read string data
                                let str_data =
                                    &data[str_ptr as usize + 4..str_ptr as usize + 4 + str_len];
                                if let Ok(string_value) = std::str::from_utf8(str_data) {
                                    return string_value.parse::<i32>().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
                0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_to_int function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "string_to_float",
            |mut caller: Caller<'_, ()>, str_ptr: i32| -> f64 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        if str_ptr >= 0 && (str_ptr as usize) + 4 < data.len() {
                            let len_bytes = &data[str_ptr as usize..str_ptr as usize + 4];
                            let str_len = u32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]) as usize;

                            if str_ptr as usize + 4 + str_len < data.len() {
                                let str_data =
                                    &data[str_ptr as usize + 4..str_ptr as usize + 4 + str_len];
                                if let Ok(string_value) = std::str::from_utf8(str_data) {
                                    return string_value.parse::<f64>().unwrap_or(0.0);
                                }
                            }
                        }
                    }
                }
                0.0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_to_float function: {e}"),
                None,
                None,
            )
        })?;

    // String concatenation function - enhanced to handle both string literal and length-prefixed formats
    linker
        .func_wrap(
            "env",
            "string_concat",
            |mut caller: Caller<'_, ()>, str1_ptr: i32, str2_ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let mut data = memory.data_mut(&mut caller);

                        // Helper function to read a string that might be in either format
                        let read_string = |ptr: i32, data: &[u8]| -> String {
                            if ptr < 0 || (ptr as usize) >= data.len() {
                                return String::new();
                            }

                            let start = ptr as usize;

                            // Try to read as length-prefixed string first (from toString())
                            if start + 4 < data.len() {
                                let len_bytes = &data[start..start + 4];
                                let potential_len = u32::from_le_bytes([
                                    len_bytes[0],
                                    len_bytes[1],
                                    len_bytes[2],
                                    len_bytes[3],
                                ]) as usize;

                                // Check if this looks like a valid length-prefixed string
                                if potential_len > 0
                                    && potential_len < 1000
                                    && start + 4 + potential_len <= data.len()
                                {
                                    let str_data = &data[start + 4..start + 4 + potential_len];
                                    if let Ok(s) = std::str::from_utf8(str_data) {
                                        // Check if it's all printable ASCII/UTF-8 - good sign it's a real string
                                        if s.chars().all(|c| !c.is_control() || c.is_whitespace()) {
                                            return s.to_string();
                                        }
                                    }
                                }
                            }

                            // Fall back to reading as null-terminated string literal
                            let mut end = start;
                            while end < data.len() && data[end] != 0 {
                                end += 1;
                            }

                            if let Ok(s) = std::str::from_utf8(&data[start..end]) {
                                s.to_string()
                            } else {
                                String::new()
                            }
                        };

                        // Read both strings using the enhanced reader
                        let str1 = read_string(str1_ptr, &data);
                        let str2 = read_string(str2_ptr, &data);

                        // Concatenate strings
                        let result = format!("{str1}{str2}");
                        let result_bytes = result.as_bytes();

                        // For now, use a simple approach: find space in existing memory
                        // This is a placeholder - proper memory management would be more complex
                        let result_len = result_bytes.len() as u32;
                        let total_size = 4 + result_len as usize; // 4 bytes for length + string content

                        // Look for free space in memory (starting from offset 1024)
                        let mut allocation_ptr = 1024;
                        while allocation_ptr + total_size < data.len() {
                            // Check if this space is free (first 4 bytes are 0)
                            let check_bytes = &data[allocation_ptr..allocation_ptr + 4];
                            if check_bytes == [0, 0, 0, 0] {
                                // Found free space, write the string here
                                // Write length
                                let len_bytes = result_len.to_le_bytes();
                                data[allocation_ptr..allocation_ptr + 4]
                                    .copy_from_slice(&len_bytes);

                                // Write string content
                                data[allocation_ptr + 4..allocation_ptr + 4 + result_bytes.len()]
                                    .copy_from_slice(result_bytes);

                                return allocation_ptr as i32;
                            }
                            allocation_ptr += 16; // Check next 16-byte aligned position
                        }

                        // If no free space found, return 0 (allocation failed)
                        return 0;
                    }
                }
                0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_concat function: {e}"),
                None,
                None,
            )
        })?;

    // String comparison function
    linker
        .func_wrap(
            "env",
            "string_compare",
            |mut caller: Caller<'_, ()>, str1_ptr: i32, str2_ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        // Read first string
                        let str1 = if str1_ptr >= 0 && (str1_ptr as usize) + 4 < data.len() {
                            let len_bytes = &data[str1_ptr as usize..str1_ptr as usize + 4];
                            let str1_len = u32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]) as usize;

                            if str1_ptr as usize + 4 + str1_len < data.len() {
                                let str1_data =
                                    &data[str1_ptr as usize + 4..str1_ptr as usize + 4 + str1_len];
                                std::str::from_utf8(str1_data).unwrap_or("")
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };

                        // Read second string
                        let str2 = if str2_ptr >= 0 && (str2_ptr as usize) + 4 < data.len() {
                            let len_bytes = &data[str2_ptr as usize..str2_ptr as usize + 4];
                            let str2_len = u32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]) as usize;

                            if str2_ptr as usize + 4 + str2_len < data.len() {
                                let str2_data =
                                    &data[str2_ptr as usize + 4..str2_ptr as usize + 4 + str2_len];
                                std::str::from_utf8(str2_data).unwrap_or("")
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };

                        // Compare strings and return result
                        return match str1.cmp(str2) {
                            std::cmp::Ordering::Less => -1,
                            std::cmp::Ordering::Equal => 0,
                            std::cmp::Ordering::Greater => 1,
                        };
                    }
                }
                0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_compare function: {e}"),
                None,
                None,
            )
        })?;

    // Add memory management functions
    linker
        .func_wrap(
            "env",
            "memory_allocate",
            |mut caller: Caller<'_, ()>, size: i32, type_id: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let mut data = memory.data_mut(&mut caller);

                        // Simple memory allocation strategy
                        // Find a free block of the requested size
                        let aligned_size = ((size + 15) & !15) as usize; // 16-byte alignment
                        let header_size = 16; // Header with size, type_id, ref_count, and flags
                        let total_size = header_size + aligned_size;

                        // Search for a free block starting from offset 2048 (after string pool)
                        let mut offset = 2048;
                        while offset + total_size < data.len() {
                            // Check if this area is free (first 4 bytes are 0)
                            let size_bytes = &data[offset..offset + 4];
                            let existing_size = u32::from_le_bytes([
                                size_bytes[0],
                                size_bytes[1],
                                size_bytes[2],
                                size_bytes[3],
                            ]);

                            if existing_size == 0 {
                                // Found free space, allocate here
                                // Write header: [size(4), type_id(4), ref_count(4), flags(4)]
                                data[offset..offset + 4]
                                    .copy_from_slice(&(aligned_size as u32).to_le_bytes());
                                data[offset + 4..offset + 8]
                                    .copy_from_slice(&(type_id as u32).to_le_bytes());
                                data[offset + 8..offset + 12].copy_from_slice(&1u32.to_le_bytes()); // ref_count = 1
                                data[offset + 12..offset + 16].copy_from_slice(&0u32.to_le_bytes()); // flags = 0

                                println!(
                                    "🧠 [MEMORY] Allocated {} bytes at offset {} (type {})",
                                    aligned_size,
                                    offset + header_size,
                                    type_id
                                );
                                return (offset + header_size) as i32; // Return pointer to data (after header)
                            } else {
                                // Skip this allocated block
                                offset += header_size + existing_size as usize;
                                // Align to 16-byte boundary
                                offset = (offset + 15) & !15;
                            }
                        }

                        println!(
                            "❌ [MEMORY] Failed to allocate {} bytes - out of memory",
                            aligned_size
                        );
                        return 0; // Out of memory
                    }
                }

                0 // Failed to get memory
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create memory_allocate function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "memory_retain",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                if ptr <= 0 {
                    return 0; // Invalid pointer
                }

                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let mut data = memory.data_mut(&mut caller);
                        let header_offset = (ptr as usize) - 16; // Go back to header

                        if header_offset + 16 < data.len() {
                            // Read current ref_count
                            let ref_count_bytes = &data[header_offset + 8..header_offset + 12];
                            let mut ref_count = u32::from_le_bytes([
                                ref_count_bytes[0],
                                ref_count_bytes[1],
                                ref_count_bytes[2],
                                ref_count_bytes[3],
                            ]);

                            // Increment ref_count
                            ref_count += 1;
                            data[header_offset + 8..header_offset + 12]
                                .copy_from_slice(&ref_count.to_le_bytes());

                            println!(
                                "🔒 [MEMORY] Retained pointer {} (ref_count: {})",
                                ptr, ref_count
                            );
                            return ref_count as i32;
                        }
                    }
                }

                0 // Failed
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create memory_retain function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "memory_release",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                if ptr <= 0 {
                    return 0; // Invalid pointer
                }

                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let mut data = memory.data_mut(&mut caller);
                        let header_offset = (ptr as usize) - 16; // Go back to header

                        if header_offset + 16 < data.len() {
                            // Read current ref_count
                            let ref_count_bytes = &data[header_offset + 8..header_offset + 12];
                            let mut ref_count = u32::from_le_bytes([
                                ref_count_bytes[0],
                                ref_count_bytes[1],
                                ref_count_bytes[2],
                                ref_count_bytes[3],
                            ]);

                            if ref_count > 0 {
                                // Decrement ref_count
                                ref_count -= 1;
                                data[header_offset + 8..header_offset + 12]
                                    .copy_from_slice(&ref_count.to_le_bytes());

                                if ref_count == 0 {
                                    // Free the memory by zeroing the size field
                                    data[header_offset..header_offset + 4]
                                        .copy_from_slice(&0u32.to_le_bytes());
                                    println!(
                                        "🗑️ [MEMORY] Freed pointer {} (ref_count reached 0)",
                                        ptr
                                    );
                                } else {
                                    println!(
                                        "🔓 [MEMORY] Released pointer {} (ref_count: {})",
                                        ptr, ref_count
                                    );
                                }

                                return ref_count as i32;
                            }
                        }
                    }
                }

                0 // Failed
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create memory_release function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "memory_collect_garbage",
            |mut caller: Caller<'_, ()>| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let mut data = memory.data_mut(&mut caller);
                        let mut freed_blocks = 0;

                        // Simple garbage collection: scan for blocks with ref_count = 0 and free them
                        let mut offset = 2048;
                        while offset + 16 < data.len() {
                            let size_bytes = &data[offset..offset + 4];
                            let size = u32::from_le_bytes([
                                size_bytes[0],
                                size_bytes[1],
                                size_bytes[2],
                                size_bytes[3],
                            ]);

                            if size > 0 {
                                // Check ref_count
                                let ref_count_bytes = &data[offset + 8..offset + 12];
                                let ref_count = u32::from_le_bytes([
                                    ref_count_bytes[0],
                                    ref_count_bytes[1],
                                    ref_count_bytes[2],
                                    ref_count_bytes[3],
                                ]);

                                if ref_count == 0 {
                                    // Free this block
                                    data[offset..offset + 4].copy_from_slice(&0u32.to_le_bytes());
                                    freed_blocks += 1;
                                }

                                offset += 16 + size as usize;
                                offset = (offset + 15) & !15; // Align to 16 bytes
                            } else {
                                offset += 16; // Skip free block header
                            }
                        }

                        println!(
                            "🧹 [MEMORY] Garbage collection freed {} blocks",
                            freed_blocks
                        );
                        return freed_blocks;
                    }
                }

                0 // Failed
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create memory_collect_garbage function: {e}"),
                None,
                None,
            )
        })?;

    // Add file functions with real filesystem operations
    linker
        .func_wrap(
            "env",
            "file_write",
            |mut caller: Caller<'_, ()>,
             path_ptr: i32,
             path_len: i32,
             content_ptr: i32,
             content_len: i32|
             -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        if path_ptr >= 0 && path_len >= 0 && content_ptr >= 0 && content_len >= 0 {
                            let path_start = path_ptr as usize;
                            let path_length = path_len as usize;
                            let content_start = content_ptr as usize;
                            let content_length = content_len as usize;

                            if path_start + path_length <= data.len()
                                && content_start + content_length <= data.len()
                            {
                                if let (Ok(path), Ok(content)) = (
                                    std::str::from_utf8(
                                        &data[path_start..path_start + path_length],
                                    ),
                                    std::str::from_utf8(
                                        &data[content_start..content_start + content_length],
                                    ),
                                ) {
                                    // Make real file write
                                    match fs::write(path, content) {
                                        Ok(()) => {
                                            println!(
                                                "✅ [FILE WRITE] Successfully wrote to {}",
                                                path
                                            );
                                            return 0; // Success
                                        }
                                        Err(e) => {
                                            println!(
                                                "❌ [FILE WRITE] Failed to write {}: {}",
                                                path, e
                                            );
                                            return -1; // Error
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                println!("❌ [FILE WRITE] Invalid parameters");
                -1 // Error indicator
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_write function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "file_read",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32, _result_ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        if path_ptr >= 0 && path_len >= 0 {
                            let start = path_ptr as usize;
                            let len = path_len as usize;

                            if start + len <= data.len() {
                                if let Ok(path) = std::str::from_utf8(&data[start..start + len]) {
                                    // Make real file read
                                    match fs::read_to_string(path) {
                                        Ok(content) => {
                                            println!(
                                                "✅ [FILE READ] Successfully read {} bytes from {}",
                                                content.len(),
                                                path
                                            );
                                            // Store content in memory and return pointer
                                            let mut data = memory.data_mut(&mut caller);
                                            let content_bytes = content.as_bytes();
                                            let total_size = 4 + content_bytes.len();

                                            // Find a place to store the content
                                            let mut offset = 1024;
                                            while offset + total_size < data.len() {
                                                let is_free = data[offset..offset + total_size]
                                                    .iter()
                                                    .all(|&b| b == 0);
                                                if is_free {
                                                    break;
                                                }
                                                offset += 32;
                                            }

                                            if offset + total_size < data.len() {
                                                // Store length
                                                data[offset..offset + 4].copy_from_slice(
                                                    &(content_bytes.len() as u32).to_le_bytes(),
                                                );
                                                // Store content
                                                data[offset + 4..offset + 4 + content_bytes.len()]
                                                    .copy_from_slice(content_bytes);
                                                return offset as i32;
                                            }
                                        }
                                        Err(e) => {
                                            println!(
                                                "❌ [FILE READ] Failed to read {}: {}",
                                                path, e
                                            );
                                            return -1; // Error
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                println!("❌ [FILE READ] Invalid path parameters");
                -1 // Error indicator
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_read function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "file_exists",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        if path_ptr >= 0 && path_len >= 0 {
                            let start = path_ptr as usize;
                            let len = path_len as usize;

                            if start + len <= data.len() {
                                if let Ok(path) = std::str::from_utf8(&data[start..start + len]) {
                                    // Check if file exists
                                    let exists = Path::new(path).exists();
                                    println!("📁 [FILE EXISTS] File '{path}' exists: {exists}");
                                    return if exists { 1 } else { 0 };
                                }
                            }
                        }
                    }
                }

                println!("❌ [FILE EXISTS] Invalid path parameters");
                0 // File doesn't exist or error
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_exists function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "file_delete",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        if path_ptr >= 0 && path_len >= 0 {
                            let start = path_ptr as usize;
                            let len = path_len as usize;

                            if start + len <= data.len() {
                                if let Ok(path) = std::str::from_utf8(&data[start..start + len]) {
                                    // Delete file
                                    match fs::remove_file(path) {
                                        Ok(()) => {
                                            println!(
                                                "✅ [FILE DELETE] Successfully deleted {}",
                                                path
                                            );
                                            return 0; // Success
                                        }
                                        Err(e) => {
                                            println!(
                                                "❌ [FILE DELETE] Failed to delete {}: {}",
                                                path, e
                                            );
                                            return -1; // Error
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                println!("❌ [FILE DELETE] Invalid path parameters");
                -1 // Error indicator
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_delete function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "file_append",
            |mut caller: Caller<'_, ()>,
             path_ptr: i32,
             path_len: i32,
             content_ptr: i32,
             content_len: i32|
             -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        if path_ptr >= 0 && path_len >= 0 && content_ptr >= 0 && content_len >= 0 {
                            let path_start = path_ptr as usize;
                            let path_length = path_len as usize;
                            let content_start = content_ptr as usize;
                            let content_length = content_len as usize;

                            if path_start + path_length <= data.len()
                                && content_start + content_length <= data.len()
                            {
                                if let (Ok(path), Ok(content)) = (
                                    std::str::from_utf8(
                                        &data[path_start..path_start + path_length],
                                    ),
                                    std::str::from_utf8(
                                        &data[content_start..content_start + content_length],
                                    ),
                                ) {
                                    // Append to file
                                    use std::fs::OpenOptions;
                                    use std::io::Write;
                                    match OpenOptions::new()
                                        .create(true)
                                        .append(true)
                                        .open(path)
                                        .and_then(|mut file| file.write_all(content.as_bytes()))
                                    {
                                        Ok(()) => {
                                            println!(
                                                "✅ [FILE APPEND] Successfully appended to {}",
                                                path
                                            );
                                            return 0; // Success
                                        }
                                        Err(e) => {
                                            println!(
                                                "❌ [FILE APPEND] Failed to append to {}: {}",
                                                path, e
                                            );
                                            return -1; // Error
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                println!("❌ [FILE APPEND] Invalid parameters");
                -1 // Error indicator
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_append function: {e}"),
                None,
                None,
            )
        })?;

    // Note: Async runtime functions are now handled by the integrated CleanRuntime
    // These are kept as simple stubs for WASM compatibility, but the real async
    // functionality is provided by the async runtime integration above
    linker
        .func_wrap(
            "env",
            "create_future",
            |mut caller: Caller<'_, ()>, future_name_ptr: i32, future_name_len: i32| -> i32 {
                // Extract future name from WASM memory
                let future_name = if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if future_name_ptr >= 0 && future_name_len >= 0 {
                            let start = future_name_ptr as usize;
                            let len = future_name_len as usize;
                            if start + len <= data.len() {
                                std::str::from_utf8(&data[start..start + len])
                                    .unwrap_or("unnamed_future")
                                    .to_string()
                            } else {
                                "unnamed_future".to_string()
                            }
                        } else {
                            "unnamed_future".to_string()
                        }
                    } else {
                        "unnamed_future".to_string()
                    }
                } else {
                    "unnamed_future".to_string()
                };

                println!("🔮 [ASYNC] Creating future: {future_name}");
                1 // Return future handle ID
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create create_future function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "start_background_task",
            |mut caller: Caller<'_, ()>, task_name_ptr: i32, task_name_len: i32| -> i32 {
                // Extract task name from WASM memory
                let task_name = if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if task_name_ptr >= 0 && task_name_len >= 0 {
                            let start = task_name_ptr as usize;
                            let len = task_name_len as usize;
                            if start + len <= data.len() {
                                std::str::from_utf8(&data[start..start + len])
                                    .unwrap_or("unnamed_task")
                                    .to_string()
                            } else {
                                "unnamed_task".to_string()
                            }
                        } else {
                            "unnamed_task".to_string()
                        }
                    } else {
                        "unnamed_task".to_string()
                    }
                } else {
                    "unnamed_task".to_string()
                };

                println!("🔄 [ASYNC] Starting background task: {task_name}");
                // Return a task ID (in real implementation, this would be managed by the async runtime)
                (rand::random::<u32>() % 1000 + 1) as i32
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create start_background_task function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "execute_background",
            |mut caller: Caller<'_, ()>, operation_ptr: i32, operation_len: i32| -> i32 {
                // Extract operation from WASM memory
                let operation = if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if operation_ptr >= 0 && operation_len >= 0 {
                            let start = operation_ptr as usize;
                            let len = operation_len as usize;
                            if start + len <= data.len() {
                                std::str::from_utf8(&data[start..start + len])
                                    .unwrap_or("unnamed_operation")
                                    .to_string()
                            } else {
                                "unnamed_operation".to_string()
                            }
                        } else {
                            "unnamed_operation".to_string()
                        }
                    } else {
                        "unnamed_operation".to_string()
                    }
                } else {
                    "unnamed_operation".to_string()
                };

                println!("🔄 [ASYNC] Executing background operation: {operation}");
                1 // Return success
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create execute_background function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "resolve_future",
            |_caller: Caller<'_, ()>, future_id: i32, value: i32| -> i32 {
                println!(
                    "✅ [ASYNC] Resolved future #{} with value: {}",
                    future_id, value
                );
                1 // Return success
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create resolve_future function: {e}"),
                None,
                None,
            )
        })?;

    // Add mathematical functions
    linker
        .func_wrap(
            "env",
            "pow",
            |_caller: Caller<'_, ()>, base: f64, exponent: f64| -> f64 { base.powf(exponent) },
        )
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create pow function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "sin", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.sin()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create sin function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "cos", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.cos()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create cos function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "tan", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.tan()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create tan function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "ln", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.ln()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create ln function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "log10", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.log10()
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create log10 function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap("env", "log2", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.log2()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create log2 function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "exp", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.exp()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create exp function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "exp2", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.exp2()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create exp2 function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "sqrt", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.sqrt()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create sqrt function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "sinh", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.sinh()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create sinh function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "cosh", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.cosh()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create cosh function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "tanh", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.tanh()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create tanh function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "asin", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.asin()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create asin function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "acos", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.acos()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create acos function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "atan", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.atan()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create atan function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "pi", |_caller: Caller<'_, ()>| -> f64 {
            std::f64::consts::PI
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create pi function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "e", |_caller: Caller<'_, ()>| -> f64 {
            std::f64::consts::E
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create e function: {e}"), None, None)
        })?;

    linker
        .func_wrap("env", "abs", |_caller: Caller<'_, ()>, x: f64| -> f64 {
            x.abs()
        })
        .map_err(|e| {
            CompilerError::runtime_error(format!("Failed to create abs function: {e}"), None, None)
        })?;

    // Console input functions
    linker
        .func_wrap(
            "env",
            "input",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
                // Extract prompt from memory
                let prompt = if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if prompt_ptr >= 0 && prompt_len >= 0 {
                            let start = prompt_ptr as usize;
                            let len = prompt_len as usize;
                            if start + len <= data.len() {
                                std::str::from_utf8(&data[start..start + len]).unwrap_or("")
                            } else {
                                ""
                            }
                        } else {
                            ""
                        }
                    } else {
                        ""
                    }
                } else {
                    ""
                };

                // Display prompt and get user input
                print!("{}", prompt);
                use std::io::{self, Write};
                io::stdout().flush().unwrap();

                let mut input = String::new();
                match io::stdin().read_line(&mut input) {
                    Ok(_) => {
                        input = input.trim().to_string();

                        // Store the input string in memory
                        if let Some(memory) = caller.get_export("memory") {
                            if let Some(memory) = memory.into_memory() {
                                let mut data = memory.data_mut(&mut caller);

                                let string_bytes = input.as_bytes();
                                let total_size = 4 + string_bytes.len();

                                // Find a place to store the string
                                let mut offset = 1024;
                                while offset + total_size < data.len() {
                                    let is_free =
                                        data[offset..offset + total_size].iter().all(|&b| b == 0);
                                    if is_free {
                                        break;
                                    }
                                    offset += 32;
                                }

                                if offset + total_size < data.len() {
                                    // Store length
                                    data[offset..offset + 4].copy_from_slice(
                                        &(string_bytes.len() as u32).to_le_bytes(),
                                    );
                                    // Store string data
                                    data[offset + 4..offset + 4 + string_bytes.len()]
                                        .copy_from_slice(string_bytes);
                                    return offset as i32;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("❌ [INPUT] Error reading input: {e}");
                    }
                }

                0 // Return null pointer on failure
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create input function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "input_integer",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
                // Extract prompt from memory
                let prompt = if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if prompt_ptr >= 0 && prompt_len >= 0 {
                            let start = prompt_ptr as usize;
                            let len = prompt_len as usize;
                            if start + len <= data.len() {
                                std::str::from_utf8(&data[start..start + len]).unwrap_or("")
                            } else {
                                ""
                            }
                        } else {
                            ""
                        }
                    } else {
                        ""
                    }
                } else {
                    ""
                };

                // Get integer input with validation and retry
                use std::io::{self, Write};
                loop {
                    print!("{}", prompt);
                    io::stdout().flush().unwrap();

                    let mut input = String::new();
                    match io::stdin().read_line(&mut input) {
                        Ok(_) => {
                            let input = input.trim();
                            match input.parse::<i32>() {
                                Ok(value) => return value,
                                Err(_) => {
                                    println!("❌ Invalid integer. Please enter a valid number.");
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            println!("❌ [INPUT INTEGER] Error reading input: {e}");
                            return 0;
                        }
                    }
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create input_integer function: {e}"),
                None,
                None,
            )
        })?;

    linker
        .func_wrap(
            "env",
            "input_float",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> f64 {
                // Extract prompt from memory
                let prompt = if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if prompt_ptr >= 0 && prompt_len >= 0 {
                            let start = prompt_ptr as usize;
                            let len = prompt_len as usize;
                            if start + len <= data.len() {
                                std::str::from_utf8(&data[start..start + len]).unwrap_or("")
                            } else {
                                ""
                            }
                        } else {
                            ""
                        }
                    } else {
                        ""
                    }
                } else {
                    ""
                };

                // Get float input with validation and retry
                use std::io::{self, Write};
                loop {
                    print!("{}", prompt);
                    io::stdout().flush().unwrap();

                    let mut input = String::new();
                    match io::stdin().read_line(&mut input) {
                        Ok(_) => {
                            let input = input.trim();
                            match input.parse::<f64>() {
                                Ok(value) => return value,
                                Err(_) => {
                                    println!(
                                        "❌ Invalid number. Please enter a valid decimal number."
                                    );
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            println!("❌ [INPUT FLOAT] Error reading input: {e}");
                            return 0.0;
                        }
                    }
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create input_float function: {e}"),
                None,
                None,
            )
        })?;

    linker.func_wrap("env", "input_yesno", |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
        // Extract prompt from memory
        let prompt = if let Some(memory) = caller.get_export("memory") {
            if let Some(memory) = memory.into_memory() {
                let data = memory.data(&caller);
                if prompt_ptr >= 0 && prompt_len >= 0 {
                    let start = prompt_ptr as usize;
                    let len = prompt_len as usize;
                    if start + len <= data.len() {
                        std::str::from_utf8(&data[start..start + len]).unwrap_or("")
                    } else {
                        ""
                    }
                } else {
                    ""
                }
            } else {
                ""
            }
        } else {
            ""
        };

        // Get yes/no input with validation and retry
        use std::io::{self, Write};
        loop {
            print!("{}", prompt);
            io::stdout().flush().unwrap();

            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    let input = input.trim().to_lowercase();
                    match input.as_str() {
                        "yes" | "y" | "true" | "1" => return 1,
                        "no" | "n" | "false" | "0" => return 0,
                        _ => {
                            println!("❌ Please enter 'yes'/'no', 'y'/'n', 'true'/'false', or '1'/'0'.");
                            continue;
                        }
                    }
                }
                Err(e) => {
                    println!("❌ [INPUT YES/NO] Error reading input: {e}");
                    return 0;
                }
            }
        }
    })
    .map_err(|e| CompilerError::runtime_error(
        format!("Failed to create input_yesno function: {e}"),
        None, None
    ))?;

    // Instantiate the module
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
