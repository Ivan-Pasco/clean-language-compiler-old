//! Assemble WebAssembly text (WAT) into a WebAssembly binary.
//!
//! Usage: `wat2wasm <input.wat> <output.wasm>`. Errors go to stderr and the
//! tool exits with status 1.

use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        eprintln!(
            "Usage: {program} <input.wat> <output.wasm>",
            program = args[0]
        );
        std::process::exit(1);
    }

    let input_file = &args[1];
    let output_file = &args[2];

    let wat_content = fs::read_to_string(input_file)
        .map_err(|e| format!("Error reading file {input_file}: {e}"))?;
    let wasm_binary =
        wat::parse_str(&wat_content).map_err(|e| format!("Error parsing WAT: {e}"))?;
    fs::write(output_file, wasm_binary)
        .map_err(|e| format!("Error writing file {output_file}: {e}"))?;

    println!("Converted {input_file} to {output_file}");
    Ok(())
}
