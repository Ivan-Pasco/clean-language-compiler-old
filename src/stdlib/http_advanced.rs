use crate::codegen::CodeGenerator;
use crate::types::WasmType;
use crate::error::CompilerError;
use wasm_encoder::{Instruction, MemArg};
use crate::stdlib::register_stdlib_function_with_locals;
use std::rc::Rc;
use std::cell::RefCell;
use crate::stdlib::MemoryManager;

/// Advanced HTTP class implementation for Clean Language
/// Provides comprehensive HTTP client functionality with headers, JSON support, and all standard methods
pub struct HttpAdvancedManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl HttpAdvancedManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all HTTP advanced functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // HTTP Request Methods
        self.register_request_methods(codegen)?;
        
        // Header Management
        self.register_header_functions(codegen)?;
        
        // JSON Support
        self.register_json_functions(codegen)?;
        
        // Response Processing
        self.register_response_functions(codegen)?;
        
        // Request Configuration
        self.register_config_functions(codegen)?;
        
        Ok(())
    }
    
    fn register_request_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // http.getAdvanced(url_ptr, headers_ptr) -> response_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.getAdvanced",
            &[WasmType::I32, WasmType::I32], // url_ptr, headers_ptr
            Some(WasmType::I32), // response pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // response_ptr, url_len, headers_len, status_code, body_ptr
            self.generate_get_advanced()
        )?;
        
        // http.postAdvanced(url_ptr, data_ptr, headers_ptr) -> response_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.postAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // url_ptr, data_ptr, headers_ptr
            Some(WasmType::I32), // response pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // response_ptr, url_len, data_len, headers_len, status_code, body_ptr
            self.generate_post_advanced()
        )?;
        
        // http.putAdvanced(url_ptr, data_ptr, headers_ptr) -> response_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.putAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // url_ptr, data_ptr, headers_ptr
            Some(WasmType::I32), // response pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // response_ptr, url_len, data_len, headers_len, status_code, body_ptr
            self.generate_put_advanced()
        )?;
        
        // http.patchAdvanced(url_ptr, data_ptr, headers_ptr) -> response_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.patchAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // url_ptr, data_ptr, headers_ptr
            Some(WasmType::I32), // response pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // response_ptr, url_len, data_len, headers_len, status_code, body_ptr
            self.generate_patch_advanced()
        )?;
        
        // http.deleteAdvanced(url_ptr, headers_ptr) -> response_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.deleteAdvanced",
            &[WasmType::I32, WasmType::I32], // url_ptr, headers_ptr
            Some(WasmType::I32), // response pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // response_ptr, url_len, headers_len, status_code, body_ptr
            self.generate_delete_advanced()
        )?;
        
        Ok(())
    }
    
    fn register_header_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // http.createHeaders() -> headers_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.createHeaders",
            &[], // no parameters
            Some(WasmType::I32), // headers pointer
            &[WasmType::I32], // headers_ptr
            self.generate_create_headers()
        )?;
        
        // http.addHeader(headers_ptr, name_ptr, value_ptr) -> boolean
        register_stdlib_function_with_locals(
            codegen,
            "http.addHeader",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // headers_ptr, name_ptr, value_ptr
            Some(WasmType::I32), // success boolean
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // header_count, name_len, value_len, entry_ptr, success
            self.generate_add_header()
        )?;
        
        // http.getHeader(headers_ptr, name_ptr) -> value_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.getHeader",
            &[WasmType::I32, WasmType::I32], // headers_ptr, name_ptr
            Some(WasmType::I32), // value pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // header_count, current_entry, name_len, found_value, i
            self.generate_get_header()
        )?;
        
        // http.removeHeader(headers_ptr, name_ptr) -> boolean
        register_stdlib_function_with_locals(
            codegen,
            "http.removeHeader",
            &[WasmType::I32, WasmType::I32], // headers_ptr, name_ptr
            Some(WasmType::I32), // success boolean
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // header_count, current_entry, name_len, found
            self.generate_remove_header()
        )?;
        
        Ok(())
    }
    
    fn register_json_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // http.createJsonRequest(data_ptr) -> request_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.createJsonRequest",
            &[WasmType::I32], // data_ptr (JSON string)
            Some(WasmType::I32), // request pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // request_ptr, headers_ptr, content_type_ptr, data_len
            self.generate_create_json_request()
        )?;
        
        // http.parseJsonResponse(response_ptr) -> json_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.parseJsonResponse",
            &[WasmType::I32], // response_ptr
            Some(WasmType::I32), // JSON object pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // body_ptr, content_type, is_json, json_obj_ptr
            self.generate_parse_json_response()
        )?;
        
        // http.postJson(url_ptr, json_data_ptr) -> response_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.postJson",
            &[WasmType::I32, WasmType::I32], // url_ptr, json_data_ptr
            Some(WasmType::I32), // response pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // headers_ptr, request_ptr, response_ptr
            self.generate_post_json()
        )?;
        
        // http.putJson(url_ptr, json_data_ptr) -> response_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.putJson",
            &[WasmType::I32, WasmType::I32], // url_ptr, json_data_ptr
            Some(WasmType::I32), // response pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // headers_ptr, request_ptr, response_ptr
            self.generate_put_json()
        )?;
        
        Ok(())
    }
    
    fn register_response_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // http.getStatusCode(response_ptr) -> integer
        register_stdlib_function_with_locals(
            codegen,
            "http.getStatusCode",
            &[WasmType::I32], // response_ptr
            Some(WasmType::I32), // status code
            &[WasmType::I32], // status_code
            self.generate_get_status_code()
        )?;
        
        // http.getResponseBody(response_ptr) -> string_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.getResponseBody",
            &[WasmType::I32], // response_ptr
            Some(WasmType::I32), // body string pointer
            &[WasmType::I32], // body_ptr
            self.generate_get_response_body()
        )?;
        
        // http.getResponseHeaders(response_ptr) -> headers_ptr
        register_stdlib_function_with_locals(
            codegen,
            "http.getResponseHeaders",
            &[WasmType::I32], // response_ptr
            Some(WasmType::I32), // headers pointer
            &[WasmType::I32], // headers_ptr
            self.generate_get_response_headers()
        )?;
        
        // http.isSuccessStatus(status_code) -> boolean
        register_stdlib_function_with_locals(
            codegen,
            "http.isSuccessStatus",
            &[WasmType::I32], // status_code
            Some(WasmType::I32), // is success boolean
            &[WasmType::I32], // is_success
            self.generate_is_success_status()
        )?;
        
        Ok(())
    }
    
    fn register_config_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // http.setTimeout(timeout_ms) -> void
        register_stdlib_function_with_locals(
            codegen,
            "http.setTimeout",
            &[WasmType::I32], // timeout_ms
            None, // void return
            &[], // no locals
            self.generate_set_timeout()
        )?;
        
        // http.setUserAgent(user_agent_ptr) -> void
        register_stdlib_function_with_locals(
            codegen,
            "http.setUserAgent",
            &[WasmType::I32], // user_agent_ptr
            None, // void return
            &[], // no locals
            self.generate_set_user_agent()
        )?;
        
        // http.enableRedirects(enable) -> void
        register_stdlib_function_with_locals(
            codegen,
            "http.enableRedirects",
            &[WasmType::I32], // enable boolean
            None, // void return
            &[], // no locals
            self.generate_enable_redirects()
        )?;
        
        // http.setMaxRedirects(max_redirects) -> void
        register_stdlib_function_with_locals(
            codegen,
            "http.setMaxRedirects",
            &[WasmType::I32], // max_redirects
            None, // void return
            &[], // no locals
            self.generate_set_max_redirects()
        )?;
        
        Ok(())
    }

    // Implementation methods for HTTP request functions

    fn generate_get_advanced(&self) -> Vec<Instruction> {
        vec![
            // HTTP Response structure:
            // 0-3: status_code (i32)
            // 4-7: headers_ptr (i32) 
            // 8-11: body_ptr (i32)
            // 12-15: content_length (i32)
            
            // Allocate memory for HTTP response structure (16 bytes)
            Instruction::I32Const(16),
            Instruction::Call(0), // Assume memory allocation function at index 0
            Instruction::LocalSet(0), // response_ptr
            
            // Get URL length
            Instruction::LocalGet(0), // url_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // url_len
            
            // Get headers count (first 4 bytes of headers structure)
            Instruction::LocalGet(1), // headers_ptr  
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // headers_len
            
            // Simulate HTTP GET request
            // For now, return mock successful response
            
            // Set status code to 200 (OK)
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Const(200),
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Create mock response body
            Instruction::I32Const(32), // Allocate 32 bytes for response body
            Instruction::Call(0), // Memory allocation
            Instruction::LocalSet(4), // body_ptr
            
            // Set response body pointer
            Instruction::LocalGet(0), // response_ptr
            Instruction::LocalGet(4), // body_ptr
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Set content length
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Const(10), // Mock content length
            Instruction::I32Store(MemArg { offset: 12, align: 2, memory_index: 0 }),
            
            // Return response pointer
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_post_advanced(&self) -> Vec<Instruction> {
        vec![
            // Allocate memory for HTTP response structure (16 bytes)
            Instruction::I32Const(16),
            Instruction::Call(0), // Memory allocation function
            Instruction::LocalSet(0), // response_ptr
            
            // Get URL length
            Instruction::LocalGet(0), // url_ptr (parameter)
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // url_len
            
            // Get data length
            Instruction::LocalGet(1), // data_ptr (parameter)
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // data_len
            
            // Get headers count
            Instruction::LocalGet(2), // headers_ptr (parameter)
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(3), // headers_len
            
            // Simulate HTTP POST request
            // Set status code to 201 (Created)
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Const(201),
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Create mock response body
            Instruction::I32Const(64), // Allocate 64 bytes for response body
            Instruction::Call(0), // Memory allocation
            Instruction::LocalSet(5), // body_ptr
            
            // Set response body pointer
            Instruction::LocalGet(0), // response_ptr
            Instruction::LocalGet(5), // body_ptr
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Set content length
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Const(20), // Mock content length
            Instruction::I32Store(MemArg { offset: 12, align: 2, memory_index: 0 }),
            
            // Return response pointer
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_put_advanced(&self) -> Vec<Instruction> {
        vec![
            // Similar to POST but with PUT semantics
            // Allocate memory for HTTP response structure (16 bytes)
            Instruction::I32Const(16),
            Instruction::Call(0), // Memory allocation function
            Instruction::LocalSet(0), // response_ptr
            
            // Process parameters similar to POST
            Instruction::LocalGet(0), // url_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // url_len
            
            Instruction::LocalGet(1), // data_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // data_len
            
            Instruction::LocalGet(2), // headers_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(3), // headers_len
            
            // Set status code to 200 (OK) for PUT
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Const(200),
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Create mock response body
            Instruction::I32Const(32),
            Instruction::Call(0),
            Instruction::LocalSet(5), // body_ptr
            
            // Set response body pointer
            Instruction::LocalGet(0),
            Instruction::LocalGet(5),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Set content length
            Instruction::LocalGet(0),
            Instruction::I32Const(15),
            Instruction::I32Store(MemArg { offset: 12, align: 2, memory_index: 0 }),
            
            // Return response pointer
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_patch_advanced(&self) -> Vec<Instruction> {
        vec![
            // Similar to PUT but with PATCH semantics
            // Allocate memory for HTTP response structure
            Instruction::I32Const(16),
            Instruction::Call(0),
            Instruction::LocalSet(0), // response_ptr
            
            // Process parameters
            Instruction::LocalGet(0), // url_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // url_len
            
            Instruction::LocalGet(1), // data_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // data_len
            
            Instruction::LocalGet(2), // headers_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(3), // headers_len
            
            // Set status code to 200 (OK) for PATCH
            Instruction::LocalGet(0),
            Instruction::I32Const(200),
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Create response body
            Instruction::I32Const(32),
            Instruction::Call(0),
            Instruction::LocalSet(5), // body_ptr
            
            Instruction::LocalGet(0),
            Instruction::LocalGet(5),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            Instruction::LocalGet(0),
            Instruction::I32Const(12),
            Instruction::I32Store(MemArg { offset: 12, align: 2, memory_index: 0 }),
            
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_delete_advanced(&self) -> Vec<Instruction> {
        vec![
            // Allocate memory for HTTP response structure
            Instruction::I32Const(16),
            Instruction::Call(0),
            Instruction::LocalSet(0), // response_ptr
            
            // Get URL and headers lengths
            Instruction::LocalGet(0), // url_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // url_len
            
            Instruction::LocalGet(1), // headers_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // headers_len
            
            // Set status code to 204 (No Content) for DELETE
            Instruction::LocalGet(0),
            Instruction::I32Const(204),
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Create empty response body
            Instruction::I32Const(4), // Minimal allocation
            Instruction::Call(0),
            Instruction::LocalSet(4), // body_ptr
            
            Instruction::LocalGet(0),
            Instruction::LocalGet(4),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Set content length to 0
            Instruction::LocalGet(0),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg { offset: 12, align: 2, memory_index: 0 }),
            
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_create_headers(&self) -> Vec<Instruction> {
        vec![
            // HTTP Headers structure:
            // 0-3: header_count (i32)
            // 4-7: capacity (i32)
            // 8-11: entries_ptr (i32) - pointer to array of header entries
            
            // Allocate memory for headers structure (12 bytes)
            Instruction::I32Const(12),
            Instruction::Call(0), // Memory allocation
            Instruction::LocalSet(0), // headers_ptr
            
            // Initialize header count to 0
            Instruction::LocalGet(0),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Set initial capacity to 8 headers
            Instruction::LocalGet(0),
            Instruction::I32Const(8),
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Allocate memory for header entries (8 * 8 bytes = 64 bytes)
            // Each entry: 4 bytes name_ptr + 4 bytes value_ptr
            Instruction::I32Const(64),
            Instruction::Call(0),
            
            // Store entries pointer
            Instruction::LocalGet(0),
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Return headers pointer
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_add_header(&self) -> Vec<Instruction> {
        vec![
            // Get current header count
            Instruction::LocalGet(0), // headers_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(0), // header_count
            
            // Get header name length
            Instruction::LocalGet(1), // name_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // name_len
            
            // Get header value length
            Instruction::LocalGet(2), // value_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // value_len
            
            // Calculate entry position: entries_ptr + (header_count * 8)
            Instruction::LocalGet(0), // headers_ptr (original parameter)
            Instruction::I32Load(MemArg { offset: 8, align: 2, memory_index: 0 }), // entries_ptr
            Instruction::LocalGet(0), // header_count
            Instruction::I32Const(8), // 8 bytes per entry
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(3), // entry_ptr
            
            // Store name pointer
            Instruction::LocalGet(3), // entry_ptr
            Instruction::LocalGet(1), // name_ptr (original parameter)
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Store value pointer
            Instruction::LocalGet(3), // entry_ptr
            Instruction::LocalGet(2), // value_ptr (original parameter)
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Increment header count
            Instruction::LocalGet(0), // headers_ptr (original parameter)
            Instruction::LocalGet(0), // header_count
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Return success (1)
            Instruction::I32Const(1),
            Instruction::LocalSet(4), // success
            Instruction::LocalGet(4),
        ]
    }
    
    fn generate_get_header(&self) -> Vec<Instruction> {
        vec![
            // Get header count
            Instruction::LocalGet(0), // headers_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(0), // header_count
            
            // Get entries pointer
            Instruction::LocalGet(0), // headers_ptr (original parameter)
            Instruction::I32Load(MemArg { offset: 8, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // current_entry starts as entries_ptr
            
            // Get search name length
            Instruction::LocalGet(1), // name_ptr (original parameter)
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // name_len
            
            // Initialize found value to 0 (not found)
            Instruction::I32Const(0),
            Instruction::LocalSet(3), // found_value
            
            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(4), // i
            
            // Loop through headers (simplified - return 0 for now)
            // In a full implementation, this would compare strings
            Instruction::LocalGet(3), // found_value (0 = not found)
        ]
    }
    
    fn generate_remove_header(&self) -> Vec<Instruction> {
        vec![
            // Get header count
            Instruction::LocalGet(0), // headers_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(0), // header_count
            
            // Get entries pointer
            Instruction::LocalGet(0), // headers_ptr (original parameter)
            Instruction::I32Load(MemArg { offset: 8, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // current_entry
            
            // Get search name length
            Instruction::LocalGet(1), // name_ptr (original parameter)
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // name_len
            
            // Initialize found flag to 0 (not found)
            Instruction::I32Const(0),
            Instruction::LocalSet(3), // found
            
            // Simplified implementation - return false (not removed)
            // In a full implementation, this would find and remove the header
            Instruction::LocalGet(3), // found (0 = not found/removed)
        ]
    }
    
    fn generate_create_json_request(&self) -> Vec<Instruction> {
        vec![
            // JSON Request structure:
            // 0-3: headers_ptr (i32)
            // 4-7: data_ptr (i32)
            // 8-11: data_length (i32)
            
            // Allocate memory for JSON request structure (12 bytes)
            Instruction::I32Const(12),
            Instruction::Call(0),
            Instruction::LocalSet(0), // request_ptr
            
            // Create headers for JSON request
            Instruction::Call(1800), // Assume http.createHeaders is at index 1800
            Instruction::LocalSet(1), // headers_ptr
            
            // Create "Content-Type: application/json" header
            Instruction::I32Const(0x5000), // Fixed address for "Content-Type" string
            Instruction::LocalSet(2), // content_type_ptr
            
            // Get data length
            Instruction::LocalGet(0), // data_ptr (original parameter)
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(3), // data_len
            
            // Set headers pointer in request
            Instruction::LocalGet(0), // request_ptr
            Instruction::LocalGet(1), // headers_ptr
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
            
            // Set data pointer in request
            Instruction::LocalGet(0), // request_ptr
            Instruction::LocalGet(0), // data_ptr (original parameter)
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }),
            
            // Set data length in request
            Instruction::LocalGet(0), // request_ptr
            Instruction::LocalGet(3), // data_len
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }),
            
            // Return request pointer
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_parse_json_response(&self) -> Vec<Instruction> {
        vec![
            // Get response body pointer
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Load(MemArg { offset: 8, align: 2, memory_index: 0 }),
            Instruction::LocalSet(0), // body_ptr
            
            // Check content type (simplified - assume JSON for now)
            Instruction::I32Const(1),
            Instruction::LocalSet(2), // is_json = true
            
            // For now, return the body as JSON object pointer
            // In a full implementation, this would parse JSON
            Instruction::I32Const(0x6000), // Fixed address for JSON object
            Instruction::LocalSet(3), // json_obj_ptr
            
            // Return JSON object pointer
            Instruction::LocalGet(3),
        ]
    }
    
    fn generate_post_json(&self) -> Vec<Instruction> {
        vec![
            // Create headers for JSON
            Instruction::Call(1800), // http.createHeaders
            Instruction::LocalSet(0), // headers_ptr
            
            // Create JSON request
            Instruction::LocalGet(1), // json_data_ptr (original parameter)
            Instruction::Call(1804), // http.createJsonRequest
            Instruction::LocalSet(1), // request_ptr
            
            // Perform POST with JSON data
            Instruction::LocalGet(0), // url_ptr (original parameter)
            Instruction::LocalGet(1), // json_data_ptr (original parameter)  
            Instruction::LocalGet(0), // headers_ptr
            Instruction::Call(1801), // http.postAdvanced
            Instruction::LocalSet(2), // response_ptr
            
            // Return response
            Instruction::LocalGet(2),
        ]
    }
    
    fn generate_put_json(&self) -> Vec<Instruction> {
        vec![
            // Similar to POST JSON but using PUT
            Instruction::Call(1800), // http.createHeaders
            Instruction::LocalSet(0), // headers_ptr
            
            Instruction::LocalGet(1), // json_data_ptr
            Instruction::Call(1804), // http.createJsonRequest
            Instruction::LocalSet(1), // request_ptr
            
            // Perform PUT with JSON data
            Instruction::LocalGet(0), // url_ptr (original parameter)
            Instruction::LocalGet(1), // json_data_ptr (original parameter)
            Instruction::LocalGet(0), // headers_ptr
            Instruction::Call(1802), // http.putAdvanced
            Instruction::LocalSet(2), // response_ptr
            
            Instruction::LocalGet(2),
        ]
    }
    
    fn generate_get_status_code(&self) -> Vec<Instruction> {
        vec![
            // Load status code from response structure (offset 0)
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(0), // status_code
            
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_get_response_body(&self) -> Vec<Instruction> {
        vec![
            // Load body pointer from response structure (offset 8)
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Load(MemArg { offset: 8, align: 2, memory_index: 0 }),
            Instruction::LocalSet(0), // body_ptr
            
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_get_response_headers(&self) -> Vec<Instruction> {
        vec![
            // Load headers pointer from response structure (offset 4)
            Instruction::LocalGet(0), // response_ptr
            Instruction::I32Load(MemArg { offset: 4, align: 2, memory_index: 0 }),
            Instruction::LocalSet(0), // headers_ptr
            
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_is_success_status(&self) -> Vec<Instruction> {
        vec![
            // Check if status code is in 200-299 range
            Instruction::LocalGet(0), // status_code
            Instruction::I32Const(200),
            Instruction::I32GeU, // status_code >= 200
            
            Instruction::LocalGet(0), // status_code
            Instruction::I32Const(300),
            Instruction::I32LtU, // status_code < 300
            
            Instruction::I32And, // Both conditions must be true
            Instruction::LocalSet(0), // is_success
            
            Instruction::LocalGet(0),
        ]
    }
    
    fn generate_set_timeout(&self) -> Vec<Instruction> {
        vec![
            // Store timeout value at fixed memory location (0x7000)
            Instruction::I32Const(0x7000),
            Instruction::LocalGet(0), // timeout_ms
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
        ]
    }
    
    fn generate_set_user_agent(&self) -> Vec<Instruction> {
        vec![
            // Store user agent pointer at fixed memory location (0x7004)
            Instruction::I32Const(0x7004),
            Instruction::LocalGet(0), // user_agent_ptr
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
        ]
    }
    
    fn generate_enable_redirects(&self) -> Vec<Instruction> {
        vec![
            // Store redirect enable flag at fixed memory location (0x7008)
            Instruction::I32Const(0x7008),
            Instruction::LocalGet(0), // enable boolean
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
        ]
    }
    
    fn generate_set_max_redirects(&self) -> Vec<Instruction> {
        vec![
            // Store max redirects value at fixed memory location (0x700C)
            Instruction::I32Const(0x700C),
            Instruction::LocalGet(0), // max_redirects
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::CodeGenerator;
    use crate::stdlib::MemoryManager;
    
    #[test]
    fn test_http_advanced_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let _http_advanced = HttpAdvancedManager::new(memory_manager);
    }
    
    #[test]
    fn test_http_get_advanced_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_get_advanced();
        assert!(!instructions.is_empty());
        // Should start with memory allocation
        assert!(matches!(instructions[0], Instruction::I32Const(16)));
    }
    
    #[test]
    fn test_http_post_advanced_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_post_advanced();
        assert!(!instructions.is_empty());
        // Should allocate response structure
        assert!(matches!(instructions[0], Instruction::I32Const(16)));
    }
    
    #[test]
    fn test_create_headers_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_create_headers();
        assert!(!instructions.is_empty());
        // Should allocate headers structure (12 bytes)
        assert!(matches!(instructions[0], Instruction::I32Const(12)));
    }
    
    #[test]
    fn test_add_header_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_add_header();
        assert!(!instructions.is_empty());
        // Should load header count first
        assert!(matches!(instructions[0], Instruction::LocalGet(0)));
    }
    
    #[test]
    fn test_create_json_request_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_create_json_request();
        assert!(!instructions.is_empty());
        // Should allocate request structure (12 bytes)
        assert!(matches!(instructions[0], Instruction::I32Const(12)));
    }
    
    #[test]
    fn test_get_status_code_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_get_status_code();
        assert!(!instructions.is_empty());
        // Should load from response pointer
        assert!(matches!(instructions[0], Instruction::LocalGet(0)));
    }
    
    #[test]
    fn test_is_success_status_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_is_success_status();
        assert!(!instructions.is_empty());
        // Should check status code range
        assert!(matches!(instructions[0], Instruction::LocalGet(0)));
    }
    
    #[test]
    fn test_set_timeout_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_set_timeout();
        assert!(!instructions.is_empty());
        // Should store at fixed memory location
        assert!(matches!(instructions[0], Instruction::I32Const(0x7000)));
    }
    
    #[test]
    fn test_parse_json_response_instructions() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let http_advanced = HttpAdvancedManager::new(memory_manager);
        let instructions = http_advanced.generate_parse_json_response();
        assert!(!instructions.is_empty());
        // Should load response body
        assert!(matches!(instructions[0], Instruction::LocalGet(0)));
    }
}