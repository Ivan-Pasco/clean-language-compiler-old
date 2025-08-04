use std::env;
use std::fs;
use wasmparser::{Parser, Payload};

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

    println!("Converting {input_file} to {output_file}...");

    let wasm_content = fs::read(input_file)?;
    let wat_content = convert_wasm_to_wat(&wasm_content)?;
    fs::write(output_file, wat_content)?;

    println!("Conversion successful!");
    Ok(())
}

fn convert_wasm_to_wat(wasm_bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let parser = Parser::new(0);
    let mut output = String::new();
    output.push_str("(module\n");

    for payload in parser.parse_all(wasm_bytes) {
        match payload? {
            Payload::Version { num, .. } => {
                output.push_str(&format!("  ;; WebAssembly version: {num}\n"));
            }
            Payload::TypeSection(reader) => {
                output.push_str("  ;; Type section\n");
                for _item in reader {
                    output.push_str("  ;; Type definition\n");
                }
            }
            Payload::ImportSection(reader) => {
                output.push_str("  ;; Import section\n");
                for item in reader {
                    let import = item?;
                    output.push_str(&format!(
                        "  (import \"{}\" \"{}\" ...)\n",
                        import.module, import.name
                    ));
                }
            }
            Payload::FunctionSection(reader) => {
                output.push_str("  ;; Function section\n");
                for item in reader {
                    let type_idx = item?;
                    output.push_str(&format!("  (func (type {type_idx}))\n"));
                }
            }
            Payload::ExportSection(reader) => {
                output.push_str("  ;; Export section\n");
                for item in reader {
                    let export = item?;
                    let kind_str = match export.kind {
                        wasmparser::ExternalKind::Func => "func",
                        wasmparser::ExternalKind::Table => "table",
                        wasmparser::ExternalKind::Memory => "memory",
                        wasmparser::ExternalKind::Global => "global",
                        _ => "unknown",
                    };
                    output.push_str(&format!(
                        "  (export \"{}\" ({} {}))\n",
                        export.name, kind_str, export.index
                    ));
                }
            }
            Payload::CodeSectionStart { .. } => {
                output.push_str("  ;; Code section start\n");
            }
            Payload::CodeSectionEntry(_) => {
                output.push_str("  (func\n");
                output.push_str("    ;; Function body\n");
                output.push_str("  )\n");
            }
            Payload::MemorySection(reader) => {
                output.push_str("  ;; Memory section\n");
                for item in reader {
                    let memory = item?;
                    output.push_str(&format!("  (memory {initial})\n", initial = memory.initial));
                }
            }
            _ => {
                // Handle other section types if needed
            }
        }
    }

    output.push_str(")\n");
    Ok(output)
}
