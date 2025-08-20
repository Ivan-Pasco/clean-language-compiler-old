use clean_language_compiler::compile_minimal;
use wasmtime::Module;

fn main() {
    let source = r#"
start()
	number sum = 0

	iterate i in 1 to 5
		if i > 2
			sum = sum + i

	list<number> numbers = [1, 2, 3, 4, 5]
	iterate n in numbers
		if n > 3
			sum = sum + n
"#;

    println!("Compiling with compile_minimal...");
    let wasm_binary = compile_minimal(source).expect("Failed to compile");
    println!("Generated WASM binary size: {len}", len = wasm_binary.len());

    println!("Creating engine...");
    let engine =
        clean_language_compiler::runtime::wasmtime_config::CleanWasmtimeConfig::create_engine()
            .expect("Failed to create wasmtime engine");

    println!("Creating module...");
    match Module::new(&engine, &wasm_binary) {
        Ok(_module) => println!("Module created successfully!"),
        Err(e) => {
            println!("Failed to create module: {e}");

            // Write the binary to a file for inspection
            std::fs::write("test_minimal_debug.wasm", &wasm_binary)
                .expect("Failed to write WASM file");
            println!("WASM binary written to test_minimal_debug.wasm for inspection");
        }
    }
}
