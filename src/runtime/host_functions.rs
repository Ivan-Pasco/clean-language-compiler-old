//! Centralized Host Functions Registry for Clean Language Runtime
//!
//! This module provides a unified registry of all WebAssembly host functions
//! required by the Clean Language runtime. It ensures consistency across
//! different runtime implementations (async, sync, fallback).

use crate::error::CompilerError;
use std::io::{self, Write};
use wasmtime::{Caller, Linker};

/// Register all host functions with a wasmtime Linker
/// This is the single source of truth for all host function definitions
pub fn register_all_host_functions(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    register_console_functions(linker)?;
    register_type_conversion_functions(linker)?;
    register_file_operations(linker)?;
    register_http_operations(linker)?;
    register_math_functions(linker)?;
    register_memory_functions(linker)?;
    register_async_functions(linker)?;
    register_method_style_functions(linker)?;
    Ok(())
}

/// Console I/O Functions
fn register_console_functions(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // print(ptr: i32, len: i32) - Output text without newline
    linker.func_wrap("env", "print", |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
        if let Some(memory) = caller.get_export("memory") {
            if let Some(memory) = memory.into_memory() {
                let data = memory.data(&caller);
                if ptr >= 0 && len >= 0 {
                    let start = ptr as usize;
                    let len = len as usize;
                    if start + len <= data.len() {
                        if let Ok(s) = std::str::from_utf8(&data[start..start + len]) {
                            print!("{s}");
                            io::stdout().flush().unwrap();
                        } else {
                            eprintln!("ERROR: Invalid UTF-8 data in print function at ptr={ptr}, len={len}");
                        }
                    } else {
                        eprintln!("ERROR: Memory access out of bounds in print function: ptr={ptr}, len={len}, memory_size={}", data.len());
                    }
                } else {
                    eprintln!("ERROR: Invalid parameters in print function: ptr={ptr}, len={len}");
                }
            } else {
                eprintln!("ERROR: Could not access memory in print function");
            }
        } else {
            eprintln!("ERROR: Memory export not found in print function");
        }
    })
    .map_err(|e| CompilerError::runtime_error(format!("Failed to create print function: {e}"), None, None))?;

    // printl(ptr: i32, len: i32) - Output text with newline
    linker.func_wrap("env", "printl", |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
        if let Some(memory) = caller.get_export("memory") {
            if let Some(memory) = memory.into_memory() {
                let data = memory.data(&caller);
                if ptr >= 0 && len >= 0 {
                    let start = ptr as usize;
                    let len = len as usize;
                    if start + len <= data.len() {
                        if let Ok(s) = std::str::from_utf8(&data[start..start + len]) {
                            println!("{s}");
                        } else {
                            eprintln!("ERROR: Invalid UTF-8 data in printl function at ptr={ptr}, len={len}");
                        }
                    } else {
                        eprintln!("ERROR: Memory access out of bounds in printl function: ptr={ptr}, len={len}, memory_size={}", data.len());
                    }
                } else {
                    eprintln!("ERROR: Invalid parameters in printl function: ptr={ptr}, len={len}");
                }
            } else {
                eprintln!("ERROR: Could not access memory in printl function");
            }
        } else {
            eprintln!("ERROR: Memory export not found in printl function");
        }
    })
    .map_err(|e| CompilerError::runtime_error(format!("Failed to create printl function: {e}"), None, None))?;

    // input(prompt_ptr: i32, prompt_len: i32) -> string_ptr: i32
    linker
        .func_wrap(
            "env",
            "input",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
                // Extract prompt from memory
                let prompt = extract_string_from_memory(&mut caller, prompt_ptr, prompt_len);

                // Get user input
                print!("{prompt}");
                io::stdout().flush().unwrap();

                let mut input = String::new();
                match io::stdin().read_line(&mut input) {
                    Ok(_) => {
                        let input = input.trim_end_matches('\n');
                        // Allocate string in WASM memory and return pointer
                        allocate_string_in_memory(&mut caller, input)
                    }
                    Err(_) => 0,
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create input function: {e}"),
                None,
                None,
            )
        })?;

    // input_integer(prompt_ptr: i32, prompt_len: i32) -> i32
    linker
        .func_wrap(
            "env",
            "input_integer",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
                let prompt = extract_string_from_memory(&mut caller, prompt_ptr, prompt_len);

                loop {
                    print!("{prompt}");
                    io::stdout().flush().unwrap();

                    let mut input = String::new();
                    match io::stdin().read_line(&mut input) {
                        Ok(_) => {
                            let input = input.trim();
                            match input.parse::<i32>() {
                                Ok(value) => return value,
                                Err(_) => {
                                    println!(
                                        "❌ Please enter a valid integer. You entered: '{input}'"
                                    );
                                    continue;
                                }
                            }
                        }
                        Err(_) => return 0,
                    }
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create input_integer function: {e}"),
                None,
                None,
            )
        })?;

    // input_float(prompt_ptr: i32, prompt_len: i32) -> f64
    linker
        .func_wrap(
            "env",
            "input_float",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> f64 {
                let prompt = extract_string_from_memory(&mut caller, prompt_ptr, prompt_len);

                loop {
                    print!("{prompt}");
                    io::stdout().flush().unwrap();

                    let mut input = String::new();
                    match io::stdin().read_line(&mut input) {
                        Ok(_) => {
                            let input = input.trim();
                            match input.parse::<f64>() {
                                Ok(value) => return value,
                                Err(_) => {
                                    println!(
                                        "❌ Please enter a valid number. You entered: '{input}'"
                                    );
                                    continue;
                                }
                            }
                        }
                        Err(_) => return 0.0,
                    }
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create input_float function: {e}"),
                None,
                None,
            )
        })?;

    // input_yesno(prompt_ptr: i32, prompt_len: i32) -> i32 (1 for yes, 0 for no)
    linker.func_wrap("env", "input_yesno", |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32| -> i32 {
        let prompt = extract_string_from_memory(&mut caller, prompt_ptr, prompt_len);

        loop {
            print!("{prompt} (y/n): ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    let input = input.trim().to_lowercase();
                    match input.as_str() {
                        "y" | "yes" | "true" | "1" => return 1,
                        "n" | "no" | "false" | "0" => return 0,
                        _ => {
                            println!("❌ Please enter y/yes/true/1 or n/no/false/0. You entered: '{input}'");
                            continue;
                        }
                    }
                }
                Err(_) => return 0
            }
        }
    })
    .map_err(|e| CompilerError::runtime_error(format!("Failed to create input_yesno function: {e}"), None, None))?;

    // input_range(prompt_ptr: i32, prompt_len: i32, min: i32, max: i32) -> i32
    linker.func_wrap("env", "input_range", |mut caller: Caller<'_, ()>, prompt_ptr: i32, prompt_len: i32, min: i32, max: i32| -> i32 {
        let prompt = extract_string_from_memory(&mut caller, prompt_ptr, prompt_len);

        loop {
            print!("{prompt} ({min}-{max}): ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    let input = input.trim();
                    match input.parse::<i32>() {
                        Ok(value) => {
                            if value >= min && value <= max {
                                return value;
                            } else {
                                println!("❌ Value must be between {min} and {max}. You entered: {value}");
                                continue;
                            }
                        }
                        Err(_) => {
                            println!("❌ Please enter a valid integer. You entered: '{input}'");
                            continue;
                        }
                    }
                }
                Err(_) => return min
            }
        }
    })
    .map_err(|e| CompilerError::runtime_error(format!("Failed to create input_range function: {e}"), None, None))?;

    Ok(())
}

