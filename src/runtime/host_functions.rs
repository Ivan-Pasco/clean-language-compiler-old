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
    linker
        .func_wrap(
            "env",
            "print",
            |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
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
                                    tracing::error!(
                                        ptr = ptr,
                                        len = len,
                                        "Invalid UTF-8 data in print function"
                                    );
                                }
                            } else {
                                tracing::error!(
                                    ptr = ptr,
                                    len = len,
                                    memory_size = data.len(),
                                    "Memory access out of bounds in print function"
                                );
                            }
                        } else {
                            tracing::error!(
                                ptr = ptr,
                                len = len,
                                "Invalid parameters in print function"
                            );
                        }
                    } else {
                        tracing::error!("Could not access memory in print function");
                    }
                } else {
                    tracing::error!("Memory export not found in print function");
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create print function: {e}"),
                None,
                None,
            )
        })?;

    // printl(ptr: i32, len: i32) - Output text with newline
    linker
        .func_wrap(
            "env",
            "printl",
            |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
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
                                    tracing::error!(
                                        ptr = ptr,
                                        len = len,
                                        "Invalid UTF-8 data in printl function"
                                    );
                                }
                            } else {
                                tracing::error!(
                                    ptr = ptr,
                                    len = len,
                                    memory_size = data.len(),
                                    "Memory access out of bounds in printl function"
                                );
                            }
                        } else {
                            tracing::error!(
                                ptr = ptr,
                                len = len,
                                "Invalid parameters in printl function"
                            );
                        }
                    } else {
                        tracing::error!("Could not access memory in printl function");
                    }
                } else {
                    tracing::error!("Memory export not found in printl function");
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create printl function: {e}"),
                None,
                None,
            )
        })?;

    // input(prompt_ptr: i32) -> string_ptr: i32
    // Takes a pointer to a length-prefixed string (prompt)
    linker
        .func_wrap(
            "env",
            "input",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32| -> i32 {
                // Extract prompt from length-prefixed string
                let prompt = extract_length_prefixed_string(&mut caller, prompt_ptr);

                // Get user input
                if !prompt.is_empty() {
                    print!("{prompt}");
                    io::stdout().flush().unwrap();
                }

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

    // input_integer(prompt_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "input_integer",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32| -> i32 {
                let prompt = extract_length_prefixed_string(&mut caller, prompt_ptr);

                loop {
                    if !prompt.is_empty() {
                        print!("{prompt}");
                        io::stdout().flush().unwrap();
                    }

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

    // input_float(prompt_ptr: i32) -> f64
    linker
        .func_wrap(
            "env",
            "input_float",
            |mut caller: Caller<'_, ()>, prompt_ptr: i32| -> f64 {
                let prompt = extract_length_prefixed_string(&mut caller, prompt_ptr);

                loop {
                    if !prompt.is_empty() {
                        print!("{prompt}");
                        io::stdout().flush().unwrap();
                    }

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

    // input_yesno(prompt_ptr: i32) -> i32 (1 for yes, 0 for no)
    linker.func_wrap("env", "input_yesno", |mut caller: Caller<'_, ()>, prompt_ptr: i32| -> i32 {
        let prompt = extract_length_prefixed_string(&mut caller, prompt_ptr);

        loop {
            if !prompt.is_empty() {
                print!("{prompt} (y/n): ");
            } else {
                print!("(y/n): ");
            }
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

    // FIXED: string.concat(ptr1: i32, ptr2: i32) -> string_ptr: i32
    // Each pointer points to a length-prefixed string: [4-byte little-endian len][content]
    linker
        .func_wrap(
            "env",
            "string.concat",
            |mut caller: Caller<'_, ()>, ptr1: i32, ptr2: i32| -> i32 {
                let str1 = extract_length_prefixed_string(&mut caller, ptr1);
                let str2 = extract_length_prefixed_string(&mut caller, ptr2);
                let result = str1 + &str2;
                allocate_string_in_memory(&mut caller, &result)
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

    // math_asin(value: f64) -> f64
    linker
        .func_wrap("env", "math_asin", |value: f64| -> f64 { value.asin() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_asin function: {e}"),
                None,
                None,
            )
        })?;

    // math_acos(value: f64) -> f64
    linker
        .func_wrap("env", "math_acos", |value: f64| -> f64 { value.acos() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_acos function: {e}"),
                None,
                None,
            )
        })?;

    // math_atan(value: f64) -> f64
    linker
        .func_wrap("env", "math_atan", |value: f64| -> f64 { value.atan() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_atan function: {e}"),
                None,
                None,
            )
        })?;

    // math_atan2(y: f64, x: f64) -> f64
    linker
        .func_wrap("env", "math_atan2", |y: f64, x: f64| -> f64 { y.atan2(x) })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_atan2 function: {e}"),
                None,
                None,
            )
        })?;

    // math_sinh(value: f64) -> f64
    linker
        .func_wrap("env", "math_sinh", |value: f64| -> f64 { value.sinh() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_sinh function: {e}"),
                None,
                None,
            )
        })?;

    // math_cosh(value: f64) -> f64
    linker
        .func_wrap("env", "math_cosh", |value: f64| -> f64 { value.cosh() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_cosh function: {e}"),
                None,
                None,
            )
        })?;

    // math_tanh(value: f64) -> f64
    linker
        .func_wrap("env", "math_tanh", |value: f64| -> f64 { value.tanh() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_tanh function: {e}"),
                None,
                None,
            )
        })?;

    // math_ln(value: f64) -> f64 (natural logarithm)
    linker
        .func_wrap("env", "math_ln", |value: f64| -> f64 { value.ln() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_ln function: {e}"),
                None,
                None,
            )
        })?;

    // math_log10(value: f64) -> f64 (base 10 logarithm)
    linker
        .func_wrap("env", "math_log10", |value: f64| -> f64 { value.log10() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_log10 function: {e}"),
                None,
                None,
            )
        })?;

    // math_log2(value: f64) -> f64 (base 2 logarithm)
    linker
        .func_wrap("env", "math_log2", |value: f64| -> f64 { value.log2() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_log2 function: {e}"),
                None,
                None,
            )
        })?;

    // math_exp(value: f64) -> f64 (e^x)
    linker
        .func_wrap("env", "math_exp", |value: f64| -> f64 { value.exp() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_exp function: {e}"),
                None,
                None,
            )
        })?;

    // math_exp2(value: f64) -> f64 (2^x)
    linker
        .func_wrap("env", "math_exp2", |value: f64| -> f64 { value.exp2() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_exp2 function: {e}"),
                None,
                None,
            )
        })?;

    // math_trunc(value: f64) -> f64 (truncate to integer)
    linker
        .func_wrap("env", "math_trunc", |value: f64| -> f64 { value.trunc() })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_trunc function: {e}"),
                None,
                None,
            )
        })?;

    // math_random() -> f64 (random number between 0 and 1)
    linker
        .func_wrap("env", "math_random", || -> f64 {
            use rand::Rng;
            rand::thread_rng().gen::<f64>()
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_random function: {e}"),
                None,
                None,
            )
        })?;

    // math_sign(value: f64) -> f64 (sign of number: -1, 0, or 1)
    linker
        .func_wrap("env", "math_sign", |value: f64| -> f64 {
            if value > 0.0 {
                1.0
            } else if value < 0.0 {
                -1.0
            } else {
                0.0
            }
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_sign function: {e}"),
                None,
                None,
            )
        })?;

    // math_clamp(value: f64, min: f64, max: f64) -> f64
    linker
        .func_wrap(
            "env",
            "math_clamp",
            |value: f64, min: f64, max: f64| -> f64 { value.clamp(min, max) },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create math_clamp function: {e}"),
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

    // mem_scope_push() - Push a new memory scope
    linker
        .func_wrap("memory_runtime", "mem_scope_push", || {
            // Scope tracking is handled by the compiler's IR
            // This is a no-op in the runtime as scopes are compile-time constructs
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create mem_scope_push function: {e}"),
                None,
                None,
            )
        })?;

    // mem_scope_pop() - Pop a memory scope
    linker
        .func_wrap("memory_runtime", "mem_scope_pop", || {
            // Scope tracking is handled by the compiler's IR
            // This is a no-op in the runtime as scopes are compile-time constructs
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create mem_scope_pop function: {e}"),
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
    register_integer_methods(linker)?;
    register_number_methods(linker)?;
    register_boolean_methods(linker)?;
    register_string_conversion_methods(linker)?;
    register_list_methods(linker)?;
    register_string_manipulation_methods(linker)?;
    register_http_server_stubs(linker)?;
    Ok(())
}

/// integer.* method registrations
fn register_integer_methods(linker: &mut Linker<()>) -> Result<(), CompilerError> {
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

    // integer.length() - Returns number of digits in integer
    linker
        .func_wrap("env", "integer.length", |value: i32| -> i32 {
            if value == 0 {
                return 1; // Zero has 1 digit
            }

            let abs_value = value.unsigned_abs();
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

    Ok(())
}

/// number.* method registrations
fn register_number_methods(linker: &mut Linker<()>) -> Result<(), CompilerError> {
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

    Ok(())
}

/// boolean.* method registrations
fn register_boolean_methods(linker: &mut Linker<()>) -> Result<(), CompilerError> {
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

    // boolean.length() - Returns the string length of the boolean representation
    // true -> 4 ("true"), false -> 5 ("false")
    linker
        .func_wrap("env", "boolean.length", |value: i32| -> i32 {
            if value != 0 {
                4
            } else {
                5
            }
        })
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create boolean.length function: {e}"),
                None,
                None,
            )
        })?;

    Ok(())
}

