#![allow(clippy::uninlined_format_args)]
#![allow(deprecated)]

use std::env;
use std::fs;
use std::sync::Mutex;
use wasmtime::{Caller, Extern, Linker, Memory, Module, Result, Store};

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

    // Create engine and store using minimal (sync) configuration for wasmtime_runner
    let engine = clean_language_compiler::runtime::wasmtime_config::CleanWasmtimeConfig::create_minimal_engine()
        .map_err(|e| anyhow::anyhow!("Failed to create wasmtime engine: {e}"))?;
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
            let mem = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                print!("[print: ptr={ptr}, len={len}]");
                return;
            };

            let data = if let Some(data) = mem.data(&caller).get(ptr as usize..(ptr + len) as usize)
            {
                data
            } else {
                print!("[print: invalid range ptr={}, len={}]", ptr, len);
                return;
            };

            match std::str::from_utf8(data) {
                Ok(s) => {
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
            let mem = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                println!("[printl: ptr={ptr}, len={len}]");
                return;
            };

            let data = if let Some(data) = mem.data(&caller).get(ptr as usize..(ptr + len) as usize)
            {
                data
            } else {
                println!("[printl: invalid range ptr={ptr}, len={len}]");
                return;
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
    linker.func_wrap("env", "input", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "input_integer", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "input_float", |_: i32| -> f64 { 0.0 })?;
    linker.func_wrap("env", "input_yesno", |_: i32| -> i32 { 0 })?;
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

    // Conditional function imports - proper implementations
    linker.func_wrap(
        "env",
        "conditional_integer",
        |condition: i32, true_value: i32, false_value: i32| -> i32 {
            if condition != 0 {
                true_value
            } else {
                false_value
            }
        },
    )?;

    linker.func_wrap(
        "env",
        "conditional_number",
        |condition: i32, true_value: f64, false_value: f64| -> f64 {
            if condition != 0 {
                true_value
            } else {
                false_value
            }
        },
    )?;

    linker.func_wrap(
        "env",
        "conditional_boolean",
        |condition: i32, true_value: i32, false_value: i32| -> i32 {
            if condition != 0 {
                true_value
            } else {
                false_value
            }
        },
    )?;

    linker.func_wrap(
        "env",
        "conditional_string",
        |condition: i32, true_value: i32, false_value: i32| -> i32 {
            if condition != 0 {
                true_value
            } else {
                false_value
            }
        },
    )?;

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
            let string_value = value.to_string();

            // CRITICAL DEBUG: Check if the function is even being called
            println!(
                "🔥 CRITICAL: float_to_string IS BEING CALLED! Input: {}, Output: '{}'",
                value, string_value
            );

            // Get memory to store the string
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    let result = allocate_string_in_memory(&memory, &mut caller, &string_value);
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

    // Add string_concat function: string_concat(ptr1: i32, len1: i32, ptr2: i32, len2: i32) -> i32
    linker.func_wrap(
        "env",
        "string_concat",
        |mut caller: Caller<'_, ()>, ptr1: i32, len1: i32, ptr2: i32, len2: i32| -> i32 {
            eprintln!(
                "🔥 string_concat called: ptr1={}, len1={}, ptr2={}, len2={}",
                ptr1, len1, ptr2, len2
            );

            // Get memory
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                eprintln!("❌ string_concat: Failed to get memory");
                return 0; // Return null on failure
            };

            // Read first string
            let str1 = if let Some(data) = memory
                .data(&caller)
                .get(ptr1 as usize..(ptr1 + len1) as usize)
            {
                match std::str::from_utf8(data) {
                    Ok(s) => {
                        eprintln!("✅ string_concat: str1 = '{}'", s);
                        s.to_string()
                    }
                    Err(e) => {
                        eprintln!("❌ string_concat: str1 UTF-8 error: {}", e);
                        return 0;
                    }
                }
            } else {
                eprintln!("❌ string_concat: str1 out of bounds");
                return 0;
            };

            // Read second string
            let str2 = if let Some(data) = memory
                .data(&caller)
                .get(ptr2 as usize..(ptr2 + len2) as usize)
            {
                match std::str::from_utf8(data) {
                    Ok(s) => {
                        eprintln!("✅ string_concat: str2 = '{}'", s);
                        s.to_string()
                    }
                    Err(e) => {
                        eprintln!("❌ string_concat: str2 UTF-8 error: {}", e);
                        return 0;
                    }
                }
            } else {
                eprintln!("❌ string_concat: str2 out of bounds");
                return 0;
            };

            // Concatenate strings
            let result = str1 + &str2;
            eprintln!("✅ string_concat: result = '{}'", result);

            // Allocate and return result
            let result_ptr = allocate_string_in_memory(&memory, &mut caller, &result);
            eprintln!("✅ string_concat: returning ptr = {}", result_ptr);
            result_ptr
        },
    )?;

    linker.func_wrap("env", "string_to_int", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "string_to_float", |_: i32| -> f64 { 0.0 })?;

    // Add memory management functions
    linker.func_wrap(
        "memory_runtime",
        "mem_alloc",
        |_type_id: i32, size: i32| -> i32 {
            // Return a mock pointer for allocation
            1024 + size // Simple mock allocation
        },
    )?;

    linker.func_wrap("memory_runtime", "mem_retain", |_ptr: i32| {
        // Mock retain - does nothing
    })?;

    linker.func_wrap("memory_runtime", "mem_release", |_ptr: i32| {
        // Mock release - does nothing
    })?;

    // Add method-style function stubs
    linker.func_wrap(
        "env",
        "integer.toString",
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

    linker.func_wrap("env", "integer.toInteger", |value: i32| -> i32 { value })?;
    linker.func_wrap("env", "integer.toNumber", |value: i32| -> f64 {
        f64::from(value)
    })?;
    linker.func_wrap("env", "integer.toBoolean", |value: i32| -> i32 {
        i32::from(value != 0)
    })?;
    linker.func_wrap("env", "integer.length", |_: i32| -> i32 { 0 })?;

    linker.func_wrap(
        "env",
        "number.toString",
        |mut caller: Caller<'_, ()>, value: f64| -> i32 {
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

    linker.func_wrap("env", "number.toInteger", |value: f64| -> i32 {
        value as i32
    })?;
    linker.func_wrap("env", "number.toNumber", |value: f64| -> f64 { value })?;
    linker.func_wrap("env", "number.toBoolean", |value: f64| -> i32 {
        i32::from(value != 0.0)
    })?;
    linker.func_wrap("env", "number.length", |_: f64| -> i32 { 0 })?;

    linker.func_wrap("env", "string.toString", |value: i32| -> i32 { value })?;
    linker.func_wrap("env", "string.toInteger", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "string.toNumber", |_: i32| -> f64 { 0.0 })?;
    linker.func_wrap("env", "string.toBoolean", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "string.length", |_: i32| -> i32 { 0 })?;

    linker.func_wrap(
        "env",
        "boolean.toString",
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

    linker.func_wrap("env", "boolean.toInteger", |value: i32| -> i32 { value })?;
    linker.func_wrap("env", "boolean.toNumber", |value: i32| -> f64 {
        f64::from(value)
    })?;
    linker.func_wrap("env", "boolean.toBoolean", |value: i32| -> i32 { value })?;
    linker.func_wrap("env", "boolean.length", |_: i32| -> i32 { 0 })?;

    linker.func_wrap("env", "string.toUpperCase", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "string.toLowerCase", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "string.concat", |_: i32, _: i32| -> i32 { 0 })?;

    // Add array access function
    linker.func_wrap(
        "env",
        "array_get",
        |mut caller: Caller<'_, ()>, array_ptr: i32, index: i32| -> i32 {
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                println!("[array_get: no memory export]");
                return 0; // Return 0 on error
            };

            let data = memory.data(&caller);
            let ptr_usize = array_ptr as usize;

            // Check bounds for reading header
            if ptr_usize + 17 > data.len() {
                println!("[array_get: invalid array pointer {}]", array_ptr);
                return 0;
            }

            // Memory layout: [ref_count(4), type_id(4), size(4), gc_flags(1), length(4), data...]
            let length_offset = ptr_usize + 13;
            let data_offset = ptr_usize + 17;

            // Read array length from offset 13-16
            let length_bytes = &data[length_offset..length_offset + 4];
            let length = u32::from_le_bytes([
                length_bytes[0],
                length_bytes[1],
                length_bytes[2],
                length_bytes[3],
            ]) as i32;

            // Bounds check
            if index < 0 || index >= length {
                println!(
                    "[array_get: index {} out of bounds for array of length {}]",
                    index, length
                );
                return 0; // Return 0 for out-of-bounds access
            }

            // Each element is 4 bytes (i32), calculate element offset
            let element_offset = data_offset + (index as usize * 4);

            if element_offset + 4 > data.len() {
                println!("[array_get: element access out of memory bounds]");
                return 0;
            }

            // Read the element value
            let element_bytes = &data[element_offset..element_offset + 4];
            let element_value = i32::from_le_bytes([
                element_bytes[0],
                element_bytes[1],
                element_bytes[2],
                element_bytes[3],
            ]);

            element_value
        },
    )?;

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

    // Try to find and call the start function (try different possible names)
    let start_func = instance
        .get_func(&mut store, "start")
        .or_else(|| instance.get_func(&mut store, "_start"))
        .or_else(|| instance.get_func(&mut store, "main"))
        .or_else(|| instance.get_func(&mut store, "_main"));

    if let Some(func) = start_func {
        println!("🎯 Executing start function...");
        println!("--- Output ---");
        func.call(&mut store, &[], &mut [])?;
        println!("--- End Output ---");
        println!("✅ Execution completed successfully!");
    } else {
        println!("⚠️  No start function found. Available exports:");
        for export in instance.exports(&mut store) {
            let name = export.name().to_string();
            if export.into_func().is_some() {
                println!("  • {} (function)", name);
            }
        }
        return Err(anyhow::anyhow!("No start/main function found").into());
    }

    Ok(())
}
