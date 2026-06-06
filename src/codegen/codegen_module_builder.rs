//! Module assembly and import registration for the `CodeGenerator`.
//! Contains import registration (file, HTTP, print, console, type conversion,
//! method-style), string pool management, and stdlib function registration helpers.

use crate::error::CompilerError;
use crate::types::WasmType;
use std::collections::HashSet;
use wasm_encoder::{Function, Instruction};

impl super::CodeGenerator {
    /// Emit a WASM import subject to the Import Minimality Rule
    /// (see foundation/platform-architecture/EXECUTION_LAYERS.md).
    ///
    /// Returns `true` if the import was emitted and the caller should
    /// record its function index (via `self.function_count`) and then
    /// increment `self.function_count`. Returns `false` if the import
    /// was filtered out as a reachability-gated unused Layer 2/3 function,
    /// in which case the caller MUST NOT touch `function_count` or any
    /// index map for that import.
    pub(crate) fn emit_import(
        &mut self,
        module: &str,
        field: &str,
        entity: wasm_encoder::EntityType,
    ) -> bool {
        if let Some(reachable) = &self.reachable_imports {
            if super::is_reachability_gated_import(field) && !reachable.contains(field) {
                tracing::debug!(
                    function = field,
                    "Skipping unused reachability-gated import (tree-shake)"
                );
                return false;
            }
        }
        self.import_section.import(module, field, entity);
        true
    }

    /// Add a string to the string pool and return its pointer
    pub fn add_string_to_pool(&mut self, string: &str) -> u32 {
        self.memory_utils
            .allocate_string(string)
            .unwrap_or_default() as u32
    }

    pub(crate) fn get_or_create_string_offset(&mut self, s: &str) -> Result<u32, CompilerError> {
        // Check if string already exists in pool
        if let Some(&existing_offset) = self.string_pool.get(s) {
            return Ok(existing_offset);
        }

        // Create new string entry WITH length prefix for proper runtime format
        // String format in memory: [4-byte length][string content]
        let string_bytes = s.as_bytes();
        let string_len = string_bytes.len() as u32;
        let current_offset = self.string_offset_counter;

        // Build string data with 4-byte little-endian length prefix
        let mut string_data = Vec::with_capacity(4 + string_bytes.len());
        string_data.extend_from_slice(&string_len.to_le_bytes()); // 4-byte length
        string_data.extend_from_slice(string_bytes); // String content

        // Add the complete string structure to the data section
        self.memory_utils
            .add_data_segment(current_offset, &string_data)
            .map_err(|e| {
                CompilerError::codegen_error(
                    format!("Failed to add string '{}' to data section: {:?}", s, e),
                    None,
                    None,
                )
            })?;

        // Update offset counter with padding for next string
        // NOTE: Ensure 4-byte alignment for next string
        let total_size = 4 + string_bytes.len() as u32; // length prefix + content
        let string_size_with_padding = total_size + 16;
        let aligned_size = (string_size_with_padding + 3) & !3; // Round up to nearest multiple of 4
        self.string_offset_counter += aligned_size;

        // Store in string pool for reuse
        self.string_pool.insert(s.to_string(), current_offset);

        Ok(current_offset)
    }

    /// Register file system import functions
    pub(crate) fn register_file_imports(&mut self) -> Result<(), CompilerError> {
        // file_write(pathPtr: i32, pathLen: i32, contentPtr: i32, contentLen: i32) -> i32
        let write_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "file_write",
            wasm_encoder::EntityType::Function(write_type),
        ) {
            let write_index = self.function_count;
            self.file_import_indices
                .insert("file_write".to_string(), write_index);
            self.function_map
                .insert("file.write".to_string(), write_index);
            self.function_count += 1;
        }