/// Type Conversion Functions
fn register_type_conversion_functions(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // int_to_string(value: i32) -> string_ptr: i32
    linker
        .func_wrap(
            "env",
            "int_to_string",
            |mut caller: Caller<'_, ()>, value: i32| -> i32 {
                let string_value = value.to_string();
                allocate_string_in_memory(&mut caller, &string_value)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create int_to_string function: {e}"),
                None,
                None,
            )
        })?;

    // float_to_string(value: f64) -> string_ptr: i32
    linker
        .func_wrap(
            "env",
            "float_to_string",
            |mut caller: Caller<'_, ()>, value: f64| -> i32 {
                let string_value = value.to_string();
                allocate_string_in_memory(&mut caller, &string_value)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create float_to_string function: {e}"),
                None,
                None,
            )
        })?;

    // bool_to_string(value: i32) -> string_ptr: i32 (0 = false, !0 = true)
    linker
        .func_wrap(
            "env",
            "bool_to_string",
            |mut caller: Caller<'_, ()>, value: i32| -> i32 {
                let string_value = if value != 0 { "true" } else { "false" };
                allocate_string_in_memory(&mut caller, string_value)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create bool_to_string function: {e}"),
                None,
                None,
            )
        })?;

    // string_to_int(string_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string_to_int",
            |mut caller: Caller<'_, ()>, string_ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if string_ptr >= 0 && (string_ptr + 4) as usize <= data.len() {
                            // Read string length from first 4 bytes
                            let len_bytes = &data[string_ptr as usize..(string_ptr + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);

                            if len >= 0 && (string_ptr + 4 + len) as usize <= data.len() {
                                // Extract string data
                                let string_data = &data
                                    [(string_ptr + 4) as usize..(string_ptr + 4 + len) as usize];
                                if let Ok(string_value) = std::str::from_utf8(string_data) {
                                    return string_value.parse::<i32>().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
                0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_to_int function: {e}"),
                None,
                None,
            )
        })?;

    // string_to_float(string_ptr: i32) -> f64
    linker
        .func_wrap(
            "env",
            "string_to_float",
            |mut caller: Caller<'_, ()>, string_ptr: i32| -> f64 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if string_ptr >= 0 && (string_ptr + 4) as usize <= data.len() {
                            // Read string length from first 4 bytes
                            let len_bytes = &data[string_ptr as usize..(string_ptr + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);

                            if len >= 0 && (string_ptr + 4 + len) as usize <= data.len() {
                                // Extract string data
                                let string_data = &data
                                    [(string_ptr + 4) as usize..(string_ptr + 4 + len) as usize];
                                if let Ok(string_value) = std::str::from_utf8(string_data) {
                                    return string_value.parse::<f64>().unwrap_or(0.0);
                                }
                            }
                        }
                    }
                }
                0.0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_to_float function: {e}"),
                None,
                None,
            )
        })?;

    Ok(())
}

