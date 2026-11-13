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
const HEAP_START: usize = 1048576;
static NEXT_ALLOCATION_OFFSET: Mutex<usize> = Mutex::new(HEAP_START);

// Scope marks for arena-style memory management
// When entering a scope (loop, function), push current offset
// When exiting scope, pop and reset - instant deallocation of all scope allocations
static SCOPE_MARKS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Tasks handed over by `_async_fire_slot` (`background` / `later`), as
/// `(slot, env)`: run in order once `start` has returned (HOST_BRIDGE.md
/// §Async).
static ASYNC_TASKS: Mutex<std::collections::VecDeque<(i32, i32)>> =
    Mutex::new(std::collections::VecDeque::new());

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
            eprintln!("⚠️  WARNING: memory.grow failed for {needed_pages} pages");
            return 0;
        }
    }

    // Step 3: Write string data
    let data = memory.data_mut(&mut *caller);
    if offset + total_size > data.len() {
        eprintln!("⚠️  WARNING: Not enough WASM memory for string allocation. Offset: {offset}, Size: {total_size}, Memory: {memory_len}", memory_len = data.len());
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

/// The bytes at `ptr..ptr + len` in the module's memory; traps on a missing
/// memory export or an out-of-bounds range.
fn read_bytes(caller: &mut Caller<'_, ()>, ptr: i32, len: i32) -> Result<Vec<u8>> {
    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        return Err(wasmtime::Error::msg("module exports no memory"));
    };
    let start = ptr as u32 as usize;
    let end = start + len as u32 as usize;
    memory
        .data(&*caller)
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| wasmtime::Error::msg(format!("string {start}..{end} is out of bounds")))
}

/// The UTF-8 text at `ptr..ptr + len` in the module's memory; traps like
/// `read_bytes`, and on invalid UTF-8.
fn read_utf8(caller: &mut Caller<'_, ()>, ptr: i32, len: i32) -> Result<String> {
    String::from_utf8(read_bytes(caller, ptr, len)?)
        .map_err(|e| wasmtime::Error::msg(e.to_string()))
}

/// Write `bytes` to stdout unchanged. A string is a byte sequence, so a byte
/// that is part of a multi-byte character is written as that byte.
fn write_stdout(bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    std::io::stdout()
        .lock()
        .write_all(bytes)
        .map_err(|e| wasmtime::Error::msg(format!("writing to stdout failed: {e}")))
}

/// Call an exported function; on a trap print the whole error chain (the
/// trap message, e.g. an unavailable host function, sits under the WASM
/// backtrace) and exit 1.
fn call_or_exit(func: &wasmtime::Func, store: &mut Store<()>, results: &mut [wasmtime::Val]) {
    if let Err(e) = func.call(&mut *store, &[], results) {
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        eprintln!("Error: {e:?}");
        std::process::exit(1);
    }
}

/// The task function a `background` / `later` slot names: entry `slot` of
/// the module's `__indirect_function_table`, `(env: i32) -> i32`.
fn async_task_function(
    caller: &mut Caller<'_, ()>,
    slot: i32,
) -> Result<wasmtime::TypedFunc<i32, i32>> {
    let table = caller
        .get_export("__indirect_function_table")
        .and_then(Extern::into_table)
        .ok_or_else(|| wasmtime::Error::msg("module exports no __indirect_function_table"))?;
    let func = u64::try_from(slot)
        .ok()
        .and_then(|index| table.get(&mut *caller, index))
        .and_then(|entry| entry.as_func().flatten().copied())
        .ok_or_else(|| wasmtime::Error::msg(format!("async task slot {slot} holds no function")))?;
    func.typed::<i32, i32>(&*caller)
}

