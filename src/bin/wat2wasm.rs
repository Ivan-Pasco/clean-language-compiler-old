use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        println!("Usage: wat2wasm <input.wat> <output.wasm>");
        return;
    }

    let input_file = &args[1];
    let output_file = &args[2];

    println!("Converting {input_file} to {output_file}...");

    // Read the WAT file
    let wat_content = match fs::read_to_string(input_file) {
        Ok(content) => content,
        Err(e) => {
            println!("Error reading file {input_file}: {e}");
            return;
        }
    };

    // Convert WAT to WASM using the wat crate
    let wasm_binary = match wat::parse_str(&wat_content) {
        Ok(wasm) => wasm,
        Err(e) => {
            println!("Error parsing WAT: {e}");
            return;
        }
    };

    // Write the WASM file
    if let Err(e) = fs::write(output_file, wasm_binary) {
        println!("Error writing file {output_file}: {e}");
        return;
    }

    println!("Conversion successful!");
}