/// File I/O Operations
fn register_file_operations(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // file_read(path_ptr: i32, path_len: i32, max_size: i32) -> string_ptr: i32
    linker
        .func_wrap(
            "env",
            "file_read",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32, _max_size: i32| -> i32 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);

                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        // Allocate string in WASM memory and return pointer
                        allocate_string_in_memory(&mut caller, &content)
                    }
                    Err(_) => 0, // Return 0 for failure
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_read function: {e}"),
                None,
                None,
            )
        })?;

    // file_write(path_ptr: i32, path_len: i32, content_ptr: i32, content_len: i32) -> i32
    linker
        .func_wrap(
            "env",
            "file_write",
            |mut caller: Caller<'_, ()>,
             path_ptr: i32,
             path_len: i32,
             content_ptr: i32,
             content_len: i32|
             -> i32 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);
                let content = extract_string_from_memory(&mut caller, content_ptr, content_len);

                match std::fs::write(&path, &content) {
                    Ok(_) => 1,  // Success
                    Err(_) => 0, // Failure
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_write function: {e}"),
                None,
                None,
            )
        })?;

    // file_exists(path_ptr: i32, path_len: i32) -> i32 (1 if exists, 0 if not)
    linker
        .func_wrap(
            "env",
            "file_exists",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);
                if std::path::Path::new(&path).exists() {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_exists function: {e}"),
                None,
                None,
            )
        })?;

    // file_delete(path_ptr: i32, path_len: i32) -> i32 (1 for success, 0 for failure)
    linker
        .func_wrap(
            "env",
            "file_delete",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);

                match std::fs::remove_file(&path) {
                    Ok(_) => 1,  // Success
                    Err(_) => 0, // Failure
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_delete function: {e}"),
                None,
                None,
            )
        })?;

    // file_append(path_ptr: i32, path_len: i32, content_ptr: i32, content_len: i32) -> i32
    linker
        .func_wrap(
            "env",
            "file_append",
            |mut caller: Caller<'_, ()>,
             path_ptr: i32,
             path_len: i32,
             content_ptr: i32,
             content_len: i32|
             -> i32 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);
                let content = extract_string_from_memory(&mut caller, content_ptr, content_len);

                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut file| {
                        use std::io::Write;
                        file.write_all(content.as_bytes())
                    }) {
                    Ok(_) => 1,  // Success
                    Err(_) => 0, // Failure
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_append function: {e}"),
                None,
                None,
            )
        })?;

    // file_size(path_ptr: i32, path_len: i32) -> i64 (file size in bytes, -1 if error)
    linker
        .func_wrap(
            "env",
            "file_size",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i64 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);

                match std::fs::metadata(&path) {
                    Ok(metadata) => metadata.len() as i64,
                    Err(_) => -1, // Error
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_size function: {e}"),
                None,
                None,
            )
        })?;

    // file_is_directory(path_ptr: i32, path_len: i32) -> i32 (1 if directory, 0 if not)
    linker
        .func_wrap(
            "env",
            "file_is_directory",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);

                match std::fs::metadata(&path) {
                    Ok(metadata) => {
                        if metadata.is_dir() {
                            1
                        } else {
                            0
                        }
                    }
                    Err(_) => 0,
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_is_directory function: {e}"),
                None,
                None,
            )
        })?;

    // directory_create(path_ptr: i32, path_len: i32) -> i32 (1 for success, 0 for failure)
    linker
        .func_wrap(
            "env",
            "directory_create",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);

                match std::fs::create_dir_all(&path) {
                    Ok(_) => 1,  // Success
                    Err(_) => 0, // Failure
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create directory_create function: {e}"),
                None,
                None,
            )
        })?;

    // directory_list(path_ptr: i32, path_len: i32) -> string_ptr: i32 (JSON array of filenames)
    linker
        .func_wrap(
            "env",
            "directory_list",
            |mut caller: Caller<'_, ()>, path_ptr: i32, path_len: i32| -> i32 {
                let path = extract_string_from_memory(&mut caller, path_ptr, path_len);

                match std::fs::read_dir(&path) {
                    Ok(entries) => {
                        let mut filenames = Vec::new();
                        for entry in entries.flatten() {
                            if let Some(name) = entry.file_name().to_str() {
                                filenames.push(format!("\"{}\"", name));
                            }
                        }
                        let json_array = format!("[{}]", filenames.join(","));
                        allocate_string_in_memory(&mut caller, &json_array)
                    }
                    Err(_) => allocate_string_in_memory(&mut caller, "[]"), // Empty array on error
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create directory_list function: {e}"),
                None,
                None,
            )
        })?;

    // file_copy(src_ptr: i32, src_len: i32, dest_ptr: i32, dest_len: i32) -> i32 (1 for success, 0 for failure)
    linker
        .func_wrap(
            "env",
            "file_copy",
            |mut caller: Caller<'_, ()>,
             src_ptr: i32,
             src_len: i32,
             dest_ptr: i32,
             dest_len: i32|
             -> i32 {
                let src_path = extract_string_from_memory(&mut caller, src_ptr, src_len);
                let dest_path = extract_string_from_memory(&mut caller, dest_ptr, dest_len);

                match std::fs::copy(&src_path, &dest_path) {
                    Ok(_) => 1,  // Success
                    Err(_) => 0, // Failure
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create file_copy function: {e}"),
                None,
                None,
            )
        })?;

    Ok(())
}