/// Run the tasks `start` handed over, in order, including those the tasks
/// themselves start. A task that traps is reported on stderr and the others
/// still run: its failure does not reach the code that started it.
fn run_async_tasks(instance: &wasmtime::Instance, store: &mut Store<()>) {
    loop {
        let next = ASYNC_TASKS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front();
        let Some((slot, env)) = next else {
            break;
        };
        let task = instance
            .get_table(&mut *store, "__indirect_function_table")
            .and_then(|table| {
                u64::try_from(slot)
                    .ok()
                    .and_then(|i| table.get(&mut *store, i))
            })
            .and_then(|entry| entry.as_func().flatten().copied())
            .ok_or_else(|| {
                wasmtime::Error::msg(format!("async task slot {slot} holds no function"))
            })
            .and_then(|func| func.typed::<i32, i32>(&*store));
        if let Err(e) = task.and_then(|task| task.call(&mut *store, env)) {
            eprintln!("Error: background task failed: {e:?}");
        }
    }
}

/// Trap raised when a program calls a host function this runner cannot
/// provide.
fn unavailable(name: &str) -> wasmtime::Error {
    wasmtime::Error::msg(format!(
        "host function `{name}` is not available in wasmtime_runner; run this program on clean-server (or another host that provides it)"
    ))
}