/// string.* type-conversion and length method registrations
fn register_string_conversion_methods(linker: &mut Linker<()>) -> Result<(), CompilerError> {
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
                                            if !string_value.is_empty() {
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

    Ok(())
}

/// list.* method registrations
fn register_list_methods(linker: &mut Linker<()>) -> Result<(), CompilerError> {
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

    Ok(())
}

/// string manipulation method registrations (case, search, split, trim, compare, etc.)
fn register_string_manipulation_methods(linker: &mut Linker<()>) -> Result<(), CompilerError> {
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

    // string.split(string_ptr: i32, delimiter_ptr: i32) -> list_ptr: i32
    // Splits a string by delimiter and returns pointer to a list of strings
    linker
        .func_wrap(
            "env",
            "string.split",
            |mut caller: Caller<'_, ()>, string_ptr: i32, delimiter_ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let delimiter = extract_length_prefixed_string(&mut caller, delimiter_ptr);

                // Split the string
                let parts: Vec<&str> = if delimiter.is_empty() {
                    // Split by characters if delimiter is empty
                    string_val.split("").filter(|s| !s.is_empty()).collect()
                } else {
                    string_val.split(&delimiter).collect()
                };

                // Allocate a list in memory
                // List layout: [size(4)][capacity(4)][type_id(4)][padding(4)][elements...]
                // Each element is a pointer to a length-prefixed string
                let num_elements = parts.len();
                let list_header_size = 16; // 4 + 4 + 4 + 4
                let elements_size = num_elements * 4; // Each element is an i32 pointer
                let total_size = list_header_size + elements_size;

                if let Some(memory) = caller.get_export("memory") {
                    if let Some(memory) = memory.into_memory() {
                        // Allocate memory for the list
                        static mut NEXT_LIST_ALLOC: i32 = 32768; // Start list allocations at 32KB
                        unsafe {
                            let list_ptr = NEXT_LIST_ALLOC;
                            NEXT_LIST_ALLOC += total_size as i32 + 8; // Add padding

                            // Check if we need to grow memory
                            let current_size = memory.data_size(&caller) as i32;
                            if NEXT_LIST_ALLOC > current_size {
                                let pages_needed =
                                    ((NEXT_LIST_ALLOC - current_size) + 65535) / 65536;
                                if memory.grow(&mut caller, pages_needed as u64).is_err() {
                                    return 0;
                                }
                            }

                            let data = memory.data_mut(&mut caller);

                            // Write list header
                            // size (number of elements)
                            let size_bytes = (num_elements as i32).to_le_bytes();
                            data[list_ptr as usize..(list_ptr + 4) as usize]
                                .copy_from_slice(&size_bytes);

                            // capacity (same as size for now)
                            data[(list_ptr + 4) as usize..(list_ptr + 8) as usize]
                                .copy_from_slice(&size_bytes);

                            // type_id (string type = 4)
                            let type_id_bytes = 4i32.to_le_bytes();
                            data[(list_ptr + 8) as usize..(list_ptr + 12) as usize]
                                .copy_from_slice(&type_id_bytes);

                            // padding
                            data[(list_ptr + 12) as usize..(list_ptr + 16) as usize]
                                .copy_from_slice(&[0, 0, 0, 0]);

                            // Allocate each string and store pointer in list
                            for (i, part) in parts.iter().enumerate() {
                                let str_ptr = allocate_string_in_memory(&mut caller, part);
                                let element_offset = list_ptr + 16 + (i as i32 * 4);

                                // Need to get memory again after allocation
                                if let Some(mem) = caller.get_export("memory") {
                                    if let Some(mem) = mem.into_memory() {
                                        let data = mem.data_mut(&mut caller);
                                        let ptr_bytes = str_ptr.to_le_bytes();
                                        data[element_offset as usize
                                            ..(element_offset + 4) as usize]
                                            .copy_from_slice(&ptr_bytes);
                                    }
                                }
                            }

                            return list_ptr;
                        }
                    }
                }
                0 // Return 0 on failure
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.split function: {e}"),
                None,
                None,
            )
        })?;

    // string_trim(ptr: i32) -> i32 - Remove leading and trailing whitespace
    linker
        .func_wrap(
            "env",
            "string_trim",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                let trimmed = string_val.trim();
                allocate_string_in_memory(&mut caller, trimmed)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_trim function: {e}"),
                None,
                None,
            )
        })?;

    // string_trim_start(ptr: i32) -> i32 - Remove leading whitespace
    linker
        .func_wrap(
            "env",
            "string_trim_start",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                let trimmed = string_val.trim_start();
                allocate_string_in_memory(&mut caller, trimmed)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_trim_start function: {e}"),
                None,
                None,
            )
        })?;

    // string_trim_end(ptr: i32) -> i32 - Remove trailing whitespace
    linker
        .func_wrap(
            "env",
            "string_trim_end",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                let trimmed = string_val.trim_end();
                allocate_string_in_memory(&mut caller, trimmed)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_trim_end function: {e}"),
                None,
                None,
            )
        })?;

    // string_compare(ptr1: i32, ptr2: i32) -> i32 - Compare two strings
    // Returns 0 if equal, 1 if not equal (C/strcmp convention; codegen uses i32.eqz to test equality)
    linker
        .func_wrap(
            "env",
            "string_compare",
            |mut caller: Caller<'_, ()>, ptr1: i32, ptr2: i32| -> i32 {
                let string1 = extract_length_prefixed_string(&mut caller, ptr1);
                let string2 = extract_length_prefixed_string(&mut caller, ptr2);
                if string1 == string2 {
                    0
                } else {
                    1
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_compare function: {e}"),
                None,
                None,
            )
        })?;

    // string.indexOf(string_ptr: i32, search_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.indexOf",
            |mut caller: Caller<'_, ()>, string_ptr: i32, search_ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let search = extract_length_prefixed_string(&mut caller, search_ptr);
                match string_val.find(&search) {
                    Some(pos) => pos as i32,
                    None => -1,
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.indexOf function: {e}"),
                None,
                None,
            )
        })?;

    // string.lastIndexOf(string_ptr: i32, search_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.lastIndexOf",
            |mut caller: Caller<'_, ()>, string_ptr: i32, search_ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let search = extract_length_prefixed_string(&mut caller, search_ptr);
                match string_val.rfind(&search) {
                    Some(pos) => pos as i32,
                    None => -1,
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.lastIndexOf function: {e}"),
                None,
                None,
            )
        })?;

    // string.substring(ptr: i32, start: i32, end: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.substring",
            |mut caller: Caller<'_, ()>, ptr: i32, start: i32, end: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                let len = string_val.len() as i32;
                let start = start.max(0).min(len) as usize;
                let end = end.max(0).min(len) as usize;
                let start = start.min(end);
                let substring = &string_val[start..end];
                allocate_string_in_memory(&mut caller, substring)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.substring function: {e}"),
                None,
                None,
            )
        })?;

    // string.charAt(ptr: i32, index: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.charAt",
            |mut caller: Caller<'_, ()>, ptr: i32, index: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                if index >= 0 && (index as usize) < string_val.len() {
                    let ch = string_val.chars().nth(index as usize).unwrap_or('\0');
                    allocate_string_in_memory(&mut caller, &ch.to_string())
                } else {
                    allocate_string_in_memory(&mut caller, "")
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.charAt function: {e}"),
                None,
                None,
            )
        })?;

    // string.startsWith(string_ptr: i32, prefix_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.startsWith",
            |mut caller: Caller<'_, ()>, string_ptr: i32, prefix_ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let prefix = extract_length_prefixed_string(&mut caller, prefix_ptr);
                if string_val.starts_with(&prefix) {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.startsWith function: {e}"),
                None,
                None,
            )
        })?;

    // string.endsWith(string_ptr: i32, suffix_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.endsWith",
            |mut caller: Caller<'_, ()>, string_ptr: i32, suffix_ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let suffix = extract_length_prefixed_string(&mut caller, suffix_ptr);
                if string_val.ends_with(&suffix) {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.endsWith function: {e}"),
                None,
                None,
            )
        })?;

    // string.includes(string_ptr: i32, search_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.includes",
            |mut caller: Caller<'_, ()>, string_ptr: i32, search_ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let search = extract_length_prefixed_string(&mut caller, search_ptr);
                if string_val.contains(&search) {
                    1
                } else {
                    0
                }
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.includes function: {e}"),
                None,
                None,
            )
        })?;

    // string.replace(string_ptr: i32, search_ptr: i32, replace_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.replace",
            |mut caller: Caller<'_, ()>,
             string_ptr: i32,
             search_ptr: i32,
             replace_ptr: i32|
             -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let search = extract_length_prefixed_string(&mut caller, search_ptr);
                let replace = extract_length_prefixed_string(&mut caller, replace_ptr);
                let result = string_val.replace(&search, &replace);
                allocate_string_in_memory(&mut caller, &result)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.replace function: {e}"),
                None,
                None,
            )
        })?;

    // string_replace alias (underscore version)
    linker
        .func_wrap(
            "env",
            "string_replace",
            |mut caller: Caller<'_, ()>,
             string_ptr: i32,
             search_ptr: i32,
             replace_ptr: i32|
             -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let search = extract_length_prefixed_string(&mut caller, search_ptr);
                let replace = extract_length_prefixed_string(&mut caller, replace_ptr);
                let result = string_val.replace(&search, &replace);
                allocate_string_in_memory(&mut caller, &result)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string_replace function: {e}"),
                None,
                None,
            )
        })?;

    // string.replaceFirst(string_ptr: i32, search_ptr: i32, replace_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.replaceFirst",
            |mut caller: Caller<'_, ()>,
             string_ptr: i32,
             search_ptr: i32,
             replace_ptr: i32|
             -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, string_ptr);
                let search = extract_length_prefixed_string(&mut caller, search_ptr);
                let replace = extract_length_prefixed_string(&mut caller, replace_ptr);
                let result = string_val.replacen(&search, &replace, 1);
                allocate_string_in_memory(&mut caller, &result)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.replaceFirst function: {e}"),
                None,
                None,
            )
        })?;

    // string.repeat(ptr: i32, count: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.repeat",
            |mut caller: Caller<'_, ()>, ptr: i32, count: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                let count = count.max(0) as usize;
                let result = string_val.repeat(count);
                allocate_string_in_memory(&mut caller, &result)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.repeat function: {e}"),
                None,
                None,
            )
        })?;

    // string.padStart(ptr: i32, target_len: i32, pad_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.padStart",
            |mut caller: Caller<'_, ()>, ptr: i32, target_len: i32, pad_ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                let pad_char = extract_length_prefixed_string(&mut caller, pad_ptr);
                let target_len = target_len.max(0) as usize;
                let current_len = string_val.len();

                if current_len >= target_len {
                    return ptr; // Already at or exceeds target length
                }

                let pad_str = if pad_char.is_empty() { " " } else { &pad_char };
                let padding_needed = target_len - current_len;
                let mut result = String::new();

                // Add padding
                while result.len() < padding_needed {
                    result.push_str(pad_str);
                }
                result.truncate(padding_needed);
                result.push_str(&string_val);

                allocate_string_in_memory(&mut caller, &result)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.padStart function: {e}"),
                None,
                None,
            )
        })?;

    // string.padEnd(ptr: i32, target_len: i32, pad_ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.padEnd",
            |mut caller: Caller<'_, ()>, ptr: i32, target_len: i32, pad_ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                let pad_char = extract_length_prefixed_string(&mut caller, pad_ptr);
                let target_len = target_len.max(0) as usize;
                let current_len = string_val.len();

                if current_len >= target_len {
                    return ptr; // Already at or exceeds target length
                }

                let pad_str = if pad_char.is_empty() { " " } else { &pad_char };
                let mut result = string_val.clone();

                // Add padding
                while result.len() < target_len {
                    result.push_str(pad_str);
                }
                result.truncate(target_len);

                allocate_string_in_memory(&mut caller, &result)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.padEnd function: {e}"),
                None,
                None,
            )
        })?;

    // string.reverse(ptr: i32) -> i32
    linker
        .func_wrap(
            "env",
            "string.reverse",
            |mut caller: Caller<'_, ()>, ptr: i32| -> i32 {
                let string_val = extract_length_prefixed_string(&mut caller, ptr);
                let reversed: String = string_val.chars().rev().collect();
                allocate_string_in_memory(&mut caller, &reversed)
            },
        )
        .map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to create string.reverse function: {e}"),
                None,
                None,
            )
        })?;

    Ok(())
}