/// HTTP Operations (Real Implementations)
fn register_http_operations(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // http_get(url_ptr: i32, url_len: i32) -> response_ptr: i32
    linker.func_wrap(
        "env",
        "http_get",
        |mut caller: Caller<'_, ()>, url_ptr: i32, url_len: i32| -> i32 {
            let url = extract_string_from_memory(&mut caller, url_ptr, url_len);
            let url_clone = url.clone();

            match std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    match reqwest::get(&url).await {
                        Ok(response) => response
                            .text()
                            .await
                            .unwrap_or_else(|_| format!("Error reading response from {}", url)),
                        Err(_) => format!("HTTP GET failed for URL: {}", url),
                    }
                })
            })
            .join()
            {
                Ok(response_text) => allocate_string_in_memory(&mut caller, &response_text),
                Err(_) => {
                    let error_msg = format!("Failed to execute HTTP GET request to {}", url_clone);
                    allocate_string_in_memory(&mut caller, &error_msg)
                }
            }
        },
    )?;
    // http_post(url_ptr: i32, url_len: i32, body_ptr: i32, body_len: i32) -> response_ptr: i32
    linker.func_wrap(
        "env",
        "http_post",
        |mut caller: Caller<'_, ()>,
         url_ptr: i32,
         url_len: i32,
         body_ptr: i32,
         body_len: i32|
         -> i32 {
            let url = extract_string_from_memory(&mut caller, url_ptr, url_len);
            let body = extract_string_from_memory(&mut caller, body_ptr, body_len);
            let url_clone = url.clone();

            match std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = reqwest::Client::new();
                    match client
                        .post(&url)
                        .body(body)
                        .header("Content-Type", "text/plain")
                        .send()
                        .await
                    {
                        Ok(response) => response.text().await.unwrap_or_else(|_| {
                            format!("Error reading POST response from {}", url)
                        }),
                        Err(_) => format!("HTTP POST failed for URL: {}", url),
                    }
                })
            })
            .join()
            {
                Ok(response_text) => allocate_string_in_memory(&mut caller, &response_text),
                Err(_) => {
                    let error_msg = format!("Failed to execute HTTP POST request to {}", url_clone);
                    allocate_string_in_memory(&mut caller, &error_msg)
                }
            }
        },
    )?;
    linker.func_wrap("env", "http_put", |_: i32, _: i32, _: i32, _: i32| -> i32 {
        0
    })?;
    linker.func_wrap(
        "env",
        "http_patch",
        |_: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
    )?;
    // http_delete(url_ptr: i32, url_len: i32) -> response_ptr: i32
    linker.func_wrap(
        "env",
        "http_delete",
        |mut caller: Caller<'_, ()>, url_ptr: i32, url_len: i32| -> i32 {
            let url = extract_string_from_memory(&mut caller, url_ptr, url_len);
            let url_clone = url.clone();

            match std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = reqwest::Client::new();
                    match client.delete(&url).send().await {
                        Ok(response) => response.text().await.unwrap_or_else(|_| {
                            format!("Error reading DELETE response from {}", url)
                        }),
                        Err(_) => format!("HTTP DELETE failed for URL: {}", url),
                    }
                })
            })
            .join()
            {
                Ok(response_text) => allocate_string_in_memory(&mut caller, &response_text),
                Err(_) => {
                    let error_msg =
                        format!("Failed to execute HTTP DELETE request to {}", url_clone);
                    allocate_string_in_memory(&mut caller, &error_msg)
                }
            }
        },
    )?;
    linker.func_wrap("env", "http_head", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_options", |_: i32, _: i32| -> i32 { 0 })?;

    // Extended HTTP functions
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
    // http_post_json(url_ptr: i32, url_len: i32, json_ptr: i32, json_len: i32) -> response_ptr: i32
    linker.func_wrap(
        "env",
        "http_post_json",
        |mut caller: Caller<'_, ()>,
         url_ptr: i32,
         url_len: i32,
         json_ptr: i32,
         json_len: i32|
         -> i32 {
            let url = extract_string_from_memory(&mut caller, url_ptr, url_len);
            let json_body = extract_string_from_memory(&mut caller, json_ptr, json_len);
            let url_clone = url.clone();

            match std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = reqwest::Client::new();
                    match client
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .body(json_body)
                        .send()
                        .await
                    {
                        Ok(response) => response.text().await.unwrap_or_else(|_| {
                            format!("Error reading JSON POST response from {}", url)
                        }),
                        Err(_) => format!("HTTP JSON POST failed for URL: {}", url),
                    }
                })
            })
            .join()
            {
                Ok(response_text) => allocate_string_in_memory(&mut caller, &response_text),
                Err(_) => {
                    let error_msg =
                        format!("Failed to execute HTTP JSON POST request to {}", url_clone);
                    allocate_string_in_memory(&mut caller, &error_msg)
                }
            }
        },
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

    // HTTP configuration functions
    linker.func_wrap("env", "http_set_user_agent", |_: i32, _: i32| {})?;
    linker.func_wrap("env", "http_set_timeout", |_: i32| {})?;
    linker.func_wrap("env", "http_set_max_redirects", |_: i32| {})?;
    linker.func_wrap("env", "http_enable_cookies", |_: i32| {})?;
    linker.func_wrap("env", "http_get_response_code", || -> i32 { 200 })?;
    linker.func_wrap("env", "http_get_response_headers", || -> i32 { 0 })?;
    linker.func_wrap("env", "http_encode_url", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_decode_url", |_: i32, _: i32| -> i32 { 0 })?;
    linker.func_wrap("env", "http_build_query", |_: i32, _: i32| -> i32 { 0 })?;

    Ok(())
}

