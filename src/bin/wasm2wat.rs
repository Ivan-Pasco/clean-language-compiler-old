//! Print a WebAssembly binary as WebAssembly text (WAT).
//!
//! Usage: `wasm2wat <input.wasm> <output.wat>`. The text is the standard
//! format produced by `wasmprinter` (the printer behind `wasm-tools print`),
//! so it parses back with `wat2wasm`.

use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        eprintln!(
            "Usage: {program} <input.wasm> <output.wat>",
            program = args[0]
        );
        std::process::exit(1);
    }

    let input_file = &args[1];
    let output_file = &args[2];

    let wasm_content = fs::read(input_file)?;
    let wat_content = wasmprinter::print_bytes(&wasm_content)?;
    fs::write(output_file, wat_content)?;

    println!("Converted {input_file} to {output_file}");
    Ok(())
}
