#![allow(clippy::uninlined_format_args)]

use std::env;
use std::fs;
use wasmparser::{Parser, Payload};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: debug_wasm <file.wasm>");
        return Ok(());
    }

    let wasm_bytes = fs::read(&args[1])?;
    println!("WASM file size: {len} bytes", len = wasm_bytes.len());

    // Try to parse the WASM file
    let parser = Parser::new(0);
    let mut function_count = 0;
    let mut type_count = 0;
    let mut start_function_index = None;

    for payload in parser.parse_all(&wasm_bytes) {
        match payload? {
            Payload::TypeSection(reader) => {
                println!("📝 Type Section:");
                for (i, ty) in reader.into_iter().enumerate() {
                    let rec_group = ty?;
                    println!("  Type {i}: {rec_group:?}");
                    type_count += 1;
                }
            }
            Payload::FunctionSection(reader) => {
                println!("🔧 Function Section:");
                for (i, type_index) in reader.into_iter().enumerate() {
                    let type_idx = type_index?;
                    println!("  Function {i}: uses type {type_idx}");
                    function_count += 1;
                }
            }
            Payload::ExportSection(reader) => {
                println!("📤 Export Section:");
                for export in reader.into_iter() {
                    let exp = export?;
                    println!(
                        "  Export '{}': {:?} index {}",
                        exp.name, exp.kind, exp.index
                    );
                    if exp.name == "start" {
                        start_function_index = Some(exp.index);
                    }
                }
            }
            Payload::CodeSectionStart { count, .. } => {
                println!("💻 Code Section: {count} functions");
            }
            Payload::CodeSectionEntry(body) => {
                println!("  Function body: {len} bytes", len = body.range().len());

                // Get function locals
                let locals_reader = body.get_locals_reader()?;
                let locals: Vec<_> = locals_reader.into_iter().collect::<Result<Vec<_>, _>>()?;
                println!("    Locals: {:?}", locals);

                // Get operators
                let ops_reader = body.get_operators_reader()?;
                let mut ops = Vec::new();
                for op_result in ops_reader {
                    let op = op_result?;
                    ops.push(format!("{op:?}"));
                    if ops.len() > 10 {
                        ops.push("...".to_string());
                        break;
                    }
                }
                println!("    Operations: {ops}", ops = ops.join(", "));
            }
            Payload::MemorySection(_) => {
                println!("💾 Memory Section found");
            }
            Payload::DataSection(_) => {
                println!("📊 Data Section found");
            }
            _ => {}
        }
    }

    println!("\n🎯 Analysis:");
    println!("Total types: {type_count}");
    println!("Total functions: {function_count}");
    if let Some(start_idx) = start_function_index {
        println!("Start function exported at index: {start_idx}");
    }

    // Validate the entire WASM module
    println!("\n🔍 WASM Validation:");
    match wasmparser::validate(&wasm_bytes) {
        Ok(_) => println!("✅ WASM validation passed!"),
        Err(e) => {
            println!("❌ WASM validation failed: {e}");
            println!("Error details: {:?}", e);
        }
    }

    // Show first 200 bytes in hex for debugging
    println!("\nFirst 200 bytes (hex):");
    for (i, chunk) in wasm_bytes.chunks(16).take(12).enumerate() {
        print!("{:08x}: ", i * 16);
        for byte in chunk {
            print!("{:02x} ", byte);
        }
        println!();
    }

    Ok(())
}
