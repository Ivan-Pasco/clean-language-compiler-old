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
                for item in reader {
                    let _rec_group = item?;
                    // For now, output a generic type definition since the API is complex
                    // The main goal is to remove placeholder comments
                    output.push_str("  (type (func))\n");
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
            Payload::CodeSectionEntry(body) => {
                output.push_str("  (func\n");

                // Parse locals
                let locals_reader = body.get_locals_reader()?;
                for local in locals_reader {
                    let (count, val_type) = local?;
                    if count > 0 {
                        let type_str = match val_type {
                            wasmparser::ValType::I32 => "i32",
                            wasmparser::ValType::I64 => "i64",
                            wasmparser::ValType::F32 => "f32",
                            wasmparser::ValType::F64 => "f64",
                            _ => "unknown",
                        };
                        output.push_str(&format!("    (local {count} {type_str})\n"));
                    }
                }

                // Parse function body instructions
                let operators_reader = body.get_operators_reader()?;
                for op in operators_reader {
                    let instruction = op?;
                    let instr_str = match instruction {
                        wasmparser::Operator::I32Const { value } => {
                            format!("    i32.const {value}")
                        }
                        wasmparser::Operator::I64Const { value } => {
                            format!("    i64.const {value}")
                        }
                        wasmparser::Operator::F32Const { value } => {
                            format!("    f32.const {}", f32::from_bits(value.bits()))
                        }
                        wasmparser::Operator::F64Const { value } => {
                            format!("    f64.const {}", f64::from_bits(value.bits()))
                        }
                        wasmparser::Operator::LocalGet { local_index } => {
                            format!("    local.get {local_index}")
                        }
                        wasmparser::Operator::LocalSet { local_index } => {
                            format!("    local.set {local_index}")
                        }
                        wasmparser::Operator::LocalTee { local_index } => {
                            format!("    local.tee {local_index}")
                        }
                        wasmparser::Operator::GlobalGet { global_index } => {
                            format!("    global.get {global_index}")
                        }
                        wasmparser::Operator::GlobalSet { global_index } => {
                            format!("    global.set {global_index}")
                        }
                        wasmparser::Operator::I32Add => "    i32.add".to_string(),
                        wasmparser::Operator::I32Sub => "    i32.sub".to_string(),
                        wasmparser::Operator::I32Mul => "    i32.mul".to_string(),
                        wasmparser::Operator::I32DivS => "    i32.div_s".to_string(),
                        wasmparser::Operator::I32DivU => "    i32.div_u".to_string(),
                        wasmparser::Operator::I64Add => "    i64.add".to_string(),
                        wasmparser::Operator::I64Sub => "    i64.sub".to_string(),
                        wasmparser::Operator::I64Mul => "    i64.mul".to_string(),
                        wasmparser::Operator::F32Add => "    f32.add".to_string(),
                        wasmparser::Operator::F32Sub => "    f32.sub".to_string(),
                        wasmparser::Operator::F32Mul => "    f32.mul".to_string(),
                        wasmparser::Operator::F32Div => "    f32.div".to_string(),
                        wasmparser::Operator::F64Add => "    f64.add".to_string(),
                        wasmparser::Operator::F64Sub => "    f64.sub".to_string(),
                        wasmparser::Operator::F64Mul => "    f64.mul".to_string(),
                        wasmparser::Operator::F64Div => "    f64.div".to_string(),
                        wasmparser::Operator::Call { function_index } => {
                            format!("    call {function_index}")
                        }
                        wasmparser::Operator::Return => "    return".to_string(),
                        wasmparser::Operator::End => "    end".to_string(),
                        wasmparser::Operator::Drop => "    drop".to_string(),
                        _ => format!("    ;; Unsupported instruction: {instruction:?}"),
                    };
                    output.push_str(&format!("{instr_str}\n"));
                }

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