/// Math Functions
fn register_math_functions(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // math_abs(value: f64) -> f64
    linker
        .func_wrap("env", "math_abs", |value: f64| -> f64 { value.abs() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_abs function: {e}"),
                None,
                None,
            )
        })?;

    // math_sqrt(value: f64) -> f64
    linker
        .func_wrap("env", "math_sqrt", |value: f64| -> f64 {
            if value < 0.0 {
                f64::NAN // Return NaN for negative inputs
            } else {
                value.sqrt()
            }
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_sqrt function: {e}"),
                None,
                None,
            )
        })?;

    // math_pow(base: f64, exponent: f64) -> f64
    linker
        .func_wrap("env", "math_pow", |base: f64, exponent: f64| -> f64 {
            base.powf(exponent)
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_pow function: {e}"),
                None,
                None,
            )
        })?;

    // math_floor(value: f64) -> f64
    linker
        .func_wrap("env", "math_floor", |value: f64| -> f64 { value.floor() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_floor function: {e}"),
                None,
                None,
            )
        })?;

    // math_ceil(value: f64) -> f64
    linker
        .func_wrap("env", "math_ceil", |value: f64| -> f64 { value.ceil() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_ceil function: {e}"),
                None,
                None,
            )
        })?;

    // math_round(value: f64) -> f64
    linker
        .func_wrap("env", "math_round", |value: f64| -> f64 { value.round() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_round function: {e}"),
                None,
                None,
            )
        })?;

    // math_sin(value: f64) -> f64
    linker
        .func_wrap("env", "math_sin", |value: f64| -> f64 { value.sin() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_sin function: {e}"),
                None,
                None,
            )
        })?;

    // math_cos(value: f64) -> f64
    linker
        .func_wrap("env", "math_cos", |value: f64| -> f64 { value.cos() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_cos function: {e}"),
                None,
                None,
            )
        })?;

    // math_tan(value: f64) -> f64
    linker
        .func_wrap("env", "math_tan", |value: f64| -> f64 { value.tan() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_tan function: {e}"),
                None,
                None,
            )
        })?;

    // math_min(a: f64, b: f64) -> f64
    linker
        .func_wrap("env", "math_min", |a: f64, b: f64| -> f64 { a.min(b) })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_min function: {e}"),
                None,
                None,
            )
        })?;

    // math_max(a: f64, b: f64) -> f64
    linker
        .func_wrap("env", "math_max", |a: f64, b: f64| -> f64 { a.max(b) })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_max function: {e}"),
                None,
                None,
            )
        })?;

    Ok(())
}

/// Memory Management Functions
fn register_memory_functions(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // mem_alloc(type_id: i32, size: i32) -> i32
    linker
        .func_wrap(
            "memory_runtime",
            "mem_alloc",
            |mut caller: Caller<'_, ()>, _type_id: i32, size: i32| -> i32 {
                if size <= 0 {
                    return 0;
                }

                // Get the memory export from the WASM module
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        // Get current memory size in bytes
                        let current_size = memory.data_size(&caller) as i32;

                        // Align the allocation (use a default alignment of 8 bytes)
                        let alignment = 8;
                        let aligned_size = ((size + alignment - 1) / alignment) * alignment;

                        // Simple bump allocator - start after initial 4KB reserved space
                        static mut NEXT_ALLOC: i32 = 4096;
                        unsafe {
                            let ptr = NEXT_ALLOC;
                            NEXT_ALLOC += aligned_size;

                            // Check if we need to grow memory
                            if NEXT_ALLOC > current_size {
                                let pages_needed = ((NEXT_ALLOC - current_size) + 65535) / 65536; // 64KB pages
                                if memory.grow(&mut caller, pages_needed as u64).is_err() {
                                    return 0; // Allocation failed
                                }
                            }

                            ptr
                        }
                    } else {
                        0
                    }
                } else {
                    0
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create mem_alloc function: {e}"),
                None,
                None,
            )
        })?;

    // mem_retain(ptr: i32) - Reference counting increment
    linker
        .func_wrap(
            "memory_runtime",
            "mem_retain",
            |mut caller: Caller<'_, ()>, ptr: i32| {
                if ptr <= 0 {
                    return; // Invalid pointer, no-op
                }

                // Access WebAssembly memory to update reference count
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data_mut(&mut caller);
                        let ptr_usize = ptr as usize;

                        // Check if we have enough space to read the reference count (first 4 bytes)
                        if ptr_usize + 4 <= data.len() {
                            // Read current reference count from first 4 bytes
                            let ref_count_bytes = &mut data[ptr_usize..ptr_usize + 4];
                            let mut ref_count = u32::from_le_bytes([
                                ref_count_bytes[0],
                                ref_count_bytes[1],
                                ref_count_bytes[2],
                                ref_count_bytes[3],
                            ]);

                            // Increment reference count (with overflow protection)
                            ref_count = ref_count.saturating_add(1);

                            // Write back the updated reference count
                            let new_bytes = ref_count.to_le_bytes();
                            ref_count_bytes.copy_from_slice(&new_bytes);
                        }
                    }
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create mem_retain function: {e}"),
                None,
                None,
            )
        })?;

    // mem_release(ptr: i32) - Reference counting decrement (void return)
    linker
        .func_wrap(
            "memory_runtime",
            "mem_release",
            |mut caller: Caller<'_, ()>, ptr: i32| {
                if ptr <= 0 {
                    return; // Invalid pointer, nothing to do
                }

                // Access WebAssembly memory to update reference count
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data_mut(&mut caller);
                        let ptr_usize = ptr as usize;

                        // Check if we have enough space to read the reference count (first 4 bytes)
                        if ptr_usize + 4 <= data.len() {
                            // Read current reference count from first 4 bytes
                            let ref_count_bytes = &mut data[ptr_usize..ptr_usize + 4];
                            let mut ref_count = u32::from_le_bytes([
                                ref_count_bytes[0],
                                ref_count_bytes[1],
                                ref_count_bytes[2],
                                ref_count_bytes[3],
                            ]);

                            // Decrement reference count (with underflow protection)
                            if ref_count > 0 {
                                ref_count -= 1;

                                // Write back the updated reference count
                                let new_bytes = ref_count.to_le_bytes();
                                ref_count_bytes.copy_from_slice(&new_bytes);

                                // If ref_count reaches 0, the memory could be marked for collection
                                // but we'll let the WASM-side handle deallocation logic
                            }
                        }
                    }
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create mem_release function: {e}"),
                None,
                None,
            )
        })?;

    Ok(())
}

