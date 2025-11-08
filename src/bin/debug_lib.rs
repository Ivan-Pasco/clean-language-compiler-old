use clean_language_compiler::compile;
use std::fs;
use wasmtime::{Engine, Instance, Module, Store};

fn main() {
    let source = r"functions:
	number add(number a, number b)
		return a + b

start()
	number result = add(40, 2)
	print(result)";

    println!("Compiling source...");
    let wasm_binary = match compile(source) {
        Ok(binary) => {
            println!(
                "✓ Compilation successful! Generated {len} bytes",
                len = binary.len()
            );

            // Write to file for debugging
            fs::write("debug_lib_output.wasm", &binary).expect("Failed to write WASM file");
            println!("✓ WASM written to debug_lib_output.wasm");

            binary
        }
        Err(e) => {
            println!("✗ Compilation failed: {e:?}");
            return;
        }
    };

    println!("Creating WebAssembly module...");
    let engine = Engine::default();
    let module = match Module::new(&engine, &wasm_binary) {
        Ok(module) => {
            println!("✓ Module creation successful!");
            module
        }
        Err(e) => {
            println!("✗ Module creation failed: {e}");
            return;
        }
    };

    println!("Creating store and instance...");
    let mut store = Store::new(&engine, ());
    let instance = match Instance::new(&mut store, &module, &[]) {
        Ok(instance) => {
            println!("✓ Instance creation successful!");
            instance
        }
        Err(e) => {
            println!("✗ Instance creation failed: {e}");
            return;
        }
    };

    println!("Looking for start function...");
    if let Some(start_func) = instance.get_func(&mut store, "start") {
        println!("✓ Found start function, calling it...");
        let mut results = [];
        match start_func.call(&mut store, &[], &mut results) {
            Ok(()) => println!("✓ Function call successful!"),
            Err(e) => println!("✗ Function call failed: {e}"),
        }
    } else {
        println!("✗ Start function not found");
    }

    println!("\nAvailable exports:");
    for export in module.exports() {
        println!("  - {name}: {ty:?}", name = export.name(), ty = export.ty());
    }
}
