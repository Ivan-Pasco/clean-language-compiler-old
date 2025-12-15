#![allow(clippy::uninlined_format_args)]
#![allow(deprecated)]

use std::env;
use std::fs;
use std::sync::Mutex;
use wasmtime::{Caller, Extern, Linker, Memory, Module, Result, Store};

// Global allocator for dynamic string storage with scope-based arena management
static NEXT_ALLOCATION_OFFSET: Mutex<usize> = Mutex::new(2048); // Start after static data

// Scope marks for arena-style memory management
// When entering a scope (loop, function), push current offset
// When exiting scope, pop and reset - instant deallocation of all scope allocations
static SCOPE_MARKS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

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

    // HTTP server stub functions (not fully implemented)
    linker.func_wrap(
        "env",
        "_http_route",
        |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    linker.func_wrap("env", "_http_listen", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "_req_param", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "_req_query", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "_req_body", || -> i32 { 0 })?;
    linker.func_wrap("env", "_req_header", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "_req_method", || -> i32 { 0 })?;
    linker.func_wrap("env", "_req_path", || -> i32 { 0 })?;

    // Math function imports - proper implementations
    linker.func_wrap("env", "math_pow", |base: f64, exp: f64| -> f64 {
        base.powf(exp)
    })?;
    linker.func_wrap("env", "math.pow", |base: f64, exp: f64| -> f64 {
        base.powf(exp)
    })?;
    linker.func_wrap("env", "math_sin", |x: f64| -> f64 { x.sin() })?;
    linker.func_wrap("env", "math.sin", |x: f64| -> f64 { x.sin() })?;
    linker.func_wrap("env", "math_cos", |x: f64| -> f64 { x.cos() })?;
    linker.func_wrap("env", "math.cos", |x: f64| -> f64 { x.cos() })?;
    linker.func_wrap("env", "math_tan", |x: f64| -> f64 { x.tan() })?;
    linker.func_wrap("env", "math.tan", |x: f64| -> f64 { x.tan() })?;
    linker.func_wrap("env", "math_asin", |x: f64| -> f64 { x.asin() })?;
    linker.func_wrap("env", "math.asin", |x: f64| -> f64 { x.asin() })?;
    linker.func_wrap("env", "math_acos", |x: f64| -> f64 { x.acos() })?;
    linker.func_wrap("env", "math.acos", |x: f64| -> f64 { x.acos() })?;
    linker.func_wrap("env", "math_atan", |x: f64| -> f64 { x.atan() })?;
    linker.func_wrap("env", "math.atan", |x: f64| -> f64 { x.atan() })?;
    linker.func_wrap("env", "math_atan2", |y: f64, x: f64| -> f64 { y.atan2(x) })?;
    linker.func_wrap("env", "math.atan2", |y: f64, x: f64| -> f64 { y.atan2(x) })?;
    linker.func_wrap("env", "math_sinh", |x: f64| -> f64 { x.sinh() })?;
    linker.func_wrap("env", "math.sinh", |x: f64| -> f64 { x.sinh() })?;
    linker.func_wrap("env", "math_cosh", |x: f64| -> f64 { x.cosh() })?;
    linker.func_wrap("env", "math.cosh", |x: f64| -> f64 { x.cosh() })?;
    linker.func_wrap("env", "math_tanh", |x: f64| -> f64 { x.tanh() })?;
    linker.func_wrap("env", "math.tanh", |x: f64| -> f64 { x.tanh() })?;
    linker.func_wrap("env", "math_ln", |x: f64| -> f64 { x.ln() })?;
    linker.func_wrap("env", "math.ln", |x: f64| -> f64 { x.ln() })?;
    linker.func_wrap("env", "math_log10", |x: f64| -> f64 { x.log10() })?;
    linker.func_wrap("env", "math.log10", |x: f64| -> f64 { x.log10() })?;
    linker.func_wrap("env", "math_log2", |x: f64| -> f64 { x.log2() })?;
    linker.func_wrap("env", "math.log2", |x: f64| -> f64 { x.log2() })?;
    linker.func_wrap("env", "math_exp", |x: f64| -> f64 { x.exp() })?;
    linker.func_wrap("env", "math.exp", |x: f64| -> f64 { x.exp() })?;
    linker.func_wrap("env", "math_exp2", |x: f64| -> f64 { x.exp2() })?;
    linker.func_wrap("env", "math.exp2", |x: f64| -> f64 { x.exp2() })?;
    linker.func_wrap("env", "math_sqrt", |x: f64| -> f64 { x.sqrt() })?;
    linker.func_wrap("env", "math.sqrt", |x: f64| -> f64 { x.sqrt() })?;
    linker.func_wrap("env", "math_floor", |x: f64| -> f64 { x.floor() })?;
    linker.func_wrap("env", "math.floor", |x: f64| -> f64 { x.floor() })?;
    linker.func_wrap("env", "math_ceil", |x: f64| -> f64 { x.ceil() })?;
    linker.func_wrap("env", "math.ceil", |x: f64| -> f64 { x.ceil() })?;
    linker.func_wrap("env", "math_round", |x: f64| -> f64 { x.round() })?;
    linker.func_wrap("env", "math.round", |x: f64| -> f64 { x.round() })?;
    linker.func_wrap("env", "math_abs", |x: f64| -> f64 { x.abs() })?;
    linker.func_wrap("env", "math.abs", |x: f64| -> f64 { x.abs() })?;

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

    // FIXED: string.concat(ptr1: i32, ptr2: i32) -> i32
    // Each pointer points to a length-prefixed string: [4-byte little-endian len][content]
    // Returns pointer to new length-prefixed concatenated string
    linker.func_wrap(
        "env",
        "string.concat",
        |mut caller: Caller<'_, ()>, ptr1: i32, ptr2: i32| -> i32 {
            // Get memory
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                eprintln!("string.concat: Failed to get memory");
                return 0; // Return null on failure
            };

            let data = memory.data(&caller);

            // Read first string (length-prefixed)
            let p1 = ptr1 as usize;
            if p1 + 4 > data.len() {
                eprintln!("string.concat: ptr1 out of bounds");
                return 0;
            }
            let len1 =
                u32::from_le_bytes([data[p1], data[p1 + 1], data[p1 + 2], data[p1 + 3]]) as usize;
            let str1 = if p1 + 4 + len1 <= data.len() {
                match std::str::from_utf8(&data[p1 + 4..p1 + 4 + len1]) {
                    Ok(s) => s.to_string(),
                    Err(e) => {
                        eprintln!("string.concat: str1 UTF-8 error: {}", e);
                        return 0;
                    }
                }
            } else {
                eprintln!("string.concat: str1 content out of bounds");
                return 0;
            };

            // Read second string (length-prefixed)
            let p2 = ptr2 as usize;
            if p2 + 4 > data.len() {
                eprintln!("string.concat: ptr2 out of bounds");
                return 0;
            }
            let len2 =
                u32::from_le_bytes([data[p2], data[p2 + 1], data[p2 + 2], data[p2 + 3]]) as usize;
            let str2 = if p2 + 4 + len2 <= data.len() {
                match std::str::from_utf8(&data[p2 + 4..p2 + 4 + len2]) {
                    Ok(s) => s.to_string(),
                    Err(e) => {
                        eprintln!("string.concat: str2 UTF-8 error: {}", e);
                        return 0;
                    }
                }
            } else {
                eprintln!("string.concat: str2 content out of bounds");
                return 0;
            };

            // Concatenate strings
            let result = str1 + &str2;

            // Allocate and return result
            allocate_string_in_memory(&memory, &mut caller, &result)
        },
    )?;

    // FIXED: Also register string_concat (underscore version) for WASM imports that use underscore naming
    linker.func_wrap(
        "env",
        "string_concat",
        |mut caller: Caller<'_, ()>, ptr1: i32, ptr2: i32| -> i32 {
            // Get memory
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                eprintln!("string_concat: Failed to get memory");
                return 0;
            };

            let data = memory.data(&caller);

            // Read first string (length-prefixed)
            let p1 = ptr1 as usize;
            if p1 + 4 > data.len() {
                eprintln!("string_concat: ptr1 out of bounds");
                return 0;
            }
            let len1 =
                u32::from_le_bytes([data[p1], data[p1 + 1], data[p1 + 2], data[p1 + 3]]) as usize;
            let str1 = if p1 + 4 + len1 <= data.len() {
                match std::str::from_utf8(&data[p1 + 4..p1 + 4 + len1]) {
                    Ok(s) => s.to_string(),
                    Err(e) => {
                        eprintln!("string_concat: str1 UTF-8 error: {}", e);
                        return 0;
                    }
                }
            } else {
                eprintln!("string_concat: str1 content out of bounds");
                return 0;
            };

            // Read second string (length-prefixed)
            let p2 = ptr2 as usize;
            if p2 + 4 > data.len() {
                eprintln!("string_concat: ptr2 out of bounds");
                return 0;
            }
            let len2 =
                u32::from_le_bytes([data[p2], data[p2 + 1], data[p2 + 2], data[p2 + 3]]) as usize;
            let str2 = if p2 + 4 + len2 <= data.len() {
                match std::str::from_utf8(&data[p2 + 4..p2 + 4 + len2]) {
                    Ok(s) => s.to_string(),
                    Err(e) => {
                        eprintln!("string_concat: str2 UTF-8 error: {}", e);
                        return 0;
                    }
                }
            } else {
                eprintln!("string_concat: str2 content out of bounds");
                return 0;
            };

            // Concatenate strings
            let result = str1 + &str2;

            // Allocate and return result
            allocate_string_in_memory(&memory, &mut caller, &result)
        },
    )?;

    linker.func_wrap("env", "string_to_int", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "string_to_float", |_: i32| -> f64 { 0.0 })?;

    // Add memory management functions
    linker.func_wrap(
        "memory_runtime",
        "mem_alloc",
        |_type_id: i32, size: i32| -> i32 {
            // Use the global allocator for proper memory allocation
            let mut next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
            let offset = *next_offset;
            // Align size to 8-byte boundary and advance
            *next_offset += ((size as usize) + 7) & !7;
            offset as i32
        },
    )?;

    linker.func_wrap("memory_runtime", "mem_retain", |_ptr: i32| {
        // Mock retain - does nothing
    })?;

    linker.func_wrap("memory_runtime", "mem_release", |_ptr: i32| {
        // Mock release - does nothing (use scope-based release instead)
    })?;

    // Scope-based memory management for arena allocation
    // Push a scope mark - saves current allocation offset for later reset
    linker.func_wrap("memory_runtime", "mem_scope_push", || {
        let next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
        let mut scope_marks = SCOPE_MARKS.lock().unwrap();
        scope_marks.push(*next_offset);
    })?;

    // Pop a scope mark - resets allocation offset to saved mark
    // This instantly "frees" all memory allocated within the scope
    linker.func_wrap("memory_runtime", "mem_scope_pop", || {
        let mut scope_marks = SCOPE_MARKS.lock().unwrap();
        if let Some(mark) = scope_marks.pop() {
            let mut next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
            *next_offset = mark;
        }
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
    // string.concat is registered above with 2-param implementation (length-prefixed pointers)

    // Add string_split function: splits a string by delimiter and returns a list of strings
    linker.func_wrap(
        "env",
        "string_split",
        |mut caller: Caller<'_, ()>, string_ptr: i32, delimiter_ptr: i32| -> i32 {
            eprintln!(
                "🔍 string_split called: string_ptr={}, delimiter_ptr={}",
                string_ptr, delimiter_ptr
            );

            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                eprintln!("❌ string_split: no memory");
                return 0; // Return null on error
            };

            // Read the string and delimiter from memory
            let data = memory.data(&caller);

            // Read string: first 4 bytes are length, then content
            let str_ptr = string_ptr as usize;
            if str_ptr + 4 > data.len() {
                return 0;
            }
            let str_len = u32::from_le_bytes([
                data[str_ptr],
                data[str_ptr + 1],
                data[str_ptr + 2],
                data[str_ptr + 3],
            ]) as usize;

            if str_ptr + 4 + str_len > data.len() {
                return 0;
            }
            let string_content =
                match std::str::from_utf8(&data[str_ptr + 4..str_ptr + 4 + str_len]) {
                    Ok(s) => s.to_string(),
                    Err(_) => return 0,
                };

            // Read delimiter
            let delim_ptr = delimiter_ptr as usize;
            if delim_ptr + 4 > data.len() {
                return 0;
            }
            let delim_len = u32::from_le_bytes([
                data[delim_ptr],
                data[delim_ptr + 1],
                data[delim_ptr + 2],
                data[delim_ptr + 3],
            ]) as usize;

            if delim_ptr + 4 + delim_len > data.len() {
                return 0;
            }
            let delimiter =
                match std::str::from_utf8(&data[delim_ptr + 4..delim_ptr + 4 + delim_len]) {
                    Ok(s) => s.to_string(),
                    Err(_) => return 0,
                };

            // Split the string
            let parts: Vec<&str> = string_content.split(&delimiter).collect();
            let num_parts = parts.len();
            eprintln!(
                "🔍 string_split: string='{}', delimiter='{}', parts={:?}",
                string_content, delimiter, parts
            );

            // Allocate list structure:
            // List header: size(4) + capacity(4) + type_id(4) + padding(4) = 16 bytes
            // Then: num_parts * 4 bytes for string pointers
            let list_size = 16 + num_parts * 4;

            // Get allocation offset
            let mut next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
            let list_ptr = *next_offset;
            *next_offset += (list_size + 7) & !7;
            drop(next_offset);

            // Now allocate all the string parts
            let mut string_ptrs = Vec::new();
            for part in &parts {
                let part_bytes = part.as_bytes();
                let part_total_size = 4 + part_bytes.len();

                let mut next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
                let part_ptr = *next_offset;
                *next_offset += (part_total_size + 7) & !7;
                drop(next_offset);

                string_ptrs.push(part_ptr as i32);
            }

            // Now write everything to memory
            let data_mut = memory.data_mut(&mut caller);

            // Check we have enough memory
            let last_needed = if string_ptrs.is_empty() {
                list_ptr + list_size
            } else {
                let last_ptr = *string_ptrs.last().unwrap() as usize;
                let last_str = parts.last().unwrap();
                last_ptr + 4 + last_str.len()
            };

            if last_needed > data_mut.len() {
                return 0; // Not enough memory
            }

            // Write list header
            // size (number of elements)
            data_mut[list_ptr..list_ptr + 4].copy_from_slice(&(num_parts as u32).to_le_bytes());
            // capacity
            data_mut[list_ptr + 4..list_ptr + 8].copy_from_slice(&(num_parts as u32).to_le_bytes());
            // type_id (3 = i32 list of string pointers)
            data_mut[list_ptr + 8..list_ptr + 12].copy_from_slice(&4u32.to_le_bytes());
            // padding
            data_mut[list_ptr + 12..list_ptr + 16].copy_from_slice(&0u32.to_le_bytes());

            // Write element pointers
            for (i, ptr) in string_ptrs.iter().enumerate() {
                let offset = list_ptr + 16 + i * 4;
                data_mut[offset..offset + 4].copy_from_slice(&ptr.to_le_bytes());
            }

            // Write string contents
            for (i, part) in parts.iter().enumerate() {
                let ptr = string_ptrs[i] as usize;
                let part_bytes = part.as_bytes();

                // Write length
                data_mut[ptr..ptr + 4].copy_from_slice(&(part_bytes.len() as u32).to_le_bytes());
                // Write content
                data_mut[ptr + 4..ptr + 4 + part_bytes.len()].copy_from_slice(part_bytes);
            }

            eprintln!("🔍 string_split: returning list at ptr={}", list_ptr);
            list_ptr as i32
        },
    )?;

    // Add string.split function (alias for string_split with dot notation for Clean Language)
    linker.func_wrap(
        "env",
        "string.split",
        |mut caller: Caller<'_, ()>, string_ptr: i32, delimiter_ptr: i32| -> i32 {
            eprintln!(
                "🔍 string.split called: string_ptr={}, delimiter_ptr={}",
                string_ptr, delimiter_ptr
            );

            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                eprintln!("❌ string.split: no memory");
                return 0;
            };

            let data = memory.data(&caller);

            // Read original string
            let str_ptr = string_ptr as usize;
            if str_ptr + 4 > data.len() {
                eprintln!("❌ string.split: invalid string ptr");
                return 0;
            }
            let str_len = u32::from_le_bytes([
                data[str_ptr],
                data[str_ptr + 1],
                data[str_ptr + 2],
                data[str_ptr + 3],
            ]) as usize;
            let original_str = if str_ptr + 4 + str_len <= data.len() {
                String::from_utf8_lossy(&data[str_ptr + 4..str_ptr + 4 + str_len]).to_string()
            } else {
                String::new()
            };

            // Read delimiter
            let delim_ptr = delimiter_ptr as usize;
            if delim_ptr + 4 > data.len() {
                eprintln!("❌ string.split: invalid delimiter ptr");
                return 0;
            }
            let delim_len = u32::from_le_bytes([
                data[delim_ptr],
                data[delim_ptr + 1],
                data[delim_ptr + 2],
                data[delim_ptr + 3],
            ]) as usize;
            let delimiter = if delim_ptr + 4 + delim_len <= data.len() {
                String::from_utf8_lossy(&data[delim_ptr + 4..delim_ptr + 4 + delim_len]).to_string()
            } else {
                String::new()
            };

            eprintln!(
                "🔍 string.split: splitting '{}' by '{}'",
                original_str, delimiter
            );

            // Split the string
            let parts: Vec<&str> = original_str.split(&delimiter).collect();
            let num_parts = parts.len();
            eprintln!("🔍 string.split: found {} parts", num_parts);

            // Calculate total memory needed
            // List header: 16 bytes (size, capacity, type_id, padding)
            // List data: num_parts * 4 bytes (string pointers)
            let list_size = 16 + (num_parts * 4);

            // Use a proper heap allocation strategy
            // Memory layout: static data up to ~4400, then heap starts
            // Read current heap pointer from global at offset 0 (initialized to 4 means empty)
            let global_heap_ptr_offset = 0usize;
            let current_heap = u32::from_le_bytes([
                data[global_heap_ptr_offset],
                data[global_heap_ptr_offset + 1],
                data[global_heap_ptr_offset + 2],
                data[global_heap_ptr_offset + 3],
            ]) as usize;

            // If heap pointer is at initial value (0-4 or any small value), start after static data
            // Static data typically ends around 4500 based on string pool usage
            let list_ptr = if current_heap < 5000 {
                5000
            } else {
                current_heap
            };
            let mut next_ptr = list_ptr + list_size;

            // Allocate string pointers
            let mut string_ptrs = Vec::with_capacity(num_parts);
            for part in &parts {
                let part_len = part.len();
                let str_allocation = 4 + part_len; // 4 bytes for length + content
                string_ptrs.push(next_ptr as u32);
                next_ptr += str_allocation;
            }

            // Update heap pointer for future allocations
            let new_heap_ptr = next_ptr as u32;

            let data_mut = memory.data_mut(&mut caller);
            data_mut[global_heap_ptr_offset..global_heap_ptr_offset + 4]
                .copy_from_slice(&new_heap_ptr.to_le_bytes());

            // Write list header
            data_mut[list_ptr..list_ptr + 4].copy_from_slice(&(num_parts as u32).to_le_bytes()); // size
            data_mut[list_ptr + 4..list_ptr + 8].copy_from_slice(&(num_parts as u32).to_le_bytes()); // capacity
            data_mut[list_ptr + 8..list_ptr + 12].copy_from_slice(&3u32.to_le_bytes()); // type_id (3 = string)
            data_mut[list_ptr + 12..list_ptr + 16].copy_from_slice(&0u32.to_le_bytes()); // padding

            // Write string pointers to list
            for (i, &ptr) in string_ptrs.iter().enumerate() {
                let offset = list_ptr + 16 + (i * 4);
                data_mut[offset..offset + 4].copy_from_slice(&ptr.to_le_bytes());
            }

            // Write string contents
            for (i, part) in parts.iter().enumerate() {
                let ptr = string_ptrs[i] as usize;
                let part_bytes = part.as_bytes();

                // Write length
                data_mut[ptr..ptr + 4].copy_from_slice(&(part_bytes.len() as u32).to_le_bytes());
                // Write content
                data_mut[ptr + 4..ptr + 4 + part_bytes.len()].copy_from_slice(part_bytes);
            }

            eprintln!("🔍 string.split: returning list at ptr={}", list_ptr);
            list_ptr as i32
        },
    )?;

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