        // file_read(pathPtr: i32, pathLen: i32, resultPtr: i32) -> i32 (returns length or -1 for error)
        let read_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "file_read",
            wasm_encoder::EntityType::Function(read_type),
        ) {
            self.file_import_indices
                .insert("file_read".to_string(), self.function_count);
            self.function_count += 1;
        }

        // file_exists(pathPtr: i32, pathLen: i32) -> i32 (returns 1 if exists, 0 if not)
        let exists_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "file_exists",
            wasm_encoder::EntityType::Function(exists_type),
        ) {
            let exists_index = self.function_count;
            self.file_import_indices
                .insert("file_exists".to_string(), exists_index);
            self.function_map
                .insert("file.exists".to_string(), exists_index);
            self.function_count += 1;
        }

        // file_delete(pathPtr: i32, pathLen: i32) -> i32 (returns 0 for success, -1 for error)
        let delete_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "file_delete",
            wasm_encoder::EntityType::Function(delete_type),
        ) {
            self.file_import_indices
                .insert("file_delete".to_string(), self.function_count);
            self.function_count += 1;
        }

        // file_append(pathPtr: i32, pathLen: i32, contentPtr: i32, contentLen: i32) -> i32
        let append_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "file_append",
            wasm_encoder::EntityType::Function(append_type),
        ) {
            self.file_import_indices
                .insert("file_append".to_string(), self.function_count);
            self.function_count += 1;
        }

        Ok(())
    }

    /// Register HTTP client import functions
    ///
    /// `skip_functions` contains function names that should NOT be registered as imports
    /// because they will be handled by plugin bridge functions with expand_strings wrappers.
    ///
    /// `include_server_imports` controls whether HTTP server imports (_http_route, _http_listen,
    /// _req_param, etc.) are registered. These should only be true when compiling web server
    /// applications that use plugins like Frame. Default should be false for CLI/library code.
    pub fn register_http_imports(
        &mut self,
        skip_functions: &HashSet<String>,
        include_server_imports: bool,
    ) -> Result<(), CompilerError> {
        // Basic HTTP methods

        // http_get(urlPtr: i32, urlLen: i32) -> i32 (returns string pointer)
        let get_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_get",
            wasm_encoder::EntityType::Function(get_type),
        ) {
            let get_index = self.function_count;
            self.http_import_indices
                .insert("http_get".to_string(), get_index);
            self.function_map.insert("http.get".to_string(), get_index);
            self.function_count += 1;
        }

        // http_post(urlPtr: i32, urlLen: i32, bodyPtr: i32, bodyLen: i32) -> i32 (returns string pointer)
        let post_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_post",
            wasm_encoder::EntityType::Function(post_type),
        ) {
            self.http_import_indices
                .insert("http_post".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_put(urlPtr: i32, urlLen: i32, bodyPtr: i32, bodyLen: i32) -> i32 (returns string pointer)
        let put_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_put",
            wasm_encoder::EntityType::Function(put_type),
        ) {
            self.http_import_indices
                .insert("http_put".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_patch(urlPtr: i32, urlLen: i32, bodyPtr: i32, bodyLen: i32) -> i32 (returns string pointer)
        let patch_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_patch",
            wasm_encoder::EntityType::Function(patch_type),
        ) {
            self.http_import_indices
                .insert("http_patch".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_delete(urlPtr: i32, urlLen: i32) -> i32 (returns string pointer)
        let delete_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_delete",
            wasm_encoder::EntityType::Function(delete_type),
        ) {
            self.http_import_indices
                .insert("http_delete".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_head(urlPtr: i32, urlLen: i32) -> i32 (returns headers string pointer)
        let head_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_head",
            wasm_encoder::EntityType::Function(head_type),
        ) {
            self.http_import_indices
                .insert("http_head".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_options(urlPtr: i32, urlLen: i32) -> i32 (returns options string pointer)
        let options_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_options",
            wasm_encoder::EntityType::Function(options_type),
        ) {
            self.http_import_indices
                .insert("http_options".to_string(), self.function_count);
            self.function_count += 1;
        }

        // Advanced HTTP methods with headers

        // http_get_with_headers(urlPtr: i32, urlLen: i32, headersPtr: i32, headersLen: i32) -> i32
        let get_headers_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_get_with_headers",
            wasm_encoder::EntityType::Function(get_headers_type),
        ) {
            self.http_import_indices
                .insert("http_get_with_headers".to_string(), self.function_count);
            self.function_map
                .insert("http.getWithHeaders".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_post_with_headers(urlPtr: i32, urlLen: i32, bodyPtr: i32, bodyLen: i32, headersPtr: i32, headersLen: i32) -> i32
        let post_headers_type = self.add_function_type(
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_post_with_headers",
            wasm_encoder::EntityType::Function(post_headers_type),
        ) {
            self.http_import_indices
                .insert("http_post_with_headers".to_string(), self.function_count);
            self.function_map
                .insert("http.postWithHeaders".to_string(), self.function_count);
            self.function_count += 1;
        }

        // JSON methods

        // http_post_json(urlPtr: i32, urlLen: i32, jsonPtr: i32, jsonLen: i32) -> i32
        let post_json_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_post_json",
            wasm_encoder::EntityType::Function(post_json_type),
        ) {
            self.http_import_indices
                .insert("http_post_json".to_string(), self.function_count);
            self.function_map
                .insert("http.postJson".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_put_json(urlPtr: i32, urlLen: i32, jsonPtr: i32, jsonLen: i32) -> i32
        let put_json_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_put_json",
            wasm_encoder::EntityType::Function(put_json_type),
        ) {
            self.http_import_indices
                .insert("http_put_json".to_string(), self.function_count);
            self.function_map
                .insert("http.putJson".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_patch_json(urlPtr: i32, urlLen: i32, jsonPtr: i32, jsonLen: i32) -> i32
        let patch_json_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_patch_json",
            wasm_encoder::EntityType::Function(patch_json_type),
        ) {
            self.http_import_indices
                .insert("http_patch_json".to_string(), self.function_count);
            self.function_count += 1;
        }

        // Form data method

        // http_post_form(urlPtr: i32, urlLen: i32, formPtr: i32, formLen: i32) -> i32
        let post_form_type = self.add_function_type(
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        if self.emit_import(
            "env",
            "http_post_form",
            wasm_encoder::EntityType::Function(post_form_type),
        ) {
            self.http_import_indices
                .insert("http_post_form".to_string(), self.function_count);
            self.function_map
                .insert("http.postForm".to_string(), self.function_count);
            self.function_count += 1;
        }

        // Configuration methods

        // http_set_user_agent(agentPtr: i32, agentLen: i32) -> void
        let set_agent_type = self.add_function_type(&[WasmType::I32, WasmType::I32], None)?;
        if self.emit_import(
            "env",
            "http_set_user_agent",
            wasm_encoder::EntityType::Function(set_agent_type),
        ) {
            self.http_import_indices
                .insert("http_set_user_agent".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_set_timeout(timeoutMs: i32) -> void
        let set_timeout_type = self.add_function_type(&[WasmType::I32], None)?;
        if self.emit_import(
            "env",
            "http_set_timeout",
            wasm_encoder::EntityType::Function(set_timeout_type),
        ) {
            self.http_import_indices
                .insert("http_set_timeout".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_set_max_redirects(maxRedirects: i32) -> void
        let set_redirects_type = self.add_function_type(&[WasmType::I32], None)?;
        if self.emit_import(
            "env",
            "http_set_max_redirects",
            wasm_encoder::EntityType::Function(set_redirects_type),
        ) {
            self.http_import_indices
                .insert("http_set_max_redirects".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_enable_cookies(enable: i32) -> void
        let enable_cookies_type = self.add_function_type(&[WasmType::I32], None)?;
        if self.emit_import(
            "env",
            "http_enable_cookies",
            wasm_encoder::EntityType::Function(enable_cookies_type),
        ) {
            self.http_import_indices
                .insert("http_enable_cookies".to_string(), self.function_count);
            self.function_count += 1;
        }

        // Response information methods

        // http_get_response_code() -> i32
        let get_code_type = self.add_function_type(&[], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_get_response_code",
            wasm_encoder::EntityType::Function(get_code_type),
        ) {
            self.http_import_indices
                .insert("http_get_response_code".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_get_response_body() -> i32 (returns string pointer)
        let get_body_type = self.add_function_type(&[], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_get_response_body",
            wasm_encoder::EntityType::Function(get_body_type),
        ) {
            self.http_import_indices
                .insert("http_get_response_body".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_get_response_headers() -> i32 (returns string pointer)
        let get_headers_type = self.add_function_type(&[], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_get_response_headers",
            wasm_encoder::EntityType::Function(get_headers_type),
        ) {
            self.http_import_indices
                .insert("http_get_response_headers".to_string(), self.function_count);
            self.function_count += 1;
        }

        // Utility methods

        // http_encode_url(urlPtr: i32, urlLen: i32) -> i32 (returns encoded string pointer)
        let encode_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_encode_url",
            wasm_encoder::EntityType::Function(encode_type),
        ) {
            self.http_import_indices
                .insert("http_encode_url".to_string(), self.function_count);
            self.function_map
                .insert("http.encodeUrl".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_decode_url(urlPtr: i32, urlLen: i32) -> i32 (returns decoded string pointer)
        let decode_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_decode_url",
            wasm_encoder::EntityType::Function(decode_type),
        ) {
            self.http_import_indices
                .insert("http_decode_url".to_string(), self.function_count);
            self.function_map
                .insert("http.decodeUrl".to_string(), self.function_count);
            self.function_count += 1;
        }

        // http_build_query(paramsPtr: i32, paramsLen: i32) -> i32 (returns query string pointer)
        let build_query_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "http_build_query",
            wasm_encoder::EntityType::Function(build_query_type),
        ) {
            self.http_import_indices
                .insert("http_build_query".to_string(), self.function_count);
            self.function_map
                .insert("http.buildQuery".to_string(), self.function_count);
            self.function_count += 1;
        }

        // =========================================
        // HTTP Server functions (for Frame runtime)
        // Only include if explicitly requested (e.g., when using web framework plugins)
        // =========================================
        if include_server_imports {
            // _http_route(methodPtr: i32, methodLen: i32, pathPtr: i32, pathLen: i32, handlerIdx: i32) -> i32
            // Skip if plugin bridge will handle this function with expand_strings wrapper
            if !skip_functions.contains("_http_route") {
                let route_type = self.add_function_type(
                    &[
                        WasmType::I32,
                        WasmType::I32,
                        WasmType::I32,
                        WasmType::I32,
                        WasmType::I32,
                    ],
                    Some(WasmType::I32),
                )?;
                if self.emit_import(
                    "env",
                    "_http_route",
                    wasm_encoder::EntityType::Function(route_type),
                ) {
                    let route_index = self.function_count;
                    self.http_import_indices
                        .insert("_http_route".to_string(), route_index);
                    // Also add to function_map for MIR codegen lookup
                    self.function_map
                        .insert("_http_route".to_string(), route_index);
                    self.function_count += 1;
                }
            }

            // _http_listen(port: i32) -> i32
            let listen_type = self.add_function_type(&[WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_http_listen",
                wasm_encoder::EntityType::Function(listen_type),
            ) {
                let listen_index = self.function_count;
                self.http_import_indices
                    .insert("_http_listen".to_string(), listen_index);
                // Also add to function_map for MIR codegen lookup
                self.function_map
                    .insert("_http_listen".to_string(), listen_index);
                self.function_count += 1;
            }

            // =========================================
            // Request context access functions
            // =========================================

            // _req_param(namePtr: i32, nameLen: i32) -> i32 (returns string pointer)
            let req_param_type =
                self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_req_param",
                wasm_encoder::EntityType::Function(req_param_type),
            ) {
                self.http_import_indices
                    .insert("_req_param".to_string(), self.function_count);
                self.function_map
                    .insert("_req_param".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _req_query(namePtr: i32, nameLen: i32) -> i32 (returns string pointer)
            let req_query_type =
                self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_req_query",
                wasm_encoder::EntityType::Function(req_query_type),
            ) {
                self.http_import_indices
                    .insert("_req_query".to_string(), self.function_count);
                self.function_map
                    .insert("_req_query".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _req_body() -> i32 (returns string pointer)
            let req_body_type = self.add_function_type(&[], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_req_body",
                wasm_encoder::EntityType::Function(req_body_type),
            ) {
                self.http_import_indices
                    .insert("_req_body".to_string(), self.function_count);
                self.function_map
                    .insert("_req_body".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _req_header(namePtr: i32, nameLen: i32) -> i32 (returns string pointer)
            let req_header_type =
                self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_req_header",
                wasm_encoder::EntityType::Function(req_header_type),
            ) {
                self.http_import_indices
                    .insert("_req_header".to_string(), self.function_count);
                self.function_map
                    .insert("_req_header".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _req_method() -> i32 (returns string pointer)
            let req_method_type = self.add_function_type(&[], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_req_method",
                wasm_encoder::EntityType::Function(req_method_type),
            ) {
                self.http_import_indices
                    .insert("_req_method".to_string(), self.function_count);
                self.function_map
                    .insert("_req_method".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _req_path() -> i32 (returns string pointer)
            let req_path_type = self.add_function_type(&[], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_req_path",
                wasm_encoder::EntityType::Function(req_path_type),
            ) {
                self.http_import_indices
                    .insert("_req_path".to_string(), self.function_count);
                self.function_map
                    .insert("_req_path".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _req_cookie(namePtr: i32, nameLen: i32) -> i32 (returns string pointer)
            let req_cookie_type =
                self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_req_cookie",
                wasm_encoder::EntityType::Function(req_cookie_type),
            ) {
                self.http_import_indices
                    .insert("_req_cookie".to_string(), self.function_count);
                self.function_map
                    .insert("_req_cookie".to_string(), self.function_count);
                self.function_count += 1;
            }

            // =========================================
            // Protected route registration
            // =========================================

            // _http_route_protected(methodPtr: i32, methodLen: i32, pathPtr: i32, pathLen: i32, handlerIdx: i32, rolePtr: i32, roleLen: i32) -> i32
            if !skip_functions.contains("_http_route_protected") {
                let route_protected_type = self.add_function_type(
                    &[
                        WasmType::I32,
                        WasmType::I32, // method
                        WasmType::I32,
                        WasmType::I32, // path
                        WasmType::I32, // handler_idx
                        WasmType::I32,
                        WasmType::I32, // role
                    ],
                    Some(WasmType::I32),
                )?;
                if self.emit_import(
                    "env",
                    "_http_route_protected",
                    wasm_encoder::EntityType::Function(route_protected_type),
                ) {
                    self.http_import_indices
                        .insert("_http_route_protected".to_string(), self.function_count);
                    self.function_map
                        .insert("_http_route_protected".to_string(), self.function_count);
                    self.function_count += 1;
                }
            }

            // =========================================
            // Session management functions
            // =========================================

            // _session_store(user_id: i32, rolePtr: i32, roleLen: i32, claimsPtr: i32, claimsLen: i32) -> i32 (returns session_id string pointer)
            let session_create_type = self.add_function_type(
                &[
                    WasmType::I32, // user_id (Clean Language integer = i32)
                    WasmType::I32,
                    WasmType::I32, // role
                    WasmType::I32,
                    WasmType::I32, // claims
                ],
                Some(WasmType::I32),
            )?;
            if self.emit_import(
                "env",
                "_session_store",
                wasm_encoder::EntityType::Function(session_create_type),
            ) {
                self.http_import_indices
                    .insert("_session_store".to_string(), self.function_count);
                self.function_map
                    .insert("_session_store".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _session_get() -> i32 (returns session JSON string pointer)
            let session_get_type = self.add_function_type(&[], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_session_get",
                wasm_encoder::EntityType::Function(session_get_type),
            ) {
                self.http_import_indices
                    .insert("_session_get".to_string(), self.function_count);
                self.function_map
                    .insert("_session_get".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _session_delete() -> i32 (returns 1 if deleted, 0 if not)
            let session_destroy_type = self.add_function_type(&[], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_session_delete",
                wasm_encoder::EntityType::Function(session_destroy_type),
            ) {
                self.http_import_indices
                    .insert("_session_delete".to_string(), self.function_count);
                self.function_map
                    .insert("_session_delete".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _http_set_cookie(cookiePtr: i32, cookieLen: i32) -> i32
            let session_set_cookie_type =
                self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_http_set_cookie",
                wasm_encoder::EntityType::Function(session_set_cookie_type),
            ) {
                self.http_import_indices
                    .insert("_http_set_cookie".to_string(), self.function_count);
                self.function_map
                    .insert("_http_set_cookie".to_string(), self.function_count);
                self.function_count += 1;
            }

            // =========================================
            // Authentication context functions
            // =========================================

            // _auth_get_session() -> i32 (returns session JSON string pointer)
            let auth_get_session_type = self.add_function_type(&[], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_auth_get_session",
                wasm_encoder::EntityType::Function(auth_get_session_type),
            ) {
                let idx = self.function_count;
                self.http_import_indices
                    .insert("_auth_get_session".to_string(), idx);
                self.function_map
                    .insert("_auth_get_session".to_string(), idx);
                self.function_map.insert("auth.getSession".to_string(), idx);
                self.function_count += 1;
            }

            // _auth_require_auth() -> i32 (returns 1 if authenticated, 0 if not)
            let auth_require_auth_type = self.add_function_type(&[], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_auth_require_auth",
                wasm_encoder::EntityType::Function(auth_require_auth_type),
            ) {
                let idx = self.function_count;
                self.http_import_indices
                    .insert("_auth_require_auth".to_string(), idx);
                self.function_map
                    .insert("_auth_require_auth".to_string(), idx);
                self.function_map
                    .insert("auth.requireAuth".to_string(), idx);
                self.function_count += 1;
            }

            // _auth_require_role(rolePtr: i32, roleLen: i32) -> i32 (returns 1 if has role, 0 if not)
            let auth_require_role_type =
                self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_auth_require_role",
                wasm_encoder::EntityType::Function(auth_require_role_type),
            ) {
                let idx = self.function_count;
                self.http_import_indices
                    .insert("_auth_require_role".to_string(), idx);
                self.function_map
                    .insert("_auth_require_role".to_string(), idx);
                self.function_map
                    .insert("auth.requireRole".to_string(), idx);
                self.function_count += 1;
            }

            // _auth_can(permissionPtr: i32, permissionLen: i32) -> i32
            let auth_can_type =
                self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_auth_can",
                wasm_encoder::EntityType::Function(auth_can_type),
            ) {
                let idx = self.function_count;
                self.http_import_indices
                    .insert("_auth_can".to_string(), idx);
                self.function_map.insert("_auth_can".to_string(), idx);
                self.function_map.insert("auth.can".to_string(), idx);
                self.function_count += 1;
            }

            // _auth_has_any_role(rolesJsonPtr: i32, rolesJsonLen: i32) -> i32
            let auth_has_any_role_type =
                self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "_auth_has_any_role",
                wasm_encoder::EntityType::Function(auth_has_any_role_type),
            ) {
                let idx = self.function_count;
                self.http_import_indices
                    .insert("_auth_has_any_role".to_string(), idx);
                self.function_map
                    .insert("_auth_has_any_role".to_string(), idx);
                self.function_map.insert("auth.hasAnyRole".to_string(), idx);
                self.function_count += 1;
            }

            // =========================================
            // Response functions
            // =========================================

            // _res_redirect(urlPtr: i32, urlLen: i32, status_code: i32) -> i32
            let res_redirect_type = self.add_function_type(
                &[WasmType::I32, WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
            )?;
            if self.emit_import(
                "env",
                "_res_redirect",
                wasm_encoder::EntityType::Function(res_redirect_type),
            ) {
                self.http_import_indices
                    .insert("_res_redirect".to_string(), self.function_count);
                self.function_map
                    .insert("_res_redirect".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _res_set_header(namePtr: i32, nameLen: i32, valuePtr: i32, valueLen: i32) -> i32
            let res_set_header_type = self.add_function_type(
                &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
            )?;
            if self.emit_import(
                "env",
                "_res_set_header",
                wasm_encoder::EntityType::Function(res_set_header_type),
            ) {
                self.http_import_indices
                    .insert("_res_set_header".to_string(), self.function_count);
                self.function_map
                    .insert("_res_set_header".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _res_status(code: i32) -> void
            let res_status_type = self.add_function_type(&[WasmType::I32], None)?;
            if self.emit_import(
                "env",
                "_res_status",
                wasm_encoder::EntityType::Function(res_status_type),
            ) {
                self.http_import_indices
                    .insert("_res_status".to_string(), self.function_count);
                self.function_map
                    .insert("_res_status".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _http_respond(status: i32, content_typePtr: i32, content_typeLen: i32, bodyPtr: i32, bodyLen: i32) -> i32
            let http_respond_type = self.add_function_type(
                &[
                    WasmType::I32,
                    WasmType::I32,
                    WasmType::I32,
                    WasmType::I32,
                    WasmType::I32,
                ],
                Some(WasmType::I32),
            )?;
            if self.emit_import(
                "env",
                "_http_respond",
                wasm_encoder::EntityType::Function(http_respond_type),
            ) {
                self.http_import_indices
                    .insert("_http_respond".to_string(), self.function_count);
                self.function_map
                    .insert("_http_respond".to_string(), self.function_count);
                self.function_count += 1;
            }

            // _http_redirect(status: i32, urlPtr: i32, urlLen: i32) -> i32
            let http_redirect_type = self.add_function_type(
                &[WasmType::I32, WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
            )?;
            if self.emit_import(
                "env",
                "_http_redirect",
                wasm_encoder::EntityType::Function(http_redirect_type),
            ) {
                self.http_import_indices
                    .insert("_http_redirect".to_string(), self.function_count);
                self.function_map
                    .insert("_http_redirect".to_string(), self.function_count);
                self.function_count += 1;
            }
        } // end if include_server_imports

        Ok(())
    }

    /// Get the import index for an HTTP function
    pub fn get_http_import_index(&self, func_name: &str) -> Option<u32> {
        self.http_import_indices.get(func_name).copied()
    }

    /// Get the import index for a file function
    pub fn get_file_import_index(&self, func_name: &str) -> Option<u32> {
        self.file_import_indices.get(func_name).copied()
    }

    /// Register Layer 2 host bridge imports for the `db` namespace (imports only).
    ///
    /// Registers raw WASM imports (`db_query`, `db_execute`, `db_begin`, `db_commit`,
    /// `db_rollback`). Does NOT create local wrapper functions — call
    /// `register_db_builtin_wrappers()` AFTER all imports are registered to avoid
    /// corrupting function indices (WASM requires all imports before local functions).
    ///
    /// Used only when no plugin bridge (e.g. `frame.data`) provides these functions.
    pub(crate) fn register_db_builtin_imports(&mut self) -> Result<(), CompilerError> {
        self.register_import_function(
            "env",
            "_db_query",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        self.register_import_function(
            "env",
            "_db_execute",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        self.register_import_function("env", "_db_begin", &[], Some(WasmType::I32))?;
        self.register_import_function("env", "_db_commit", &[], Some(WasmType::I32))?;
        self.register_import_function("env", "_db_rollback", &[], Some(WasmType::I32))?;
        Ok(())
    }

    /// Create local wrapper functions for the `db` namespace.
    ///
    /// MUST be called AFTER all imports are registered. Wrappers expand Clean
    /// length-prefixed string pointers to the (ptr+4, len) pair the host bridge expects.
    pub(crate) fn register_db_builtin_wrappers(&mut self) -> Result<(), CompilerError> {
        if let Some(&raw_idx) = self.function_map.get("_db_query") {
            let wrap_idx = self.register_function(
                "db.query",
                &[WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
                &[
                    Instruction::LocalGet(0),
                    Instruction::I32Const(4),
                    Instruction::I32Add,
                    Instruction::LocalGet(0),
                    Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::LocalGet(1),
                    Instruction::I32Const(4),
                    Instruction::I32Add,
                    Instruction::LocalGet(1),
                    Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::Call(raw_idx),
                ],
            )?;
            self.function_map.insert("db.query".to_string(), wrap_idx);
        }

        if let Some(&raw_idx) = self.function_map.get("_db_execute") {
            let wrap_idx = self.register_function(
                "db.execute",
                &[WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
                &[
                    Instruction::LocalGet(0),
                    Instruction::I32Const(4),
                    Instruction::I32Add,
                    Instruction::LocalGet(0),
                    Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::LocalGet(1),
                    Instruction::I32Const(4),
                    Instruction::I32Add,
                    Instruction::LocalGet(1),
                    Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::Call(raw_idx),
                ],
            )?;
            self.function_map.insert("db.execute".to_string(), wrap_idx);
        }

        if let Some(&raw_idx) = self.function_map.get("_db_begin") {
            let wrap_idx = self.register_function(
                "db.begin",
                &[],
                Some(WasmType::I32),
                &[Instruction::Call(raw_idx)],
            )?;
            self.function_map.insert("db.begin".to_string(), wrap_idx);
        }

        if let Some(&raw_idx) = self.function_map.get("_db_commit") {
            let wrap_idx = self.register_function(
                "db.commit",
                &[],
                Some(WasmType::I32),
                &[Instruction::Call(raw_idx)],
            )?;
            self.function_map.insert("db.commit".to_string(), wrap_idx);
        }

        if let Some(&raw_idx) = self.function_map.get("_db_rollback") {
            let wrap_idx = self.register_function(
                "db.rollback",
                &[],
                Some(WasmType::I32),
                &[Instruction::Call(raw_idx)],
            )?;
            self.function_map
                .insert("db.rollback".to_string(), wrap_idx);
        }

        Ok(())
    }

    /// Register Layer 2 host bridge import for the `env` namespace (import only).
    ///
    /// Call `register_env_builtin_wrappers()` AFTER all imports to create the
    /// local string-expansion wrapper.
    pub(crate) fn register_env_builtin_imports(&mut self) -> Result<(), CompilerError> {
        self.register_import_function(
            "env",
            "_env_get",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        Ok(())
    }

    /// Create local wrapper function for `env.get`. Must be called AFTER all imports.
    pub(crate) fn register_env_builtin_wrappers(&mut self) -> Result<(), CompilerError> {
        if let Some(&raw_idx) = self.function_map.get("_env_get") {
            let wrap_idx = self.register_function(
                "env.get",
                &[WasmType::I32],
                Some(WasmType::I32),
                &[
                    Instruction::LocalGet(0),
                    Instruction::I32Const(4),
                    Instruction::I32Add,
                    Instruction::LocalGet(0),
                    Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::Call(raw_idx),
                ],
            )?;
            self.function_map.insert("env.get".to_string(), wrap_idx);
        }
        Ok(())
    }

    /// Register Layer 2 host bridge import for the `time` namespace (import only).
    ///
    /// Call `register_time_builtin_wrappers()` AFTER all imports to create the
    /// local passthrough wrapper.
    pub(crate) fn register_time_builtin_imports(&mut self) -> Result<(), CompilerError> {
        // Host bridge returns i64 (Unix timestamp); wrapper truncates to i32 for Clean integer
        self.register_import_function("env", "_time_now", &[], Some(WasmType::I64))?;
        Ok(())
    }

    /// Create local wrapper function for `time.now`. Must be called AFTER all imports.
    pub(crate) fn register_time_builtin_wrappers(&mut self) -> Result<(), CompilerError> {
        if let Some(&raw_idx) = self.function_map.get("_time_now") {
            let wrap_idx = self.register_function(
                "time.now",
                &[],
                Some(WasmType::I32),
                &[Instruction::Call(raw_idx), Instruction::I32WrapI64],
            )?;
            self.function_map.insert("time.now".to_string(), wrap_idx);
            // now() is a bare alias used by frame.data plugin-generated code
            self.function_map.insert("now".to_string(), wrap_idx);
        }
        Ok(())
    }

    /// Register host bridge imports for state reset operations.
    ///
    /// `_state_reset_all()` — reset all declared state to defaults
    /// `_state_reset_named(base_ptr: i32)` — reset a single named state variable.
    ///   `base_ptr` points to a `[4-byte len][content bytes]` string structure.
    pub(crate) fn register_state_reset_imports(&mut self) -> Result<(), CompilerError> {
        self.register_import_function("env", "_state_reset_all", &[], None)?;
        self.register_import_function("env", "_state_reset_named", &[WasmType::I32], None)?;
        Ok(())
    }

    /// Register Layer 3 host bridge import for `_server_sleep` (import only).
    ///
    /// Signature: `_server_sleep(ms: i64) -> void`
    /// Call `register_server_sleep_wrapper()` AFTER all imports.
    pub(crate) fn register_server_sleep_import(&mut self) -> Result<(), CompilerError> {
        self.register_import_function("env", "_server_sleep", &[WasmType::I64], None)?;
        Ok(())
    }

    /// Create local wrapper for `server.sleep(ms: i32)`. Extends i32→i64 and calls `_server_sleep`.
    /// Must be called AFTER all imports.
    pub(crate) fn register_server_sleep_wrapper(&mut self) -> Result<(), CompilerError> {
        if let Some(&raw_idx) = self.function_map.get("_server_sleep") {
            let wrap_idx = self.register_function(
                "server.sleep",
                &[WasmType::I32],
                None,
                &[
                    Instruction::LocalGet(0),
                    Instruction::I64ExtendI32S,
                    Instruction::Call(raw_idx),
                ],
            )?;
            self.function_map
                .insert("server.sleep".to_string(), wrap_idx);
        }
        Ok(())
    }

    /// Register Layer 2 host bridge imports for the `crypto` namespace (imports only).
    ///
    /// Call `register_crypto_builtin_wrappers()` AFTER all imports to create local
    /// string-expansion wrappers.
    pub(crate) fn register_crypto_builtin_imports(&mut self) -> Result<(), CompilerError> {
        self.register_import_function(
            "env",
            "_crypto_hash_password",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        self.register_import_function(
            "env",
            "_crypto_random_bytes",
            &[WasmType::I32],
            Some(WasmType::I32),
        )?;
        self.register_import_function(
            "env",
            "_crypto_random_hex",
            &[WasmType::I32],
            Some(WasmType::I32),
        )?;
        self.register_import_function(
            "env",
            "_crypto_hash_sha256",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        self.register_import_function(
            "env",
            "_crypto_hash_sha512",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        self.register_import_function(
            "env",
            "_crypto_verify_password",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        Ok(())
    }

    /// Create local wrapper functions for the `crypto` namespace.
    /// Must be called AFTER all imports are registered.
    pub(crate) fn register_crypto_builtin_wrappers(&mut self) -> Result<(), CompilerError> {
        // Single-string-param wrappers (expand ptr → ptr+4, len)
        for (raw_name, lang_name) in &[
            ("_crypto_hash_password", "crypto.hashPassword"),
            ("_crypto_hash_sha256", "crypto.sha256"),
            ("_crypto_hash_sha512", "crypto.sha512"),
        ] {
            if let Some(&raw_idx) = self.function_map.get(*raw_name) {
                let wrap_idx = self.register_function(
                    lang_name,
                    &[WasmType::I32],
                    Some(WasmType::I32),
                    &[
                        Instruction::LocalGet(0),
                        Instruction::I32Const(4),
                        Instruction::I32Add,
                        Instruction::LocalGet(0),
                        Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }),
                        Instruction::Call(raw_idx),
                    ],
                )?;
                self.function_map.insert(lang_name.to_string(), wrap_idx);
            }
        }

        // Pass-through wrappers (no string expansion needed)
        for (raw_name, lang_name) in &[
            ("_crypto_random_bytes", "crypto.randomBytes"),
            ("_crypto_random_hex", "crypto.randomHex"),
        ] {
            if let Some(&raw_idx) = self.function_map.get(*raw_name) {
                let wrap_idx = self.register_function(
                    lang_name,
                    &[WasmType::I32],
                    Some(WasmType::I32),
                    &[Instruction::LocalGet(0), Instruction::Call(raw_idx)],
                )?;
                self.function_map.insert(lang_name.to_string(), wrap_idx);
            }
        }

        // Two-string-param wrapper for verifyPassword
        if let Some(&raw_idx) = self.function_map.get("_crypto_verify_password") {
            let wrap_idx = self.register_function(
                "crypto.verifyPassword",
                &[WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
                &[
                    Instruction::LocalGet(0),
                    Instruction::I32Const(4),
                    Instruction::I32Add,
                    Instruction::LocalGet(0),
                    Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::LocalGet(1),
                    Instruction::I32Const(4),
                    Instruction::I32Add,
                    Instruction::LocalGet(1),
                    Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }),
                    Instruction::Call(raw_idx),
                ],
            )?;
            self.function_map
                .insert("crypto.verifyPassword".to_string(), wrap_idx);
        }

        Ok(())
    }

    /// Register simplified print function imports following WebAssembly best practices
    /// Only registers essential print functions to avoid duplication issues
    pub(crate) fn register_print_imports(&mut self) -> Result<(), CompilerError> {
        // print(ptr: i32, len: i32) -> void - matches runtime expectation
        let print_type = self.add_function_type(&[WasmType::I32, WasmType::I32], None)?;
        if self.emit_import(
            "env",
            "print",
            wasm_encoder::EntityType::Function(print_type),
        ) {
            self.function_map
                .insert("print".to_string(), self.function_count);
            self.imported_functions.insert("print".to_string());
            self.function_count += 1;
        }

        // printl(ptr: i32, len: i32) -> void - print with newline
        let printl_type = self.add_function_type(&[WasmType::I32, WasmType::I32], None)?;
        if self.emit_import(
            "env",
            "printl",
            wasm_encoder::EntityType::Function(printl_type),
        ) {
            self.function_map
                .insert("printl".to_string(), self.function_count);
            self.imported_functions.insert("printl".to_string());
            self.function_count += 1;
        }

        // print_integer(value: i64) -> void — typed integer print (spec stdlib-reference.md §1)
        let print_integer_type = self.add_function_type(&[WasmType::I64], None)?;
        if self.emit_import(
            "env",
            "print_integer",
            wasm_encoder::EntityType::Function(print_integer_type),
        ) {
            self.function_map
                .insert("print_integer".to_string(), self.function_count);
            self.imported_functions.insert("print_integer".to_string());
            self.function_count += 1;
        }

        // print_float(value: f64) -> void — typed float print (spec stdlib-reference.md §1)
        let print_float_type = self.add_function_type(&[WasmType::F64], None)?;
        if self.emit_import(
            "env",
            "print_float",
            wasm_encoder::EntityType::Function(print_float_type),
        ) {
            self.function_map
                .insert("print_float".to_string(), self.function_count);
            self.imported_functions.insert("print_float".to_string());
            self.function_count += 1;
        }

        // print_boolean(value: i32) -> void — typed boolean print (spec stdlib-reference.md §1)
        let print_boolean_type = self.add_function_type(&[WasmType::I32], None)?;
        if self.emit_import(
            "env",
            "print_boolean",
            wasm_encoder::EntityType::Function(print_boolean_type),
        ) {
            self.function_map
                .insert("print_boolean".to_string(), self.function_count);
            self.imported_functions.insert("print_boolean".to_string());
            self.function_count += 1;
        }

        // error(ptr: i32) -> i32 — raise a runtime error.
        // Takes a string struct pointer (like input), returns i32 so it can appear
        // in expression context. Host prints the message and traps.
        let error_type = self.add_function_type(&[WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "error",
            wasm_encoder::EntityType::Function(error_type),
        ) {
            let error_idx = self.function_count;
            self.function_map.insert("error".to_string(), error_idx);
            self.imported_functions.insert("error".to_string());
            self.function_count += 1;
        }

        Ok(())
    }

    /// Register console input function imports
    ///
    /// `input` (plain string input) is always emitted — it is a core language
    /// built-in and cheap for any host to provide.
    ///
    /// The typed variants (`input_integer`, `input_float`, `input_yesno`,
    /// `input_range`) are gated on reachability: if no call site in the MIR
    /// references any `input.*` variant, they are omitted from the import
    /// section.  This is safe because `mem_alloc` (and the other memory
    /// imports registered by `register_type_conversion_imports`) is now
    /// resolved dynamically by `ValidatorManager` and `ListClass` rather
    /// than via a hardcoded index, so skipping these four imports no longer
    /// shifts subsequent indices in a way that corrupts those wrappers.
    pub(crate) fn register_console_imports(&mut self) -> Result<(), CompilerError> {
        // input(prompt_ptr: i32) -> string_ptr: i32  [always emitted]
        let input_type = self.add_function_type(&[WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "input",
            wasm_encoder::EntityType::Function(input_type),
        ) {
            let input_func_index = self.function_count;
            self.function_map
                .insert("input".to_string(), input_func_index);
            // input.string (alias for input) - dotted namespace version
            self.function_map
                .insert("input.string".to_string(), input_func_index);
            self.function_count += 1;
        }

        // Gate the typed input variants on reachability.
        // Programs that never call input.integer / input.number / input.yesNo /
        // input.range do not need these host imports.
        let has_typed_input = self.has_reachable_prefix("input_integer")
            || self.has_reachable_prefix("input_float")
            || self.has_reachable_prefix("input_yesno")
            || self.has_reachable_prefix("input_range")
            || self.has_reachable_prefix("input.integer")
            || self.has_reachable_prefix("input.number")
            || self.has_reachable_prefix("input.yesNo")
            || self.has_reachable_prefix("input.range");

        if has_typed_input {
            // input_integer(prompt_ptr: i32) -> integer: i32
            let input_integer_type =
                self.add_function_type(&[WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "input_integer",
                wasm_encoder::EntityType::Function(input_integer_type),
            ) {
                self.function_map
                    .insert("input.integer".to_string(), self.function_count);
                self.function_count += 1;
            }

            // input_float(prompt_ptr: i32) -> number: f64
            let input_number_type =
                self.add_function_type(&[WasmType::I32], Some(WasmType::F64))?;
            if self.emit_import(
                "env",
                "input_float",
                wasm_encoder::EntityType::Function(input_number_type),
            ) {
                self.function_map
                    .insert("input.number".to_string(), self.function_count);
                self.function_count += 1;
            }

            // input_yesno(prompt_ptr: i32) -> boolean: i32
            let input_yesno_type = self.add_function_type(&[WasmType::I32], Some(WasmType::I32))?;
            if self.emit_import(
                "env",
                "input_yesno",
                wasm_encoder::EntityType::Function(input_yesno_type),
            ) {
                self.function_map
                    .insert("input.yesNo".to_string(), self.function_count);
                self.function_count += 1;
            }

            // input_range(prompt_ptr: i32, prompt_len: i32, min: i32, max: i32) -> integer: i32
            let input_range_type = self.add_function_type(
                &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
            )?;
            if self.emit_import(
                "env",
                "input_range",
                wasm_encoder::EntityType::Function(input_range_type),
            ) {
                self.function_map
                    .insert("input.range".to_string(), self.function_count);
                self.function_count += 1;
            }
        }

        Ok(())
    }

    /// Register type conversion import functions
    pub(crate) fn register_type_conversion_imports(&mut self) -> Result<(), CompilerError> {
        // CRITICAL: Register memory allocation function FIRST to ensure correct indices
        // mem_alloc(type_id: i32, size: i32) -> i32 (returns pointer)
        let mem_alloc_type =
            self.add_function_type(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "memory_runtime",
            "mem_alloc",
            wasm_encoder::EntityType::Function(mem_alloc_type),
        ) {
            self.function_map
                .insert("mem_alloc".to_string(), self.function_count);
            self.imported_functions.insert("mem_alloc".to_string());
            self.function_count += 1;
        }

        // mem_retain(ptr: i32) -> void
        let mem_retain_type = self.add_function_type(&[WasmType::I32], None)?;
        if self.emit_import(
            "memory_runtime",
            "mem_retain",
            wasm_encoder::EntityType::Function(mem_retain_type),
        ) {
            self.function_map
                .insert("mem_retain".to_string(), self.function_count);
            self.imported_functions.insert("mem_retain".to_string());
            self.function_count += 1;
        }

        // mem_release(ptr: i32) -> void
        let mem_release_type = self.add_function_type(&[WasmType::I32], None)?;
        if self.emit_import(
            "memory_runtime",
            "mem_release",
            wasm_encoder::EntityType::Function(mem_release_type),
        ) {
            self.function_map
                .insert("mem_release".to_string(), self.function_count);
            self.imported_functions.insert("mem_release".to_string());
            self.function_count += 1;
        }

        // mem_scope_push() -> void - Push current allocation offset as scope mark
        // Used at the start of loops/blocks for arena-style memory management
        let mem_scope_push_type = self.add_function_type(&[], None)?;
        if self.emit_import(
            "memory_runtime",
            "mem_scope_push",
            wasm_encoder::EntityType::Function(mem_scope_push_type),
        ) {
            self.function_map
                .insert("mem_scope_push".to_string(), self.function_count);
            self.imported_functions.insert("mem_scope_push".to_string());
            self.function_count += 1;
        }

        // mem_scope_pop() -> void - Pop scope mark and reset allocation offset
        // Used at the end of loops/blocks to free all allocations made in that scope
        let mem_scope_pop_type = self.add_function_type(&[], None)?;
        if self.emit_import(
            "memory_runtime",
            "mem_scope_pop",
            wasm_encoder::EntityType::Function(mem_scope_pop_type),
        ) {
            self.function_map
                .insert("mem_scope_pop".to_string(), self.function_count);
            self.imported_functions.insert("mem_scope_pop".to_string());
            self.function_count += 1;
        }

        // NOTE: int_to_string is now NATIVE (registered in register_memory_operations)
        // The native implementation uses malloc to allocate strings and is fully standalone

        // float_to_string(value: f64) -> i32 (returns string pointer)
        let float_to_string_type = self.add_function_type(&[WasmType::F64], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "float_to_string",
            wasm_encoder::EntityType::Function(float_to_string_type),
        ) {
            let idx = self.function_count;
            self.function_map.insert("float_to_string".to_string(), idx);
            // number_to_string is the language-level alias used by the class serializer
            // (SymbolId 1010 in MIR). Register it here so codegen can resolve it.
            self.function_map
                .insert("number_to_string".to_string(), idx);
            self.imported_functions
                .insert("float_to_string".to_string());
            self.function_count += 1;
        }

        // NOTE: bool_to_string is now NATIVE (registered in register_memory_operations)
        // The native implementation uses pre-allocated "true"/"false" strings from the string pool

        // NOTE: string_to_int is now NATIVE (registered in register_memory_operations)
        // The native implementation parses decimal strings to integers

        // string_to_float(str_ptr: i32) -> f64 (returns parsed float)
        // NOTE: float parsing is complex, keeping as import for accuracy
        let string_to_float_type = self.add_function_type(&[WasmType::I32], Some(WasmType::F64))?;
        if self.emit_import(
            "env",
            "string_to_float",
            wasm_encoder::EntityType::Function(string_to_float_type),
        ) {
            self.function_map
                .insert("string_to_float".to_string(), self.function_count);
            self.imported_functions
                .insert("string_to_float".to_string());
            self.function_count += 1;
        }

        // FIXED: string.concat(str1_ptr: i32, str2_ptr: i32) -> i32
        // Each pointer points to a length-prefixed string: [4-byte len][content]
        // Returns result_ptr (pointer to new length-prefixed concatenated string)
        let string_concat_type = self
            .type_manager
            .add_function_type_single(&[WasmType::I32, WasmType::I32], Some(WasmType::I32))?;
        if self.emit_import(
            "env",
            "string.concat",
            wasm_encoder::EntityType::Function(string_concat_type),
        ) {
            self.function_map
                .insert("string.concat".to_string(), self.function_count);
            self.imported_functions.insert("string.concat".to_string());
            self.function_count += 1;
        }

        Ok(())
    }

    /// Register method-style functions as imports from the env module
    pub(crate) fn register_method_style_imports(&mut self) -> Result<(), CompilerError> {
        // Register type-specific method functions that match the semantic analyzer's function_table
        // These are the method-style functions like string.length, integer.toString, etc.

        let types = ["integer", "number", "string", "boolean"];

        for type_name in &types {
            // Type conversion methods - object is first parameter
            self.register_import_function(
                "env",
                &format!("{}.toString", type_name),
                match *type_name {
                    "integer" => &[WasmType::I32],
                    "number" => &[WasmType::F64],
                    "string" => &[WasmType::I32],
                    "boolean" => &[WasmType::I32],
                    _ => &[WasmType::I32],
                },
                Some(WasmType::I32), // Returns string pointer
            )?;

            self.register_import_function(
                "env",
                &format!("{}.toInteger", type_name),
                match *type_name {
                    "integer" => &[WasmType::I32],
                    "number" => &[WasmType::F64],
                    "string" => &[WasmType::I32],
                    "boolean" => &[WasmType::I32],
                    _ => &[WasmType::I32],
                },
                Some(WasmType::I32), // Returns integer
            )?;

            self.register_import_function(
                "env",
                &format!("{}.toNumber", type_name),
                match *type_name {
                    "integer" => &[WasmType::I32],
                    "number" => &[WasmType::F64],
                    "string" => &[WasmType::I32],
                    "boolean" => &[WasmType::I32],
                    _ => &[WasmType::I32],
                },
                Some(WasmType::F64), // Returns number
            )?;

            self.register_import_function(
                "env",
                &format!("{}.toBoolean", type_name),
                match *type_name {
                    "integer" => &[WasmType::I32],
                    "number" => &[WasmType::F64],
                    "string" => &[WasmType::I32],
                    "boolean" => &[WasmType::I32],
                    _ => &[WasmType::I32],
                },
                Some(WasmType::I32), // Returns boolean (as i32)
            )?;

            // Utility methods
            self.register_import_function(
                "env",
                &format!("{}.length", type_name),
                match *type_name {
                    "integer" => &[WasmType::I32],
                    "number" => &[WasmType::F64],
                    "string" => &[WasmType::I32],
                    "boolean" => &[WasmType::I32],
                    _ => &[WasmType::I32],
                },
                Some(WasmType::I32), // Returns length
            )?;
        }

        // Register string-specific methods
        self.register_import_function(
            "env",
            "string.toUpperCase",
            &[WasmType::I32],    // string pointer
            Some(WasmType::I32), // returns string pointer
        )?;

        self.register_import_function(
            "env",
            "string.toLowerCase",
            &[WasmType::I32],    // string pointer
            Some(WasmType::I32), // returns string pointer
        )?;

        self.register_import_function(
            "env",
            "string.concat",
            &[WasmType::I32, WasmType::I32], // string1 pointer, string2 pointer
            Some(WasmType::I32),             // returns concatenated string pointer
        )?;

        Ok(())
    }

    /// Helper method for tests to generate complete WASM module without imports
    pub fn generate_test_module_without_imports(&mut self) -> Result<Vec<u8>, CompilerError> {
        // Set up memory section
        self.setup_memory_section();

        // Export all registered functions
        for (func_name, &func_index) in &self.function_map.clone() {
            self.export_section
                .export(func_name, wasm_encoder::ExportKind::Func, func_index);
        }
        self.export_section
            .export("memory", wasm_encoder::ExportKind::Memory, 0);

        self.assemble_module()
    }

    pub fn register_function_with_locals(
        &mut self,
        name: &str,
        params: &[WasmType],
        return_type: Option<WasmType>,
        local_types: &[WasmType],
        instructions: &[Instruction],
    ) -> Result<u32, CompilerError> {
        let function_index = self.function_count;
        self.instruction_generator.register_function(
            name,
            params,
            return_type,
            instructions,
            function_index,
        )?;
        let type_index = self.add_function_type(params, return_type)?;
        self.function_section.function(type_index);
        let locals_needed: Vec<(u32, wasm_encoder::ValType)> = local_types
            .iter()
            .map(|wasm_type| {
                (
                    1u32,
                    match wasm_type {
                        WasmType::I32 => wasm_encoder::ValType::I32,
                        WasmType::F64 => wasm_encoder::ValType::F64,
                        WasmType::I64 => wasm_encoder::ValType::I64,
                        WasmType::F32 => wasm_encoder::ValType::F32,
                        WasmType::V128 => wasm_encoder::ValType::V128,
                        WasmType::Unit => wasm_encoder::ValType::I32,
                    },
                )
            })
            .collect();
        let mut func = Function::new(locals_needed);
        for inst in instructions {
            func.instruction(inst);
        }
        func.instruction(&Instruction::End);
        self.code_section.function(&func);
        self.function_names.push(name.to_string());
        self.function_map.insert(name.to_string(), function_index);
        self.function_count += 1;
        Ok(function_index)
    }

    pub fn register_function(
        &mut self,
        name: &str,
        params: &[WasmType],
        return_type: Option<WasmType>,
        instructions: &[Instruction],
    ) -> Result<u32, CompilerError> {
        let function_index = self.function_count;
        self.instruction_generator.register_function(
            name,
            params,
            return_type,
            instructions,
            function_index,
        )?;
        let type_index = self.add_function_type(params, return_type)?;
        self.function_section.function(type_index);
        let max_local_index = instructions
            .iter()
            .filter_map(|inst| match inst {
                Instruction::LocalGet(idx)
                | Instruction::LocalSet(idx)
                | Instruction::LocalTee(idx) => Some(*idx),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let param_count = params.len() as u32;
        let locals_needed: Vec<(u32, wasm_encoder::ValType)> = if max_local_index >= param_count {
            let additional_locals = max_local_index - param_count + 1;
            (0..additional_locals)
                .map(|_| (1u32, wasm_encoder::ValType::I32))
                .collect()
        } else {
            vec![]
        };
        let mut func = Function::new(locals_needed);
        for inst in instructions {
            if let Instruction::Call(idx) = inst {
                if *idx == u32::MAX {
                    panic!(
                        "BUG: register_function('{}') contains Call(u32::MAX) — tree-shaken import index leaked into function body",
                        name
                    );
                }
            }
            func.instruction(inst);
        }
        func.instruction(&Instruction::End);
        self.code_section.function(&func);
        self.function_names.push(name.to_string());
        self.function_map.insert(name.to_string(), function_index);
        self.function_count += 1;
        Ok(function_index)
    }

    pub fn add_function_alias(&mut self, alias: &str, function_index: u32) {
        self.function_map.insert(alias.to_string(), function_index);
    }

    pub fn register_function_multi(
        &mut self,
        name: &str,
        params: &[WasmType],
        return_types: &[WasmType],
        instructions: &[Instruction],
    ) -> Result<u32, CompilerError> {
        let function_index = self.function_count;
        let type_index = self.add_function_type_multi(params, return_types)?;
        self.function_section.function(type_index);
        let max_local_index = instructions
            .iter()
            .filter_map(|inst| match inst {
                Instruction::LocalGet(idx)
                | Instruction::LocalSet(idx)
                | Instruction::LocalTee(idx) => Some(*idx),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let param_count = params.len() as u32;
        let locals_needed: Vec<(u32, wasm_encoder::ValType)> = if max_local_index >= param_count {
            let additional_locals = max_local_index - param_count + 1;
            (0..additional_locals)
                .map(|_| (1u32, wasm_encoder::ValType::I32))
                .collect()
        } else {
            vec![]
        };
        let mut func = Function::new(locals_needed);
        for inst in instructions {
            func.instruction(inst);
        }
        func.instruction(&Instruction::End);
        self.code_section.function(&func);
        self.function_names.push(name.to_string());
        self.function_map.insert(name.to_string(), function_index);
        self.function_count += 1;
        let single_return = return_types.first().copied();
        self.instruction_generator.register_function(
            name,
            params,
            single_return,
            instructions,
            function_index,
        )?;
        Ok(function_index)
    }
}