/// HTTP server stub registrations (required for compilation but not implemented in standalone runner)
fn register_http_server_stubs(linker: &mut Linker<()>) -> Result<(), CompilerError> {
    // Signatures must match what the codegen produces
    linker
        .func_wrap(
            "env",
            "_http_route",
            |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
        )
        .ok();
    linker
        .func_wrap("env", "_http_listen", |_: i32| -> i32 { 0 })
        .ok();
    linker
        .func_wrap("env", "_req_param", |_: i32, _: i32| -> i32 { 0 })
        .ok();
    linker
        .func_wrap("env", "_req_query", |_: i32, _: i32| -> i32 { 0 })
        .ok();
    linker.func_wrap("env", "_req_body", || -> i32 { 0 }).ok();
    linker
        .func_wrap("env", "_req_header", |_: i32, _: i32| -> i32 { 0 })
        .ok();
    linker.func_wrap("env", "_req_method", || -> i32 { 0 }).ok();
    linker.func_wrap("env", "_req_path", || -> i32 { 0 }).ok();
    linker
        .func_wrap("env", "_req_cookie", |_: i32, _: i32| -> i32 { 0 })
        .ok();
    linker
        .func_wrap(
            "env",
            "_http_route_protected",
            |_: i32, _: i32, _: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
        )
        .ok();
    // Session: _session_store(user_id: i32, rolePtr: i32, roleLen: i32, claimsPtr: i32, claimsLen: i32) -> i32
    linker
        .func_wrap(
            "env",
            "_session_store",
            |_: i32, _: i32, _: i32, _: i32, _: i32| -> i32 { 0 },
        )
        .ok();
    // Session: _session_get() -> i32
    linker
        .func_wrap("env", "_session_get", || -> i32 { 0 })
        .ok();
    // Session: _session_delete() -> i32
    linker
        .func_wrap("env", "_session_delete", || -> i32 { 0 })
        .ok();
    // Session: _http_set_cookie(cookiePtr: i32, cookieLen: i32) -> i32
    linker
        .func_wrap("env", "_http_set_cookie", |_: i32, _: i32| -> i32 { 0 })
        .ok();
    // Auth: _auth_get_session() -> i32
    linker
        .func_wrap("env", "_auth_get_session", || -> i32 { 0 })
        .ok();
    // Auth: _auth_require_auth() -> i32
    linker
        .func_wrap("env", "_auth_require_auth", || -> i32 { 0 })
        .ok();
    // Auth: _auth_require_role(rolePtr: i32, roleLen: i32) -> i32
    linker
        .func_wrap("env", "_auth_require_role", |_: i32, _: i32| -> i32 { 0 })
        .ok();
    // Auth: _auth_can(permPtr: i32, permLen: i32) -> i32
    linker
        .func_wrap("env", "_auth_can", |_: i32, _: i32| -> i32 { 0 })
        .ok();
    // Auth: _auth_has_any_role(rolesPtr: i32, rolesLen: i32) -> i32
    linker
        .func_wrap("env", "_auth_has_any_role", |_: i32, _: i32| -> i32 { 0 })
        .ok();

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

/// Utility function to extract a length-prefixed string from WASM memory
/// String format: [4-byte little-endian length][content bytes]
fn extract_length_prefixed_string(caller: &mut Caller<'_, ()>, ptr: i32) -> String {
    if let Some(memory) = caller.get_export("memory") {
        if let Some(memory) = memory.into_memory() {
            let data = memory.data(caller);
            if ptr >= 0 {
                let p = ptr as usize;
                // Read 4-byte length prefix
                if p + 4 <= data.len() {
                    let len = u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]])
                        as usize;
                    let content_start = p + 4;
                    let content_end = content_start + len;
                    if content_end <= data.len() {
                        if let Ok(s) = std::str::from_utf8(&data[content_start..content_end]) {
                            return s.to_string();
                        }
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
                let current_size = memory.data_size(&*caller);
                let needed_size = (ptr + total_size) as usize;

                if needed_size > current_size {
                    let pages_needed = (needed_size - current_size).div_ceil(65536);
                    if memory.grow(&mut *caller, pages_needed as u64).is_err() {
                        tracing::error!(
                            pages_needed = pages_needed,
                            current_size = current_size,
                            needed_size = needed_size,
                            "Memory growth failed"
                        );
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

    tracing::error!(string_len = string_value.len(), "String allocation failed");
    0 // Allocation failed
}
