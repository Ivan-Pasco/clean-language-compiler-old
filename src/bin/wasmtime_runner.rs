#![allow(clippy::uninlined_format_args)]
#![allow(deprecated)]

use std::env;
use std::fs;
use std::sync::Mutex;
use wasmtime::{Caller, Extern, Linker, Memory, Module, Result, Store};

// Global allocator for dynamic string storage with scope-based arena management.
// Starts at 1 MB — the boundary between compile-time data sections and the
// runtime heap.  Must match `native_stdlib::HEAP_START` (1,048,576) in
// `src/codegen/native_stdlib/mod.rs`.  See MEMORY_POLICY.md section 7 for the
// full memory layout:  [0..1KB] reserved, [1KB..1MB] data section, [1MB..] heap.
static NEXT_ALLOCATION_OFFSET: Mutex<usize> = Mutex::new(1048576);

// Scope marks for arena-style memory management
// When entering a scope (loop, function), push current offset
// When exiting scope, pop and reset - instant deallocation of all scope allocations
static SCOPE_MARKS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

// Helper function to allocate memory for a string in WASM memory
// CRITICAL: This must keep WASM global 0 (__heap_ptr) in sync with the host-side
// allocation offset so that WASM-side malloc doesn't return addresses that overlap
// with host-allocated data.
fn allocate_string_in_memory(
    memory: &Memory,
    caller: &mut Caller<'_, ()>,
    string_value: &str,
) -> i32 {
    let string_bytes = string_value.as_bytes();
    let total_size = 4 + string_bytes.len(); // 4 bytes for length + string content

    // Step 1: Sync host-side offset with WASM __heap_ptr (global 0).
    // The WASM side may have advanced __heap_ptr via its own malloc calls,
    // so we must take the maximum of both to avoid overlapping allocations.
    let wasm_heap = if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
        heap_global.get(&mut *caller).i32().unwrap_or(0) as usize
    } else {
        0
    };

    let mut next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
    if wasm_heap > *next_offset {
        *next_offset = wasm_heap;
    }

    let offset = *next_offset;
    let aligned_end = offset + ((total_size + 7) & !7);
    *next_offset = aligned_end;
    drop(next_offset);

    // Step 2: Grow memory if needed
    let current_pages = memory.size(&caller) as usize;
    let current_bytes = current_pages * 65536;
    if aligned_end > current_bytes {
        let needed_pages = (aligned_end - current_bytes).div_ceil(65536) as u64;
        if memory.grow(&mut *caller, needed_pages).is_err() {
            println!("⚠️  WARNING: memory.grow failed for {needed_pages} pages");
            return 0;
        }
    }

    // Step 3: Write string data
    let data = memory.data_mut(&mut *caller);
    if offset + total_size > data.len() {
        println!("⚠️  WARNING: Not enough WASM memory for string allocation. Offset: {offset}, Size: {total_size}, Memory: {memory_len}", memory_len = data.len());
        return 0;
    }

    // Store length in first 4 bytes (little-endian)
    data[offset..offset + 4].copy_from_slice(&(string_bytes.len() as u32).to_le_bytes());

    // Store string content
    data[offset + 4..offset + 4 + string_bytes.len()].copy_from_slice(string_bytes);

    // Step 4: Update WASM global 0 (__heap_ptr) so WASM-side malloc starts PAST this allocation
    if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
        heap_global
            .set(&mut *caller, wasmtime::Val::I32(aligned_end as i32))
            .ok();
    }

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

    // Add print_integer function: print_integer(value: i64) -> void
    linker.func_wrap("env", "print_integer", |value: i64| {
        print!("{}", value);
    })?;

    // env::error(msg_ptr: i32) -> i32 — print to stderr and return the same
    // pointer so the value passes through expression context unchanged.
    linker.func_wrap(
        "env",
        "error",
        |mut caller: Caller<'_, ()>, msg_ptr: i32| -> i32 {
            if let Some(Extern::Memory(memory)) = caller.get_export("memory") {
                let data = memory.data(&caller);
                let start = msg_ptr as usize;
                if start + 4 <= data.len() {
                    let len =
                        u32::from_le_bytes(data[start..start + 4].try_into().unwrap_or([0; 4]))
                            as usize;
                    let bytes_start = start + 4;
                    if bytes_start + len <= data.len() {
                        if let Ok(s) = std::str::from_utf8(&data[bytes_start..bytes_start + len]) {
                            eprintln!("error: {s}");
                        }
                    }
                }
            }
            msg_ptr
        },
    )?;

    // Add print_float function: print_float(value: f64) -> void
    linker.func_wrap("env", "print_float", |value: f64| {
        print!("{}", value);
    })?;

    // Add print_boolean function: print_boolean(value: i32) -> void
    linker.func_wrap("env", "print_boolean", |value: i32| {
        print!("{}", if value != 0 { "true" } else { "false" });
    })?;

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
    // State reset stubs (runtime resets are no-ops in standalone test runner)
    linker.func_wrap("env", "_state_reset_all", || {})?;
    linker.func_wrap("env", "_state_reset_named", |_: i32| {})?;

    // Server sleep stub
    linker.func_wrap("env", "_server_sleep", |_: i64| {})?;

    // Async bridge stubs — fire-and-forget and await-result are no-ops in standalone runner
    linker.func_wrap("env", "_async_fire", |_: i32, _: i32, _: i32, _: i32| {})?;
    linker.func_wrap(
        "env",
        "_async_await",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;

    linker.func_wrap("env", "_req_param", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "_req_query", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "_req_body", || -> i32 { 0 })?;
    linker.func_wrap("env", "_req_header", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "_req_method", || -> i32 { 0 })?;
    linker.func_wrap("env", "_req_path", || -> i32 { 0 })?;
    linker.func_wrap("env", "_req_cookie", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap(
        "env",
        "_http_route_protected",
        |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    // Session: _session_store(user_id: i32, rolePtr: i32, roleLen: i32, claimsPtr: i32, claimsLen: i32) -> i32
    linker.func_wrap(
        "env",
        "_session_store",
        |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    // Session: _session_get() -> i32
    linker.func_wrap("env", "_session_get", || -> i32 { 0 })?;
    // Session: _session_delete() -> i32
    linker.func_wrap("env", "_session_delete", || -> i32 { 0 })?;
    // Session: _http_set_cookie(cookiePtr: i32, cookieLen: i32) -> i32
    linker.func_wrap("env", "_http_set_cookie", |_: i32, _: i32| -> i32 { 0 })?;
    // Auth: _auth_get_session() -> i32
    linker.func_wrap("env", "_auth_get_session", || -> i32 { 0 })?;
    // Auth: _auth_require_auth() -> i32
    linker.func_wrap("env", "_auth_require_auth", || -> i32 { 0 })?;
    // Auth: _auth_require_role(rolePtr: i32, roleLen: i32) -> i32
    linker.func_wrap("env", "_auth_require_role", |_: i32, _: i32| -> i32 { 0 })?;
    // Auth: _auth_can(permPtr: i32, permLen: i32) -> i32
    linker.func_wrap("env", "_auth_can", |_: i32, _: i32| -> i32 { 0 })?;
    // Auth: _auth_has_any_role(rolesPtr: i32, rolesLen: i32) -> i32
    linker.func_wrap("env", "_auth_has_any_role", |_: i32, _: i32| -> i32 { 0 })?;

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
        "int64_to_string",
        |mut caller: Caller<'_, ()>, value: i64| -> i32 {
            let string_value = value.to_string();

            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    return allocate_string_in_memory(&memory, &mut caller, &string_value);
                }
            }

            0
        },
    )?;

    linker.func_wrap(
        "env",
        "float_to_string",
        |mut caller: Caller<'_, ()>, value: f64| -> i32 {
            let string_value = value.to_string();

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

    // env.string_to_int(str_ptr) -> i32. Parses the length-prefixed string at
    // str_ptr as a base-10 integer; returns 0 on parse failure or bad pointer.
    // Mirrors src/plugins/wasm_adapter.rs::string_to_int.
    linker.func_wrap(
        "env",
        "string_to_int",
        |mut caller: Caller<'_, ()>, str_ptr: i32| -> i32 {
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                return 0;
            };
            let data = memory.data(&caller);
            let ptr = str_ptr as usize;
            if ptr + 4 > data.len() {
                return 0;
            }
            let len = u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]])
                as usize;
            if ptr + 4 + len > data.len() {
                return 0;
            }
            std::str::from_utf8(&data[ptr + 4..ptr + 4 + len])
                .ok()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0)
        },
    )?;

    // env.string_to_float(str_ptr) -> f64. Parses the length-prefixed string at
    // str_ptr as f64; returns 0.0 on parse failure or bad pointer.
    // Mirrors src/plugins/wasm_adapter.rs::string_to_float.
    linker.func_wrap(
        "env",
        "string_to_float",
        |mut caller: Caller<'_, ()>, str_ptr: i32| -> f64 {
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                return 0.0;
            };
            let data = memory.data(&caller);
            let ptr = str_ptr as usize;
            if ptr + 4 > data.len() {
                return 0.0;
            }
            let len = u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]])
                as usize;
            if ptr + 4 + len > data.len() {
                return 0.0;
            }
            std::str::from_utf8(&data[ptr + 4..ptr + 4 + len])
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0)
        },
    )?;

    // env.string_matches(str_ptr, str_len, pattern_id) -> i32 (boolean 0/1)
    // pattern_id is a compile-time constant: email=0 url=1 uuid=2 phone=3
    //   date=4 integer=5 number=6 alphanumeric=7
    // Mirrors src/plugins/wasm_adapter.rs::string_matches so the runner
    // matches the plugin sandbox behavior.
    linker.func_wrap(
        "env",
        "string_matches",
        |mut caller: Caller<'_, ()>, str_ptr: i32, _str_len: i32, pattern_id: i32| -> i32 {
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                return 0;
            };
            let data = memory.data(&caller);
            let ptr = str_ptr as usize;
            if ptr + 4 > data.len() {
                return 0;
            }
            let len = u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]])
                as usize;
            if ptr + 4 + len > data.len() {
                return 0;
            }
            let s = String::from_utf8_lossy(&data[ptr + 4..ptr + 4 + len]).to_string();
            let matched = match pattern_id {
                0 => {
                    let p: Vec<&str> = s.splitn(2, '@').collect();
                    p.len() == 2 && !p[0].is_empty() && p[1].contains('.')
                }
                1 => s.starts_with("http://") || s.starts_with("https://"),
                2 => {
                    let b = s.as_bytes();
                    b.len() == 36
                        && b[8] == b'-'
                        && b[13] == b'-'
                        && b[18] == b'-'
                        && b[23] == b'-'
                        && b.iter().enumerate().all(|(i, &c)| {
                            if i == 8 || i == 13 || i == 18 || i == 23 {
                                c == b'-'
                            } else {
                                c.is_ascii_hexdigit()
                            }
                        })
                }
                3 => {
                    let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
                    d.len() >= 7 && d.len() <= 15
                }
                4 => {
                    let p: Vec<&str> = s.splitn(3, '-').collect();
                    p.len() == 3
                        && p[0].len() == 4
                        && p[1].len() == 2
                        && p[2].len() == 2
                        && p.iter().all(|x| x.chars().all(|c| c.is_ascii_digit()))
                }
                5 => !s.is_empty() && s.parse::<i64>().is_ok(),
                6 => !s.is_empty() && s.parse::<f64>().is_ok(),
                7 => !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()),
                _ => false,
            };
            if matched {
                1
            } else {
                0
            }
        },
    )?;

    // env.string_repeat(str_ptr, str_len, count) -> i32 pointer to new string.
    // Reads the length-prefixed input string, repeats it, and writes the
    // result back through allocate_string_in_memory so __heap_ptr stays in sync.
    linker.func_wrap(
        "env",
        "string_repeat",
        |mut caller: Caller<'_, ()>, str_ptr: i32, _str_len: i32, count: i32| -> i32 {
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                return 0;
            };
            let result = {
                let data = memory.data(&caller);
                let ptr = str_ptr as usize;
                if ptr + 4 > data.len() {
                    return 0;
                }
                let len =
                    u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]])
                        as usize;
                if ptr + 4 + len > data.len() {
                    return 0;
                }
                let s = match std::str::from_utf8(&data[ptr + 4..ptr + 4 + len]) {
                    Ok(s) => s.to_string(),
                    Err(_) => return 0,
                };
                let repeat_count = count.max(0) as usize;
                s.repeat(repeat_count)
            };
            allocate_string_in_memory(&memory, &mut caller, &result)
        },
    )?;
    // Dotted-name alias kept for any module that still imports `string.repeat`.
    linker.func_wrap(
        "env",
        "string.repeat",
        |mut caller: Caller<'_, ()>, str_ptr: i32, _str_len: i32, count: i32| -> i32 {
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                return 0;
            };
            let result = {
                let data = memory.data(&caller);
                let ptr = str_ptr as usize;
                if ptr + 4 > data.len() {
                    return 0;
                }
                let len =
                    u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]])
                        as usize;
                if ptr + 4 + len > data.len() {
                    return 0;
                }
                let s = match std::str::from_utf8(&data[ptr + 4..ptr + 4 + len]) {
                    Ok(s) => s.to_string(),
                    Err(_) => return 0,
                };
                let repeat_count = count.max(0) as usize;
                s.repeat(repeat_count)
            };
            allocate_string_in_memory(&memory, &mut caller, &result)
        },
    )?;

    // Endpoint test bridge stubs (no live server in runner — always return 0)
    linker.func_wrap(
        "env",
        "_test_http_request",
        |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 {
            0
        },
    )?;
    linker.func_wrap("env", "_test_response_status", |_: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "_test_response_body", |_: i32| -> i32 { 0 })?;

    // String trim functions
    linker.func_wrap(
        "env",
        "string_trim",
        |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
            // Extract string content first (immutable borrow)
            let trimmed_string = {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr as usize + 4 > data.len() {
                            return 0;
                        }
                        let len = u32::from_le_bytes([
                            data[ptr as usize],
                            data[ptr as usize + 1],
                            data[ptr as usize + 2],
                            data[ptr as usize + 3],
                        ]) as usize;
                        if ptr as usize + 4 + len > data.len() {
                            return 0;
                        }
                        match std::str::from_utf8(&data[ptr as usize + 4..ptr as usize + 4 + len]) {
                            Ok(s) => s.trim().to_string(),
                            Err(_) => return 0,
                        }
                    } else {
                        return 0;
                    }
                } else {
                    return 0;
                }
            };
            // Now allocate with mutable borrow
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    return allocate_string_in_memory(&memory, &mut caller, &trimmed_string);
                }
            }
            0
        },
    )?;

    linker.func_wrap(
        "env",
        "string_trim_start",
        |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
            let trimmed_string = {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr as usize + 4 > data.len() {
                            return 0;
                        }
                        let len = u32::from_le_bytes([
                            data[ptr as usize],
                            data[ptr as usize + 1],
                            data[ptr as usize + 2],
                            data[ptr as usize + 3],
                        ]) as usize;
                        if ptr as usize + 4 + len > data.len() {
                            return 0;
                        }
                        match std::str::from_utf8(&data[ptr as usize + 4..ptr as usize + 4 + len]) {
                            Ok(s) => s.trim_start().to_string(),
                            Err(_) => return 0,
                        }
                    } else {
                        return 0;
                    }
                } else {
                    return 0;
                }
            };
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    return allocate_string_in_memory(&memory, &mut caller, &trimmed_string);
                }
            }
            0
        },
    )?;

    linker.func_wrap(
        "env",
        "string_trim_end",
        |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
            let trimmed_string = {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr as usize + 4 > data.len() {
                            return 0;
                        }
                        let len = u32::from_le_bytes([
                            data[ptr as usize],
                            data[ptr as usize + 1],
                            data[ptr as usize + 2],
                            data[ptr as usize + 3],
                        ]) as usize;
                        if ptr as usize + 4 + len > data.len() {
                            return 0;
                        }
                        match std::str::from_utf8(&data[ptr as usize + 4..ptr as usize + 4 + len]) {
                            Ok(s) => s.trim_end().to_string(),
                            Err(_) => return 0,
                        }
                    } else {
                        return 0;
                    }
                } else {
                    return 0;
                }
            };
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    return allocate_string_in_memory(&memory, &mut caller, &trimmed_string);
                }
            }
            0
        },
    )?;

    // string_compare - Compare two strings, return 0 if equal, 1 if not (C/strcmp convention; codegen uses i32.eqz)
    linker.func_wrap(
        "env",
        "string_compare",
        |mut caller: Caller<'_, ()>, ptr1: i32, ptr2: i32| -> i32 {
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    let data = memory.data(&caller);

                    // Read first string
                    if ptr1 as usize + 4 > data.len() {
                        return 0;
                    }
                    let len1 = u32::from_le_bytes([
                        data[ptr1 as usize],
                        data[ptr1 as usize + 1],
                        data[ptr1 as usize + 2],
                        data[ptr1 as usize + 3],
                    ]) as usize;
                    let start1 = ptr1 as usize + 4;
                    let string1 = std::str::from_utf8(&data[start1..start1 + len1]).unwrap_or("");

                    // Read second string
                    if ptr2 as usize + 4 > data.len() {
                        return 0;
                    }
                    let len2 = u32::from_le_bytes([
                        data[ptr2 as usize],
                        data[ptr2 as usize + 1],
                        data[ptr2 as usize + 2],
                        data[ptr2 as usize + 3],
                    ]) as usize;
                    let start2 = ptr2 as usize + 4;
                    let string2 = std::str::from_utf8(&data[start2..start2 + len2]).unwrap_or("");

                    return if string1 == string2 { 0 } else { 1 };
                }
            }
            0
        },
    )?;

    // string_replace - Replace all occurrences of search with replace in string
    linker.func_wrap(
        "env",
        "string_replace",
        |mut caller: Caller<'_, ()>, string_ptr: i32, search_ptr: i32, replace_ptr: i32| -> i32 {
            if let Some(memory) = caller.get_export("memory") {
                if let Some(memory) = memory.into_memory() {
                    let data = memory.data(&caller);

                    // Read source string
                    if string_ptr as usize + 4 > data.len() {
                        return 0;
                    }
                    let len1 = u32::from_le_bytes([
                        data[string_ptr as usize],
                        data[string_ptr as usize + 1],
                        data[string_ptr as usize + 2],
                        data[string_ptr as usize + 3],
                    ]) as usize;
                    let start1 = string_ptr as usize + 4;
                    let source = std::str::from_utf8(&data[start1..start1 + len1])
                        .unwrap_or("")
                        .to_string();

                    // Read search string
                    if search_ptr as usize + 4 > data.len() {
                        return 0;
                    }
                    let len2 = u32::from_le_bytes([
                        data[search_ptr as usize],
                        data[search_ptr as usize + 1],
                        data[search_ptr as usize + 2],
                        data[search_ptr as usize + 3],
                    ]) as usize;
                    let start2 = search_ptr as usize + 4;
                    let search = std::str::from_utf8(&data[start2..start2 + len2])
                        .unwrap_or("")
                        .to_string();

                    // Read replace string
                    if replace_ptr as usize + 4 > data.len() {
                        return 0;
                    }
                    let len3 = u32::from_le_bytes([
                        data[replace_ptr as usize],
                        data[replace_ptr as usize + 1],
                        data[replace_ptr as usize + 2],
                        data[replace_ptr as usize + 3],
                    ]) as usize;
                    let start3 = replace_ptr as usize + 4;
                    let replace = std::str::from_utf8(&data[start3..start3 + len3])
                        .unwrap_or("")
                        .to_string();

                    // Perform replacement
                    let result = source.replace(&search, &replace);

                    // Allocate and write result
                    return allocate_string_in_memory(&memory, &mut caller, &result);
                }
            }
            0
        },
    )?;

    // Add memory management functions.
    //
    // mem_alloc shares heap state with the WASM-side bump allocator
    // (`__malloc` / `list.allocate`) through the `__heap_ptr` global. Without
    // an initial sync, the host's `NEXT_ALLOCATION_OFFSET` could lag behind
    // `__heap_ptr` after the WASM side advances it via `__malloc`, and the
    // next host `mem_alloc` would return an offset that overlaps with a
    // previously WASM-allocated region. Observed as: JSON converter
    // boxes (via `__malloc`) being silently overwritten by subsequent
    // `mem_alloc` of an object literal's outer Any box, corrupting tag bytes.
    // Mirror the read-then-take-max pattern from `allocate_string_in_memory`.
    linker.func_wrap(
        "memory_runtime",
        "mem_alloc",
        |mut caller: Caller<'_, ()>, _type_id: i32, size: i32| -> i32 {
            let wasm_heap =
                if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
                    heap_global.get(&mut caller).i32().unwrap_or(0) as usize
                } else {
                    0
                };
            let mut next_offset = NEXT_ALLOCATION_OFFSET.lock().unwrap();
            if wasm_heap > *next_offset {
                *next_offset = wasm_heap;
            }
            let offset = *next_offset;
            let aligned_size = ((size as usize) + 7) & !7;
            let aligned_end = offset + aligned_size;
            *next_offset = aligned_end;
            drop(next_offset);
            // Keep WASM __heap_ptr in sync so WASM-side malloc doesn't
            // allocate over host-allocated objects.
            if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
                let current = heap_global.get(&mut caller).i32().unwrap_or(0) as usize;
                if aligned_end > current {
                    heap_global
                        .set(&mut caller, wasmtime::Val::I32(aligned_end as i32))
                        .ok();
                }
            }
            offset as i32
        },
    )?;

    linker.func_wrap("memory_runtime", "mem_retain", |_ptr: i32| {
        // Mock retain - does nothing
    })?;

    linker.func_wrap("memory_runtime", "mem_release", |_ptr: i32| {
        // Mock release - does nothing (use scope-based release instead)
    })?;

    // Scope-based memory management for arena allocation.
    //
    // The mark must capture BOTH the host-side `NEXT_ALLOCATION_OFFSET` and
    // the WASM-side `__heap_ptr` global, because the WASM-side bump allocator
    // advances `__heap_ptr` independently (e.g. via `__malloc` / `list.allocate`).
    // Resetting only the host offset on pop leaves WASM-side allocations
    // accumulating until the linear-memory cap is reached
    // (COMPILER-NO-FREE-EXPORT-LEAKS-WASM-MEMORY).
    linker.func_wrap(
        "memory_runtime",
        "mem_scope_push",
        |mut caller: Caller<'_, ()>| {
            let host_offset = *NEXT_ALLOCATION_OFFSET.lock().unwrap();
            let wasm_heap =
                if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
                    heap_global.get(&mut caller).i32().unwrap_or(0) as usize
                } else {
                    host_offset
                };
            // Save the high-water mark of both to ensure correctness even when
            // they have temporarily drifted between syncs.
            let mark = host_offset.max(wasm_heap);
            SCOPE_MARKS.lock().unwrap().push(mark);
        },
    )?;

    linker.func_wrap(
        "memory_runtime",
        "mem_scope_pop",
        |mut caller: Caller<'_, ()>| {
            if let Some(mark) = SCOPE_MARKS.lock().unwrap().pop() {
                *NEXT_ALLOCATION_OFFSET.lock().unwrap() = mark;
                if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
                    heap_global
                        .set(&mut caller, wasmtime::Val::I32(mark as i32))
                        .ok();
                }
            }
        },
    )?;

    // Host bridge arena scope push/pop — used by the HIR dual-accumulator rewrite
    // (COMPILER-MEM-ALLOC-NO-GROW-RECURRENCE).  The runner uses the same
    // NEXT_ALLOCATION_OFFSET / SCOPE_MARKS infrastructure as mem_scope_push/pop.
    //
    // _arena_scope_push() -> i32 : saves current alloc offset, returns depth handle.
    // _arena_scope_pop(handle: i32) -> void : rewinds to the saved offset at `handle`,
    //   reclaiming all allocations made since the matching push.
    linker.func_wrap(
        "env",
        "_arena_scope_push",
        |mut caller: Caller<'_, ()>| -> i32 {
            let host_offset = *NEXT_ALLOCATION_OFFSET.lock().unwrap();
            let wasm_heap =
                if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
                    heap_global.get(&mut caller).i32().unwrap_or(0) as usize
                } else {
                    host_offset
                };
            let mark = host_offset.max(wasm_heap);
            let mut marks = SCOPE_MARKS.lock().unwrap();
            marks.push(mark);
            marks.len() as i32
        },
    )?;

    linker.func_wrap(
        "env",
        "_arena_scope_pop",
        |mut caller: Caller<'_, ()>, handle: i32| {
            if handle <= 0 {
                return;
            }
            let target_depth = (handle as usize).saturating_sub(1);
            // Pop marks until we reach target_depth, saving the earliest
            // saved offset (the one we will reset to).
            let reset_to = {
                let mut marks = SCOPE_MARKS.lock().unwrap();
                let mut earliest: Option<usize> = None;
                while marks.len() > target_depth {
                    if let Some(mark) = marks.pop() {
                        earliest = Some(mark);
                    }
                }
                earliest
            };
            if let Some(mark) = reset_to {
                *NEXT_ALLOCATION_OFFSET.lock().unwrap() = mark;
                if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
                    heap_global
                        .set(&mut caller, wasmtime::Val::I32(mark as i32))
                        .ok();
                }
            }
        },
    )?;

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

            // Read current heap pointer from WASM global 0 (__heap_ptr), NOT from linear memory.
            // Also sync with host-side NEXT_ALLOCATION_OFFSET to avoid overlapping allocations.
            let current_heap =
                if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
                    heap_global.get(&mut caller).i32().unwrap_or(0) as usize
                } else {
                    0
                };

            let mut next_offset_guard = NEXT_ALLOCATION_OFFSET.lock().unwrap();
            let sync_heap = (*next_offset_guard).max(current_heap);

            let list_ptr = sync_heap;
            let mut next_ptr = list_ptr + list_size;

            // Allocate string pointers
            let mut string_ptrs = Vec::with_capacity(num_parts);
            for part in &parts {
                let part_len = part.len();
                let str_allocation = 4 + part_len; // 4 bytes for length + content
                string_ptrs.push(next_ptr as u32);
                next_ptr += str_allocation;
            }

            // Align and update both allocation trackers
            let new_heap_ptr = ((next_ptr + 7) & !7) as u32;
            *next_offset_guard = new_heap_ptr as usize;
            drop(next_offset_guard);

            // Grow memory if needed
            let current_bytes = memory.size(&caller) as usize * 65536;
            if (new_heap_ptr as usize) > current_bytes {
                let needed_pages = (new_heap_ptr as usize - current_bytes).div_ceil(65536) as u64;
                let _ = memory.grow(&mut caller, needed_pages);
            }

            // Update WASM global __heap_ptr
            if let Some(Extern::Global(heap_global)) = caller.get_export("__heap_ptr") {
                heap_global
                    .set(&mut caller, wasmtime::Val::I32(new_heap_ptr as i32))
                    .ok();
            }

            let data_mut = memory.data_mut(&mut caller);

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

            i32::from_le_bytes([
                element_bytes[0],
                element_bytes[1],
                element_bytes[2],
                element_bytes[3],
            ])
        },
    )?;

    // Add list.push_f64: pushes an f64 element to a list
    // Parameters: (array_ptr: i32, value: f64) -> i32 (new array pointer or same)
    // List memory layout: [length(4), capacity(4), type_id(4), flags(4), data...]
    linker.func_wrap(
        "env",
        "list.push_f64",
        |mut caller: Caller<'_, ()>, array_ptr: i32, value: f64| -> i32 {
            let memory = if let Some(Extern::Memory(mem)) = caller.get_export("memory") {
                mem
            } else {
                eprintln!("[list.push_f64: no memory export]");
                return array_ptr;
            };

            let data = memory.data(&caller);
            let ptr = array_ptr as usize;

            if ptr + 16 > data.len() {
                eprintln!("[list.push_f64: invalid pointer {}]", array_ptr);
                return array_ptr;
            }

            // Read length from offset 0
            let length =
                u32::from_le_bytes([data[ptr], data[ptr + 1], data[ptr + 2], data[ptr + 3]])
                    as usize;
            // Read capacity from offset 4
            let _capacity =
                u32::from_le_bytes([data[ptr + 4], data[ptr + 5], data[ptr + 6], data[ptr + 7]])
                    as usize;

            // Data starts at offset 16, each f64 element is 8 bytes
            let data_offset = ptr + 16;
            let element_offset = data_offset + length * 8;

            if element_offset + 8 > data.len() {
                eprintln!("[list.push_f64: out of memory bounds]");
                return array_ptr;
            }

            // Write the f64 value
            let data_mut = memory.data_mut(&mut caller);
            let bytes = value.to_le_bytes();
            data_mut[element_offset..element_offset + 8].copy_from_slice(&bytes);

            // Increment length
            let new_length = (length + 1) as u32;
            data_mut[ptr..ptr + 4].copy_from_slice(&new_length.to_le_bytes());

            array_ptr
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
        // Provide a result buffer sized to the function's declared result
        // count. The `start:` block produces a `(result i32)` whenever its
        // last statement is an expression that yields a value (e.g.
        // `d.show()` returning `any` lowers to i32). Calling with an empty
        // results slice trips wasmtime's "expected N results, got 0"
        // check. Discard whatever value comes back.
        let ty = func.ty(&store);
        let mut results: Vec<wasmtime::Val> = ty
            .results()
            .map(|t| match t {
                wasmtime::ValType::I32 => wasmtime::Val::I32(0),
                wasmtime::ValType::I64 => wasmtime::Val::I64(0),
                wasmtime::ValType::F32 => wasmtime::Val::F32(0),
                wasmtime::ValType::F64 => wasmtime::Val::F64(0),
                _ => wasmtime::Val::I32(0),
            })
            .collect();
        func.call(&mut store, &[], &mut results)?;
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

    // If the module exports _run_tests, invoke it and report results.
    if let Some(func) = instance.get_func(&mut store, "_run_tests") {
        println!("🧪 Running tests (_run_tests)...");
        println!("--- Test Output ---");
        let mut results = vec![wasmtime::Val::I32(0)];
        func.call(&mut store, &[], &mut results)?;
        println!("--- End Test Output ---");
        if let Some(wasmtime::Val::I32(failures)) = results.first() {
            if *failures == 0 {
                println!("✅ All tests passed!");
            } else {
                println!("❌ {} test(s) failed.", failures);
                return Err(anyhow::anyhow!("{} test(s) failed", failures).into());
            }
        }
    }

    Ok(())
}
