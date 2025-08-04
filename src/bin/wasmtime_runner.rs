use std::env;
use std::fs;
use std::sync::Mutex;
use wasmtime::*;

// Global allocator for dynamic string storage
static NEXT_ALLOCATION_OFFSET: Mutex<usize> = Mutex::new(2048); // Start after static data

// Helper function to allocate memory for a string in WASM memory
fn allocate_string_in_memory(
    memory: &Memory,
    caller: &mut Caller<'_, ()>,
    string_value: &str,
) -> i32 {
    let string_bytes = string_value.as_bytes();
    let total_size = 4 + string_bytes.len(); // 4 bytes for length + string content

    // Get the next allocation offset
    let mut next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
    let offset = *next_offset;
    *next_offset += (total_size + 7) & !7; // Align to 8-byte boundary for next allocation
    drop(next_offset);

    // Get mutable memory data
    let data = memory.data_mut(caller);

    // Ensure we have enough memory
    if offset + total_size >= data.len() {
        println!("⚠️  WARNING: Not enough WASM memory for string allocation. Offset: {offset}, Size: {total_size}, Memory: {memory_len}", memory_len = data.len());
        return 0; // Return null pointer on failure
    }

    // Store length in first 4 bytes (little-endian)
    data[offset..offset + 4].copy_from_slice(&(string_bytes.len() as u32).to_le_bytes());

    // Store string content
    data[offset + 4..offset + 4 + string_bytes.len()].copy_from_slice(string_bytes);

    println!(
        "📝 DEBUG: Allocated '{}' (len={}) at address {}",
        string_value,
        string_bytes.len(),
        offset
    );

    offset as i32
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {program} <wasm_file>", program = args[0]);
        return Ok(());
    }

    let wasm_file = &args[1];
    println!("🚀 Loading WebAssembly file: {wasm_file}");

    // Read the WASM file
    let wasm_bytes = fs::read(wasm_file)?;
    println!("📦 File size: {len} bytes", len = wasm_bytes.len());

    // Create engine and store
    let engine = Engine::default();
    let mut store = Store::new(&engine, ());

    // Create module
    let module = Module::new(&engine, &wasm_bytes)?;

    // Create linker and add imports
    let mut linker = Linker::new(&engine);

    // Add print function: print(ptr: i32, len: i32) -> void
    linker.func_wrap(
        "env",
        "print",
        |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
            println!("\n🔍 DEBUG: print() called with ptr={ptr}, len={len}");

            let mem = match caller.get_export("memory") {
                Some(Extern::Memory(mem)) => mem,
                _ => {
                    print!("[print: ptr={ptr}, len={len}]");
                    return;
                }
            };

            let data = match mem.data(&caller).get(ptr as usize..(ptr + len) as usize) {
                Some(data) => data,
                None => {
                    print!("[print: invalid range ptr={}, len={}]", ptr, len);
                    return;
                }
            };

            println!("🔍 DEBUG: Raw bytes at ptr {ptr}: {data:?}");

            match std::str::from_utf8(data) {
                Ok(s) => {
                    println!("🔍 DEBUG: Decoded string: '{s}'");
                    print!("{}", s);
                }
                Err(_) => print!("[invalid utf8: {} bytes]", len),
            }
        },
    )?;

    // Add printl function: printl(ptr: i32, len: i32) -> void
    linker.func_wrap(
        "env",
        "printl",
        |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
            let mem = match caller.get_export("memory") {
                Some(Extern::Memory(mem)) => mem,
                _ => {
                    println!("[printl: ptr={ptr}, len={len}]");
                    return;
                }
            };

            let data = match mem.data(&caller).get(ptr as usize..(ptr + len) as usize) {
                Some(data) => data,
                None => {
                    println!("[printl: invalid range ptr={ptr}, len={len}]");
                    return;
                }
            };

            match std::str::from_utf8(data) {
                Ok(s) => println!("{s}"),
                Err(_) => println!("[invalid utf8: {len} bytes]"),
            }
        },
    )?;

    // Add print_simple function: print_simple(value: i32) -> void
    linker.func_wrap("env", "print_simple", |value: i32| {
        print!("{}", value);
    })?;

    // Add printl_simple function: printl_simple(value: i32) -> void
    linker.func_wrap("env", "printl_simple", |value: i32| {
        println!("{value}");
    })?;

    // Add file operation stubs (they won't be used in this test)
    linker.func_wrap(
        "env",
        "file_write",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap("env", "file_read", |_: i32, _: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "file_exists", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "file_delete", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap(
        "env",
        "file_append",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap("env", "http_get", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap(
        "env",
        "http_post",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap("env", "http_put", |_: i32, _: i32, _: i32, _: i32| -> i32 {
        0
    })?;
    linker.func_wrap("env", "http_delete", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap(
        "env",
        "http_patch",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap("env", "http_head", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_options", |_: i32, _: i32| -> i32 { 0 })?;

    // Add additional required imports
    linker.func_wrap("env", "input", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "input_integer", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "input_float", |_: i32, _: i32| -> f64 { 0.0 })?;
    linker.func_wrap("env", "input_yesno", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap(
        "env",
        "input_range",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;

    // HTTP imports with headers
    linker.func_wrap(
        "env",
        "http_get_with_headers",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_post_with_headers",
        |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_post_json",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_put_json",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_patch_json",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "env",
        "http_post_form",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap("env", "http_set_user_agent", |_: i32, _: i32| {})?;
    linker.func_wrap("env", "http_set_timeout", |_: i32| {})?;
    linker.func_wrap("env", "http_set_max_redirects", |_: i32| {})?;
    linker.func_wrap("env", "http_enable_cookies", |_: i32| {})?;
    linker.func_wrap("env", "http_get_response_code", || -> i32 { 0 })?;
    linker.func_wrap("env", "http_get_response_headers", || -> i32 { 0 })?;
    linker.func_wrap("env", "http_encode_url", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_decode_url", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_build_query", |_: i32, _: i32| -> i32 { 0 })?;

    // Type conversion imports - proper implementations
    linker.func_wrap(
        "env",
        "int_to_string",
        |mut caller: Caller<'_, ()>, value: i32| -> i32 {
            let string_value = value.to_string();

            // Get memory to store the string
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    return allocate_string_in_memory(&memory, &mut caller, &string_value);
                }
            }

            0 // Return null pointer on failure
        },
    )?;

    linker.func_wrap(
        "env",
        "float_to_string",
        |mut caller: Caller<'_, ()>, value: f64| -> i32 {
            println!("🔍 DEBUG: float_to_string called with value = {value}");
            let string_value = value.to_string();
            println!("🔍 DEBUG: Converted to string: '{string_value}'");

            // CRITICAL DEBUG: Check if the function is even being called
            println!(
                "🔥 CRITICAL: float_to_string IS BEING CALLED! Input: {}, Output: '{}'",
                value, string_value
            );

            // Get memory to store the string
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    let result = allocate_string_in_memory(&memory, &mut caller, &string_value);
                    println!("🔍 DEBUG: float_to_string returning address {result}");
                    return result;
                }
            }

            0 // Return null pointer on failure
        },
    )?;

    linker.func_wrap(
        "env",
        "bool_to_string",
        |mut caller: Caller<'_, ()>, value: i32| -> i32 {
            let string_value = if value != 0 { "true" } else { "false" };

            // Get memory to store the string
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    return allocate_string_in_memory(&memory, &mut caller, string_value);
                }
            }

            0 // Return null pointer on failure
        },
    )?;

    linker.func_wrap("env", "string_to_int", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "string_to_float", |_: i32| -> f64 { 0.0 })?;

    // Instantiate the module
    let instance = linker.instantiate(&mut store, &module)?;

    println!("✅ WebAssembly module loaded successfully");
    println!(
        "📋 Exported functions: {:?}",
        instance
            .exports(&mut store)
            .map(|e| e.name())
            .collect::<Vec<_>>()
    );

    // Get and call the start function
    let start_func = instance
        .get_func(&mut store, "start")
        .ok_or_else(|| anyhow::anyhow!("start function not found"))?;
    println!("🎯 Executing start function...");
    println!("--- Output ---");
    start_func.call(&mut store, &[], &mut [])?;
    println!("--- End Output ---");
    println!("✅ Execution completed successfully!");

    Ok(())
}