/// `text.split(delimiter)` on the bytes of the text. An empty delimiter
/// gives one part per character, a character being a byte as `.length()`,
/// `.charAt(i)` and `iterate` count it (stdlib-reference.md §4); an empty
/// text then gives no parts.
fn split_bytes<'a>(text: &'a [u8], delimiter: &[u8]) -> Vec<&'a [u8]> {
    if delimiter.is_empty() {
        return text.chunks(1).collect();
    }
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + delimiter.len() <= text.len() {
        if text[i..].starts_with(delimiter) {
            parts.push(&text[start..i]);
            i += delimiter.len();
            start = i;
        } else {
            i += 1;
        }
    }
    parts.push(&text[start..]);
    parts
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let usage = format!(
        "Usage: {} <wasm_file>\n\nRuns a compiled Clean Language module: its start function, then its \
         tests: block when the module has one.\nProgram output goes to stdout between the \
         `--- Output ---` and `--- End Output ---` lines; runner messages go to stderr.\n\
         Exits 1 when the module traps or a test fails.\n\nOptions:\n  -h, --help     Print this help\n  -V, --version  Print the version",
        args.first().map(String::as_str).unwrap_or("wasmtime_runner")
    );
    match args.get(1).map(String::as_str) {
        Some("-h" | "--help") if args.len() == 2 => {
            println!("{usage}");
            return Ok(());
        }
        Some("-V" | "--version") if args.len() == 2 => {
            println!("wasmtime_runner {}", clean_language_compiler::VERSION);
            return Ok(());
        }
        _ if args.len() != 2 => {
            eprintln!("{usage}");
            std::process::exit(1);
        }
        _ => {}
    }

    // Runner messages go to stderr so stdout carries only the program's
    // output and the `--- Output ---` markers around it.
    let wasm_file = &args[1];
    eprintln!("🚀 Loading WebAssembly file: {wasm_file}");

    // Read the WASM file
    let wasm_bytes =
        fs::read(wasm_file).map_err(|e| anyhow::anyhow!("cannot read '{wasm_file}': {e}"))?;
    eprintln!("📦 File size: {len} bytes", len = wasm_bytes.len());

    // Create engine and store using minimal (sync) configuration for wasmtime_runner
    let engine = clean_language_compiler::runtime::wasmtime_config::CleanWasmtimeConfig::create_minimal_engine()
        .map_err(|e| anyhow::anyhow!("Failed to create wasmtime engine: {e}"))?;
    let mut store = Store::new(&engine, ());

    // Create module
    let module = Module::new(&engine, &wasm_bytes)?;

    // Create linker and add imports
    let mut linker = Linker::new(&engine);

    // print(ptr, len) / printl(ptr, len): the bytes at ptr..ptr+len written
    // unchanged, printl adding a newline. A range outside the module's
    // memory traps.
    linker.func_wrap(
        "env",
        "print",
        |mut caller: Caller<'_, ()>, ptr: i32, len: i32| -> Result<()> {
            write_stdout(&read_bytes(&mut caller, ptr, len)?)
        },
    )?;
    linker.func_wrap(
        "env",
        "printl",
        |mut caller: Caller<'_, ()>, ptr: i32, len: i32| -> Result<()> {
            let mut bytes = read_bytes(&mut caller, ptr, len)?;
            bytes.push(b'\n');
            write_stdout(&bytes)
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

    // File bridges work on the local file system. `file_read` and the host
    // functions this standalone runner does not provide (HTTP, console
    // input, server request context, sessions, auth, UI, async, endpoint
    // tests) trap with a clear message when called, so a program that needs
    // them fails loudly instead of running on fake results.
    linker.func_wrap(
        "env",
        "file_write",
        |mut caller: Caller<'_, ()>,
         path_ptr: i32,
         path_len: i32,
         ptr: i32,
         len: i32|
         -> Result<i32> {
            let path = read_utf8(&mut caller, path_ptr, path_len)?;
            let content = read_utf8(&mut caller, ptr, len)?;
            Ok(i32::from(fs::write(path, content).is_ok()))
        },
    )?;
    linker.func_wrap(
        "env",
        "file_read",
        |_: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("file_read")) },
    )?;
    linker.func_wrap(
        "env",
        "file_exists",
        |mut caller: Caller<'_, ()>, ptr: i32, len: i32| -> Result<i32> {
            let path = read_utf8(&mut caller, ptr, len)?;
            Ok(i32::from(std::path::Path::new(&path).exists()))
        },
    )?;
    linker.func_wrap(
        "env",
        "file_delete",
        |mut caller: Caller<'_, ()>, ptr: i32, len: i32| -> Result<i32> {
            let path = read_utf8(&mut caller, ptr, len)?;
            Ok(i32::from(fs::remove_file(path).is_ok()))
        },
    )?;
    linker.func_wrap(
        "env",
        "file_append",
        |mut caller: Caller<'_, ()>,
         path_ptr: i32,
         path_len: i32,
         ptr: i32,
         len: i32|
         -> Result<i32> {
            use std::io::Write as _;
            let path = read_utf8(&mut caller, path_ptr, path_len)?;
            let content = read_utf8(&mut caller, ptr, len)?;
            let appended = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(content.as_bytes()));
            Ok(i32::from(appended.is_ok()))
        },
    )?;
    linker.func_wrap("env", "http_get", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("http_get"))
    })?;
    linker.func_wrap(
        "env",
        "http_post",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("http_post")) },
    )?;
    linker.func_wrap(
        "env",
        "http_put",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("http_put")) },
    )?;
    linker.func_wrap("env", "http_delete", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("http_delete"))
    })?;
    linker.func_wrap(
        "env",
        "http_patch",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("http_patch")) },
    )?;
    linker.func_wrap("env", "http_head", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("http_head"))
    })?;
    linker.func_wrap("env", "http_options", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("http_options"))
    })?;

    // JSON bridges (stdlib-reference §8): the real implementation, so JSON
    // programs run end to end without a clean-server host.
    register_json_bridges(&mut linker)?;

    // Add additional required imports
    linker.func_wrap("env", "input", |_: i32| -> Result<i32> {
        Err(unavailable("input"))
    })?;
    linker.func_wrap("env", "input_integer", |_: i32| -> Result<i32> {
        Err(unavailable("input_integer"))
    })?;
    linker.func_wrap("env", "input_float", |_: i32| -> Result<f64> {
        Err(unavailable("input_float"))
    })?;
    linker.func_wrap("env", "input_yesno", |_: i32| -> Result<i32> {
        Err(unavailable("input_yesno"))
    })?;
    linker.func_wrap(
        "env",
        "input_range",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("input_range")) },
    )?;

    // HTTP imports with headers
    linker.func_wrap(
        "env",
        "http_get_with_headers",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> {
            Err(unavailable("http_get_with_headers"))
        },
    )?;
    linker.func_wrap(
        "env",
        "http_post_with_headers",
        |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> Result<i32> {
            Err(unavailable("http_post_with_headers"))
        },
    )?;
    linker.func_wrap(
        "env",
        "http_post_json",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("http_post_json")) },
    )?;
    linker.func_wrap(
        "env",
        "http_put_json",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("http_put_json")) },
    )?;
    linker.func_wrap(
        "env",
        "http_patch_json",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("http_patch_json")) },
    )?;
    linker.func_wrap(
        "env",
        "http_post_form",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("http_post_form")) },
    )?;
    linker.func_wrap(
        "env",
        "http_set_user_agent",
        |_: i32, _: i32| -> Result<()> { Err(unavailable("http_set_user_agent")) },
    )?;
    linker.func_wrap("env", "http_set_timeout", |_: i32| -> Result<()> {
        Err(unavailable("http_set_timeout"))
    })?;
    linker.func_wrap("env", "http_set_max_redirects", |_: i32| -> Result<()> {
        Err(unavailable("http_set_max_redirects"))
    })?;
    linker.func_wrap("env", "http_enable_cookies", |_: i32| -> Result<()> {
        Err(unavailable("http_enable_cookies"))
    })?;
    linker.func_wrap("env", "http_get_response_code", || -> Result<i32> {
        Err(unavailable("http_get_response_code"))
    })?;
    linker.func_wrap("env", "http_get_response_headers", || -> Result<i32> {
        Err(unavailable("http_get_response_headers"))
    })?;
    linker.func_wrap("env", "http_encode_url", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("http_encode_url"))
    })?;
    linker.func_wrap("env", "http_decode_url", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("http_decode_url"))
    })?;
    linker.func_wrap("env", "http_build_query", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("http_build_query"))
    })?;

    linker.func_wrap(
        "env",
        "_http_route",
        |_: i32, _: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("_http_route")) },
    )?;
    linker.func_wrap("env", "_http_listen", |_: i32| -> Result<i32> {
        Err(unavailable("_http_listen"))
    })?;
    linker.func_wrap("env", "_dev_snapshot", || -> Result<i32> {
        Err(unavailable("_dev_snapshot"))
    })?;
    // `_state_reset_all` resets the host allocator between handler runs
    // (as clean-server does); host allocations then restart at the larger
    // of the heap start and the module's `__heap_ptr`.
    linker.func_wrap("env", "_state_reset_all", || {
        *NEXT_ALLOCATION_OFFSET.lock().unwrap() = HEAP_START;
        SCOPE_MARKS.lock().unwrap().clear();
    })?;
    // Per the function registry this is a no-op for a bump allocator.
    linker.func_wrap("env", "_state_reset_named", |_: i32| {})?;
    // frame.ui bridges: linked so re-exported wrappers instantiate; calling
    // one needs the browser/server host.
    linker.func_wrap(
        "env",
        "_ui_render_page",
        |_: i32, _: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("_ui_render_page")) },
    )?;
    linker.func_wrap("env", "_html_escape", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("_html_escape"))
    })?;
    // Heap-probe stubs for --emit-heap-probes / --emit-bridge-probes instrumentation.
    // Real impl lives in the errors-dashboard capture harness; standalone runner
    // prints callsite_id + resulting heap ptr when CLN_WASMTIME_PROBE=1 is set,
    // so leak investigations can eyeball per-callsite growth without cluttering
    // normal test output. Silent no-op by default.
    linker.func_wrap("env", "_probe_ptr", |id: i32, ptr: i32| {
        if std::env::var("CLN_WASMTIME_PROBE").is_ok() {
            eprintln!("[probe] id={id} ptr={ptr}");
        }
    })?;
    linker.func_wrap("env", "_probe_ptr_dump", || -> i32 { 0 })?;
    linker.func_wrap("env", "_probe_ptr_reset", || {})?;

    // server.sleep(milliseconds)
    linker.func_wrap("env", "_server_sleep", |ms: i64| {
        std::thread::sleep(std::time::Duration::from_millis(ms.max(0) as u64));
    })?;

    // background / later (HOST_BRIDGE.md §Async): a task is queued and run
    // after `start` returns; awaiting it runs it now (a task runs once — the
    // task function itself returns the stored result on a second call).
    linker.func_wrap("env", "_async_fire_slot", |slot: i32, env: i32| {
        ASYNC_TASKS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back((slot, env));
    })?;
    linker.func_wrap(
        "env",
        "_async_await_slot",
        |mut caller: Caller<'_, ()>, slot: i32, env: i32| -> Result<i32> {
            let task = async_task_function(&mut caller, slot)?;
            task.call(&mut caller, env)
        },
    )?;

    linker.func_wrap("env", "_req_param", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("_req_param"))
    })?;
    linker.func_wrap("env", "_req_query", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("_req_query"))
    })?;
    linker.func_wrap("env", "_req_body", || -> Result<i32> {
        Err(unavailable("_req_body"))
    })?;
    linker.func_wrap("env", "_req_body_bytes", || -> Result<i32> {
        Err(unavailable("_req_body_bytes"))
    })?;
    linker.func_wrap(
        "env",
        "_fs_write_bytes",
        |_: i32, _: i32, _: i32| -> Result<i32> { Err(unavailable("_fs_write_bytes")) },
    )?;
    linker.func_wrap("env", "_crypto_sha256_bytes", |_: i32| -> Result<i32> {
        Err(unavailable("_crypto_sha256_bytes"))
    })?;
    linker.func_wrap("env", "_req_header", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("_req_header"))
    })?;
    linker.func_wrap("env", "_req_method", || -> Result<i32> {
        Err(unavailable("_req_method"))
    })?;
    linker.func_wrap("env", "_req_path", || -> Result<i32> {
        Err(unavailable("_req_path"))
    })?;
    linker.func_wrap("env", "_req_cookie", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("_req_cookie"))
    })?;
    linker.func_wrap(
        "env",
        "_http_route_protected",
        |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> Result<i32> {
            Err(unavailable("_http_route_protected"))
        },
    )?;
    // Session: _session_store(user_id: i32, rolePtr: i32, roleLen: i32, claimsPtr: i32, claimsLen: i32) -> i32
    linker.func_wrap(
        "env",
        "_session_store",
        |_: i32, _: i32, _: i32, _: i32, _: i32| -> Result<i32> {
            Err(unavailable("_session_store"))
        },
    )?;
    // Session: _session_get() -> i32
    linker.func_wrap("env", "_session_get", || -> Result<i32> {
        Err(unavailable("_session_get"))
    })?;
    // Session: _session_delete() -> i32
    linker.func_wrap("env", "_session_delete", || -> Result<i32> {
        Err(unavailable("_session_delete"))
    })?;
    // Session: _http_set_cookie(cookiePtr: i32, cookieLen: i32) -> i32
    linker.func_wrap("env", "_http_set_cookie", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("_http_set_cookie"))
    })?;
    // Auth: _auth_get_session() -> i32
    linker.func_wrap("env", "_auth_get_session", || -> Result<i32> {
        Err(unavailable("_auth_get_session"))
    })?;
    // Auth: _auth_require_auth() -> i32
    linker.func_wrap("env", "_auth_require_auth", || -> Result<i32> {
        Err(unavailable("_auth_require_auth"))
    })?;
    // Auth: _auth_require_role(rolePtr: i32, roleLen: i32) -> i32
    linker.func_wrap(
        "env",
        "_auth_require_role",
        |_: i32, _: i32| -> Result<i32> { Err(unavailable("_auth_require_role")) },
    )?;
    // Auth: _auth_can(permPtr: i32, permLen: i32) -> i32
    linker.func_wrap("env", "_auth_can", |_: i32, _: i32| -> Result<i32> {
        Err(unavailable("_auth_can"))
    })?;
    // Auth: _auth_has_any_role(rolesPtr: i32, rolesLen: i32) -> i32
    linker.func_wrap(
        "env",
        "_auth_has_any_role",
        |_: i32, _: i32| -> Result<i32> { Err(unavailable("_auth_has_any_role")) },
    )?;

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
    linker.func_wrap("env", "math_trunc", |x: f64| -> f64 { x.trunc() })?;
    linker.func_wrap("env", "math_min", |a: f64, b: f64| -> f64 { a.min(b) })?;
    linker.func_wrap("env", "math_max", |a: f64, b: f64| -> f64 { a.max(b) })?;
    linker.func_wrap("env", "math_sign", |x: f64| -> f64 {
        if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            x
        }
    })?;
    linker.func_wrap("env", "math_pi", || -> f64 { std::f64::consts::PI })?;
    linker.func_wrap("env", "math_e", || -> f64 { std::f64::consts::E })?;
    // Uniform in [0.0, 1.0) from the thread-local CSPRNG.
    linker.func_wrap("env", "math_random", || -> f64 { rand::random::<f64>() })?;
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

    // Endpoint tests need a live server.
    linker.func_wrap(
        "env",
        "_test_http_request",
        |_: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32,
         _: i32|
         -> Result<i32> { Err(unavailable("_test_http_request")) },
    )?;
    linker.func_wrap("env", "_test_response_status", |_: i32| -> Result<i32> {
        Err(unavailable("_test_response_status"))
    })?;
    linker.func_wrap("env", "_test_response_body", |_: i32| -> Result<i32> {
        Err(unavailable("_test_response_body"))
    })?;

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

    // string_compare - Lexicographic comparison: -1 if a < b, 0 if equal, 1 if a > b
    // (function-registry.toml; codegen uses i32.eqz for equality and compares
    // against 0 for ordering)
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

                    return match string1.cmp(string2) {
                        std::cmp::Ordering::Less => -1,
                        std::cmp::Ordering::Equal => 0,
                        std::cmp::Ordering::Greater => 1,
                    };
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

    // Method-style conversions imported by name
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
    linker.func_wrap("env", "integer.length", |_: i32| -> Result<i32> {
        Err(unavailable("integer.length"))
    })?;

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
    linker.func_wrap("env", "number.length", |_: f64| -> Result<i32> {
        Err(unavailable("number.length"))
    })?;

    linker.func_wrap("env", "string.toString", |value: i32| -> i32 { value })?;
    linker.func_wrap("env", "string.toInteger", |_: i32| -> Result<i32> {
        Err(unavailable("string.toInteger"))
    })?;
    linker.func_wrap("env", "string.toNumber", |_: i32| -> Result<f64> {
        Err(unavailable("string.toNumber"))
    })?;
    linker.func_wrap("env", "string.toBoolean", |_: i32| -> Result<i32> {
        Err(unavailable("string.toBoolean"))
    })?;
    linker.func_wrap("env", "string.length", |_: i32| -> Result<i32> {
        Err(unavailable("string.length"))
    })?;

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
    linker.func_wrap("env", "boolean.length", |_: i32| -> Result<i32> {
        Err(unavailable("boolean.length"))
    })?;

    linker.func_wrap("env", "string.toUpperCase", |_: i32| -> Result<i32> {
        Err(unavailable("string.toUpperCase"))
    })?;
    linker.func_wrap("env", "string.toLowerCase", |_: i32| -> Result<i32> {
        Err(unavailable("string.toLowerCase"))
    })?;
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

            let parts = split_bytes(string_content.as_bytes(), delimiter.as_bytes());
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
                let part_bytes: &[u8] = part;
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
                let part_bytes: &[u8] = part;

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
                data[str_ptr + 4..str_ptr + 4 + str_len].to_vec()
            } else {
                Vec::new()
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
                data[delim_ptr + 4..delim_ptr + 4 + delim_len].to_vec()
            } else {
                Vec::new()
            };

            eprintln!(
                "🔍 string.split: splitting '{}' by '{}'",
                String::from_utf8_lossy(&original_str),
                String::from_utf8_lossy(&delimiter)
            );

            let parts = split_bytes(&original_str, &delimiter);
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
                let part_bytes: &[u8] = part;

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
                eprintln!("[array_get: no memory export]");
                return 0; // Return 0 on error
            };

            let data = memory.data(&caller);
            let ptr_usize = array_ptr as usize;

            // Check bounds for reading header
            if ptr_usize + 17 > data.len() {
                eprintln!("[array_get: invalid array pointer {}]", array_ptr);
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
                eprintln!(
                    "[array_get: index {} out of bounds for array of length {}]",
                    index, length
                );
                return 0; // Return 0 for out-of-bounds access
            }

            // Each element is 4 bytes (i32), calculate element offset
            let element_offset = data_offset + (index as usize * 4);

            if element_offset + 4 > data.len() {
                eprintln!("[array_get: element access out of memory bounds]");
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

    eprintln!("✅ WebAssembly module loaded successfully");
    eprintln!(
        "📋 Exported functions: {:?}",
        instance
            .exports(&mut store)
            .map(|e| e.name())
            .collect::<Vec<_>>()
    );

    let run_tests = instance.get_func(&mut store, "_run_tests");
    let start_func = instance
        .get_func(&mut store, "start")
        .or_else(|| instance.get_func(&mut store, "main"))
        .or_else(|| instance.get_func(&mut store, "_main"));

    // A module without a `start:` block but with a `tests:` block exports a
    // `_start` that only calls `_run_tests`. Call `_run_tests` directly
    // instead, so the tests run once and their failure count is seen.
    let start_func = match (start_func, &run_tests) {
        (Some(func), _) => Some(func),
        (None, Some(_)) => None,
        (None, None) => instance.get_func(&mut store, "_start"),
    };

    if let Some(func) = start_func {
        eprintln!("🎯 Executing start function...");
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
        call_or_exit(&func, &mut store, &mut results);
        run_async_tasks(&instance, &mut store);
        println!("--- End Output ---");
        // Post-run heap footprint report — opt-in via CLN_WASMTIME_HEAP_REPORT=1.
        // Handy for SSR-style leak investigations where you want to compare
        // final __heap_ptr and memory.pages across N-scaled runs of the same
        // module. Off by default so normal test output stays clean.
        if std::env::var("CLN_WASMTIME_HEAP_REPORT").is_ok() {
            if let Some(Extern::Global(heap_global)) = instance.get_export(&mut store, "__heap_ptr")
            {
                let final_heap = heap_global.get(&mut store).i32().unwrap_or(0);
                let pages = instance
                    .get_memory(&mut store, "memory")
                    .map(|m| m.size(&store))
                    .unwrap_or(0);
                eprintln!("📊 __heap_ptr={final_heap} memory_pages={pages}");
            }
        }
        eprintln!("✅ Execution completed successfully!");
    } else if run_tests.is_none() {
        eprintln!("⚠️  No start function found. Available exports:");
        for export in instance.exports(&mut store) {
            let name = export.name().to_string();
            if export.into_func().is_some() {
                eprintln!("  • {} (function)", name);
            }
        }
        return Err(anyhow::anyhow!("No start/main function found").into());
    }

    if let Some(func) = run_tests {
        // A test-only module's test output is its program output.
        let (open, close) = if start_func.is_some() {
            ("--- Test Output ---", "--- End Test Output ---")
        } else {
            ("--- Output ---", "--- End Output ---")
        };
        eprintln!("🧪 Running tests (_run_tests)...");
        println!("{open}");
        let mut results = vec![wasmtime::Val::I32(0)];
        call_or_exit(&func, &mut store, &mut results);
        if start_func.is_none() {
            run_async_tasks(&instance, &mut store);
        }
        println!("{close}");
        let failures = match results.first() {
            Some(wasmtime::Val::I32(failures)) => *failures,
            _ => return Err(anyhow::anyhow!("_run_tests returned no failure count").into()),
        };
        if failures == 0 {
            eprintln!("✅ All tests passed!");
        } else {
            eprintln!("❌ {} test(s) failed.", failures);
            return Err(anyhow::anyhow!("{} test(s) failed", failures).into());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// JSON bridges (stdlib-reference §8, BOXED_ANY_ABI.md)
// ---------------------------------------------------------------------------
//
// `_json_encode_v2` / `_json_encode_pretty_v2` / `_json_decode_v2` /
// `_json_get` come from the crate's shared host implementation, the same
// one the plugin sandbox uses. Results are allocated with the module's own
// `malloc` export so they stay coherent with its heap bookkeeping.

fn register_json_bridges(linker: &mut Linker<()>) -> Result<()> {
    clean_language_compiler::runtime::json_bridge::register_json_bridges(linker, module_malloc)
}

fn module_malloc(caller: &mut Caller<'_, ()>, size: i32) -> std::result::Result<i32, String> {
    let malloc = caller
        .get_export("malloc")
        .and_then(Extern::into_func)
        .ok_or_else(|| "module exports no malloc".to_string())?
        .typed::<i32, i32>(&*caller)
        .map_err(|e| format!("malloc has an unexpected signature: {e}"))?;
    malloc
        .call(&mut *caller, size)
        .map_err(|e| format!("malloc failed: {e}"))
}