/// Async Functions (Stubs)
fn register_async_functions(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // execute_background stub
    linker.func_wrap("env", "execute_background", |_: i32, _: i32| -> i32 { 0 })?;
    Ok(())
}

/// Method-Style Function Imports
fn register_method_style_functions(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // integer.toString() - Converts integer to string
    linker
        .func_wrap(
            "env",
            "integer.toString",
            |mut caller: Caller<'_, ()>, value: i32| -> i32 {
                let string_value = value.to_string();
                allocate_string_in_memory(&mut caller, &string_value)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create integer.toString function: {e}"),
                None,
                None,
            )
        })?;

    // number.toString() - Converts float to string
    linker
        .func_wrap(
            "env",
            "number.toString",
            |mut caller: Caller<'_, ()>, value: f64| -> i32 {
                let string_value = value.to_string();
                allocate_string_in_memory(&mut caller, &string_value)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create number.toString function: {e}"),
                None,
                None,
            )
        })?;

    // boolean.toString() - Converts boolean to string
    linker
        .func_wrap(
            "env",
            "boolean.toString",
            |mut caller: Caller<'_, ()>, value: i32| -> i32 {
                let string_value = if value != 0 { "true" } else { "false" };
                allocate_string_in_memory(&mut caller, string_value)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create boolean.toString function: {e}"),
                None,
                None,
            )
        })?;

    // string.length() - Returns string length
    linker
        .func_wrap(
            "env",
            "string.length",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr >= 0 && (ptr + 4) as usize <= data.len() {
                            // Read the length from the first 4 bytes at the pointer
                            let len_bytes = &data[ptr as usize..(ptr + 4) as usize];
                            return i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);
                        }
                    }
                }
                0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.length function: {e}"),
                None,
                None,
            )
        })?;

    // string.toString() - Identity conversion for strings
    linker
        .func_wrap("env", "string.toString", |ptr: i32| -> i32 {
            // String to string is identity - just return the same pointer
            ptr
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.toString function: {e}"),
                None,
                None,
            )
        })?;

    // string.toInteger() - Convert string to integer
    linker
        .func_wrap(
            "env",
            "string.toInteger",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr >= 0 && (ptr + 4) as usize <= data.len() {
                            // Read string length from first 4 bytes
                            let len_bytes = &data[ptr as usize..(ptr + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);

                            if len >= 0 && (ptr + 4 + len) as usize <= data.len() {
                                // Extract string data
                                let string_data =
                                    &data[(ptr + 4) as usize..(ptr + 4 + len) as usize];
                                if let Ok(string_value) = std::str::from_utf8(string_data) {
                                    return string_value.parse::<i32>().unwrap_or(0);
                                }
                            }
                        }
                    }
                }
                0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.toInteger function: {e}"),
                None,
                None,
            )
        })?;

    // string.toNumber() - Convert string to float
    linker
        .func_wrap(
            "env",
            "string.toNumber",
            |mut caller: Caller<'_, ()>, ptr: i32| -> f64 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr >= 0 && (ptr + 4) as usize <= data.len() {
                            // Read string length from first 4 bytes
                            let len_bytes = &data[ptr as usize..(ptr + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);

                            if len >= 0 && (ptr + 4 + len) as usize <= data.len() {
                                // Extract string data
                                let string_data =
                                    &data[(ptr + 4) as usize..(ptr + 4 + len) as usize];
                                if let Ok(string_value) = std::str::from_utf8(string_data) {
                                    return string_value.parse::<f64>().unwrap_or(0.0);
                                }
                            }
                        }
                    }
                }
                0.0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.toNumber function: {e}"),
                None,
                None,
            )
        })?;

    // string.toBoolean() - Convert string to boolean
    linker
        .func_wrap(
            "env",
            "string.toBoolean",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr >= 0 && (ptr + 4) as usize <= data.len() {
                            // Read string length from first 4 bytes
                            let len_bytes = &data[ptr as usize..(ptr + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);

                            if len >= 0 && (ptr + 4 + len) as usize <= data.len() {
                                // Extract string data
                                let string_data =
                                    &data[(ptr + 4) as usize..(ptr + 4 + len) as usize];
                                if let Ok(string_value) = std::str::from_utf8(string_data) {
                                    return match string_value.to_lowercase().as_str() {
                                        "true" | "1" | "yes" | "on" => 1,
                                        "false" | "0" | "no" | "off" | "" => 0,
                                        _ => {
                                            if string_value.len() > 0 {
                                                1
                                            } else {
                                                0
                                            }
                                        } // Non-empty string is truthy
                                    };
                                }
                            }
                        }
                    }
                }
                0
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.toBoolean function: {e}"),
                None,
                None,
            )
        })?;

    // list.length() - Returns list length
    linker
        .func_wrap(
            "env",
            "list.length",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                if ptr <= 0 {
                    return 0; // Invalid pointer
                }

                // Access WebAssembly memory to read list structure
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        let ptr_usize = ptr as usize;

                        // List memory layout: [ref_count(4), type_id(4), size(4), gc_flags(1), length(4), data...]
                        // We need to read the length field which is at offset 13 (4+4+4+1)
                        let length_offset = ptr_usize + 13;

                        if length_offset + 4 <= data.len() {
                            // Read list length from memory
                            let length_bytes = &data[length_offset..length_offset + 4];
                            let length = i32::from_le_bytes([
                                length_bytes[0],
                                length_bytes[1],
                                length_bytes[2],
                                length_bytes[3],
                            ]);
                            return length.max(0); // Ensure non-negative length
                        }
                    }
                }
                0 // Return 0 if unable to read from memory
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create list.length function: {e}"),
                None,
                None,
            )
        })?;

    // Type conversion methods

    // integer.toInteger() - Identity conversion
    linker
        .func_wrap("env", "integer.toInteger", |value: i32| -> i32 { value })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create integer.toInteger function: {e}"),
                None,
                None,
            )
        })?;

    // integer.toNumber() - Convert integer to float
    linker
        .func_wrap("env", "integer.toNumber", |value: i32| -> f64 {
            value as f64
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create integer.toNumber function: {e}"),
                None,
                None,
            )
        })?;

    // integer.toBoolean() - Convert integer to boolean
    linker
        .func_wrap("env", "integer.toBoolean", |value: i32| -> i32 {
            if value != 0 {
                1
            } else {
                0
            }
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create integer.toBoolean function: {e}"),
                None,
                None,
            )
        })?;

    // number.toNumber() - Identity conversion
    linker
        .func_wrap("env", "number.toNumber", |value: f64| -> f64 { value })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create number.toNumber function: {e}"),
                None,
                None,
            )
        })?;

    // number.toInteger() - Convert float to integer
    linker
        .func_wrap("env", "number.toInteger", |value: f64| -> i32 {
            value as i32
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create number.toInteger function: {e}"),
                None,
                None,
            )
        })?;

    // number.toBoolean() - Convert float to boolean
    linker
        .func_wrap("env", "number.toBoolean", |value: f64| -> i32 {
            if value != 0.0 {
                1
            } else {
                0
            }
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create number.toBoolean function: {e}"),
                None,
                None,
            )
        })?;

    // boolean.toBoolean() - Identity conversion
    linker
        .func_wrap("env", "boolean.toBoolean", |value: i32| -> i32 {
            if value != 0 {
                1
            } else {
                0
            }
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create boolean.toBoolean function: {e}"),
                None,
                None,
            )
        })?;

    // boolean.toInteger() - Convert boolean to integer
    linker
        .func_wrap("env", "boolean.toInteger", |value: i32| -> i32 {
            if value != 0 {
                1
            } else {
                0
            }
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create boolean.toInteger function: {e}"),
                None,
                None,
            )
        })?;

    // boolean.toNumber() - Convert boolean to float
    linker
        .func_wrap("env", "boolean.toNumber", |value: i32| -> f64 {
            if value != 0 {
                1.0
            } else {
                0.0
            }
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create boolean.toNumber function: {e}"),
                None,
                None,
            )
        })?;

    // Additional method-style functions

    // integer.length() - Returns number of digits in integer
    linker
        .func_wrap("env", "integer.length", |value: i32| -> i32 {
            if value == 0 {
                return 1; // Zero has 1 digit
            }

            let abs_value = value.abs() as u32;
            let mut digit_count = 0;
            let mut temp = abs_value;

            while temp > 0 {
                digit_count += 1;
                temp /= 10;
            }

            digit_count
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create integer.length function: {e}"),
                None,
                None,
            )
        })?;

    // number.length() - Returns number of digits in float
    linker
        .func_wrap("env", "number.length", |value: f64| -> i32 {
            if value.is_nan() || value.is_infinite() {
                return 0; // Special values have no meaningful digit count
            }

            // Convert to string representation and count significant digits
            let string_repr = value.to_string();
            let mut digit_count = 0;

            for ch in string_repr.chars() {
                if ch.is_ascii_digit() {
                    digit_count += 1;
                }
            }

            // Ensure at least 1 for zero
            digit_count.max(1)
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create number.length function: {e}"),
                None,
                None,
            )
        })?;

    // boolean.length() - Returns 1 for true, 1 for false (single character)
    linker
        .func_wrap("env", "boolean.length", |_value: i32| -> i32 {
            // Both "true" and "false" have lengths, but this is just a placeholder
            1
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create boolean.length function: {e}"),
                None,
                None,
            )
        })?;

    // String manipulation methods

    // string.toUpperCase() - Convert string to uppercase
    linker
        .func_wrap(
            "env",
            "string.toUpperCase",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr >= 0 && (ptr + 4) as usize <= data.len() {
                            // Read string length from first 4 bytes
                            let len_bytes = &data[ptr as usize..(ptr + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);

                            if len >= 0 && (ptr + 4 + len) as usize <= data.len() {
                                // Extract string data
                                let string_data =
                                    &data[(ptr + 4) as usize..(ptr + 4 + len) as usize];
                                if let Ok(string_value) = std::str::from_utf8(string_data) {
                                    let uppercase = string_value.to_uppercase();
                                    return allocate_string_in_memory(&mut caller, &uppercase);
                                }
                            }
                        }
                    }
                }
                ptr // Return original pointer if extraction failed
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.toUpperCase function: {e}"),
                None,
                None,
            )
        })?;

    // string.toLowerCase() - Convert string to lowercase
    linker
        .func_wrap(
            "env",
            "string.toLowerCase",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);
                        if ptr >= 0 && (ptr + 4) as usize <= data.len() {
                            // Read string length from first 4 bytes
                            let len_bytes = &data[ptr as usize..(ptr + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);

                            if len >= 0 && (ptr + 4 + len) as usize <= data.len() {
                                // Extract string data
                                let string_data =
                                    &data[(ptr + 4) as usize..(ptr + 4 + len) as usize];
                                if let Ok(string_value) = std::str::from_utf8(string_data) {
                                    let lowercase = string_value.to_lowercase();
                                    return allocate_string_in_memory(&mut caller, &lowercase);
                                }
                            }
                        }
                    }
                }
                ptr // Return original pointer if extraction failed
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.toLowerCase function: {e}"),
                None,
                None,
            )
        })?;

    // string.concat(other_ptr: i32) - Concatenate two strings
    linker
        .func_wrap(
            "env",
            "string.concat",
            |mut caller: Caller<'_, ()>, ptr1: i32, ptr2: i32| -> i32 {
                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        let data = memory.data(&caller);

                        // Extract first string
                        let str1 = if ptr1 >= 0 && (ptr1 + 4) as usize <= data.len() {
                            let len_bytes = &data[ptr1 as usize..(ptr1 + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);
                            if len >= 0 && (ptr1 + 4 + len) as usize <= data.len() {
                                let string_data =
                                    &data[(ptr1 + 4) as usize..(ptr1 + 4 + len) as usize];
                                std::str::from_utf8(string_data).unwrap_or("")
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };

                        // Extract second string
                        let str2 = if ptr2 >= 0 && (ptr2 + 4) as usize <= data.len() {
                            let len_bytes = &data[ptr2 as usize..(ptr2 + 4) as usize];
                            let len = i32::from_le_bytes([
                                len_bytes[0],
                                len_bytes[1],
                                len_bytes[2],
                                len_bytes[3],
                            ]);
                            if len >= 0 && (ptr2 + 4 + len) as usize <= data.len() {
                                let string_data =
                                    &data[(ptr2 + 4) as usize..(ptr2 + 4 + len) as usize];
                                std::str::from_utf8(string_data).unwrap_or("")
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };

                        // Concatenate and allocate new string
                        let concatenated = format!("{}{}", str1, str2);
                        return allocate_string_in_memory(&mut caller, &concatenated);
                    }
                }
                ptr1 // Return first pointer if extraction failed
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.concat function: {e}"),
                None,
                None,
            )
        })?;

    Ok(())
}

/// Utility function to extract string from WASM memory
fn extract_string_from_memory(caller: &mut Caller<'_, ()>, ptr: i32, len: i32) -> String {
    if let Some(memory) = caller.get_export("memory") {
        if let Some(memory) = memory.into_memory() {
            let data = memory.data(caller);
            if ptr >= 0 && len >= 0 {
                let start = ptr as usize;
                let len = len as usize;
                if start + len <= data.len() {
                    if let Ok(s) = std::str::from_utf8(&data[start..start + len]) {
                        return s.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

/// Utility function to allocate and write a string to WASM memory
fn allocate_string_in_memory(caller: &mut Caller<'_, ()>, string_value: &str) -> i32 {
    let string_bytes = string_value.as_bytes();
    let string_len = string_bytes.len() as i32;

    // Allocate memory for the string (we'll store length + data)
    let total_size = 4 + string_len; // 4 bytes for length + string data

    if let Some(memory) = caller.get_export("memory") {
        if let Some(memory) = memory.into_memory() {
            // Simple allocation - find next available spot
            static mut NEXT_STRING_ALLOC: i32 = 8192; // Start string allocations at 8KB
            unsafe {
                let ptr = NEXT_STRING_ALLOC;
                NEXT_STRING_ALLOC += total_size + 4; // Add padding

                // Check if we need to grow memory first
                let current_size = memory.data_size(&*caller) as usize;
                let needed_size = (ptr + total_size) as usize;

                if needed_size > current_size {
                    let pages_needed = ((needed_size - current_size) + 65535) / 65536;
                    if memory.grow(&mut *caller, pages_needed as u64).is_err() {
                        return 0; // Growth failed
                    }
                }

                // Get memory data as mutable after potential growth
                let data = memory.data_mut(&mut *caller);

                // Write string length at the pointer location (first 4 bytes)
                let len_bytes = string_len.to_le_bytes();
                data[ptr as usize..(ptr + 4) as usize].copy_from_slice(&len_bytes);

                // Write string data after the length
                data[(ptr + 4) as usize..(ptr + 4 + string_len) as usize]
                    .copy_from_slice(string_bytes);

                return ptr;
            }
        }
    }

    0 // Allocation failed
}
