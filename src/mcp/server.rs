/*!
 * MCP Server Implementation
 *
 * Implements the Model Context Protocol server for AI agent communication.
 * Reads JSON-RPC requests from stdin and writes responses to stdout.
 */

use super::protocol::{error_codes, JsonRpcRequest, JsonRpcResponse, Tool, ToolInputSchema};
use crate::builtins::registry::BuiltinRegistry;
use crate::plugins::PluginDiscovery;
use crate::{compile_with_opt_level, parse_to_ast, type_check, VERSION};
use lazy_static::lazy_static;
use serde_json::json;
use std::sync::Mutex;
use std::time::SystemTime;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

// ============================================================================
// Specification Cache
// ============================================================================

#[derive(Debug)]
struct SpecSection {
    title: String,
    content: String,
    line_start: usize,
}

struct SpecificationCache {
    sections: Vec<SpecSection>,
    last_modified: SystemTime,
    file_path: String,
}

impl SpecificationCache {
    fn load(file_path: &str) -> Option<Self> {
        use std::fs;
        use std::io::BufRead;

        let metadata = fs::metadata(file_path).ok()?;
        let last_modified = metadata.modified().ok()?;
        let file = fs::File::open(file_path).ok()?;
        let reader = std::io::BufReader::new(file);

        let mut sections = Vec::new();
        let mut current_title = String::new();
        let mut current_content = String::new();
        let mut current_line_start = 0;
        let mut line_number = 0;

        for line in reader.lines() {
            line_number += 1;
            let line = line.ok()?;

            if line.starts_with("## ") {
                // Save previous section if exists
                if !current_title.is_empty() {
                    sections.push(SpecSection {
                        title: current_title.clone(),
                        content: current_content.clone(),
                        line_start: current_line_start,
                    });
                }

                // Start new section
                current_title = line[3..].trim().to_string();
                current_content = String::new();
                current_line_start = line_number;
            } else if !current_title.is_empty() {
                current_content.push_str(&line);
                current_content.push('\n');
            }
        }

        // Save final section
        if !current_title.is_empty() {
            sections.push(SpecSection {
                title: current_title,
                content: current_content,
                line_start: current_line_start,
            });
        }

        Some(SpecificationCache {
            sections,
            last_modified,
            file_path: file_path.to_string(),
        })
    }

    fn get_or_reload(&mut self) -> &[SpecSection] {
        use std::fs;

        // Check if file has been modified
        if let Ok(metadata) = fs::metadata(&self.file_path) {
            if let Ok(modified) = metadata.modified() {
                if modified > self.last_modified {
                    // Reload the file
                    if let Some(new_cache) = Self::load(&self.file_path) {
                        *self = new_cache;
                    }
                }
            }
        }

        &self.sections
    }
}

lazy_static! {
    static ref SPEC_CACHE: Mutex<Option<SpecificationCache>> = Mutex::new(None);
}

fn get_spec_path() -> Option<String> {
    // Try environment variable first
    if let Ok(path) = std::env::var("CLN_SPEC_PATH") {
        if std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }

    // Try relative to CWD
    let paths = [
        "documentation/Clean_Language_Specification.md",
        "../documentation/Clean_Language_Specification.md",
        "docs/language/Clean_Language_Specification.md",
    ];

    for path in &paths {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    // Try relative to binary location
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let spec = dir.join("../../documentation/Clean_Language_Specification.md");
            if spec.exists() {
                return spec.to_str().map(|s| s.to_string());
            }
        }
    }

    // Try home directory
    if let Ok(home) = std::env::var("HOME") {
        let cleen_spec = format!(
            "{}/.cleen/specification/Clean_Language_Specification.md",
            home
        );
        if std::path::Path::new(&cleen_spec).exists() {
            return Some(cleen_spec);
        }
    }

    None
}

// ============================================================================
// Error Code Catalog
// ============================================================================

struct ErrorCodeEntry {
    code: &'static str,
    category: &'static str,
    title: &'static str,
    description: &'static str,
    example: &'static str,
    fix: &'static str,
}

fn get_error_catalog() -> Vec<ErrorCodeEntry> {
    vec![
        // Syntax Errors (SYN)
        ErrorCodeEntry {
            code: "SYN001",
            category: "syntax",
            title: "Invalid token",
            description: "Lexer encountered an invalid token or character that doesn't match any valid syntax.",
            example: "start:\n\tinteger x = @invalid",
            fix: "Remove or replace the invalid character with valid Clean Language syntax.",
        },
        ErrorCodeEntry {
            code: "SYN002",
            category: "syntax",
            title: "Unexpected token",
            description: "Parser found a token that doesn't fit the expected syntax at this position.",
            example: "start:\n\tinteger x = // missing value",
            fix: "Check for missing values, typos in keywords, or missing operators.",
        },
        ErrorCodeEntry {
            code: "SYN003",
            category: "syntax",
            title: "Missing element",
            description: "Required syntax element is missing (e.g., closing bracket, semicolon, parameter).",
            example: "start:\n\tif (x > 0\n\t\tprint(\"positive\")",
            fix: "Add the missing syntax element (closing parenthesis, bracket, etc.).",
        },
        ErrorCodeEntry {
            code: "SYN004",
            category: "syntax",
            title: "Unterminated construct",
            description: "String literal, comment, or code block was not properly closed.",
            example: "start:\n\tstring s = \"unterminated",
            fix: "Add the closing delimiter (quote, comment marker, or closing brace).",
        },
        ErrorCodeEntry {
            code: "SYN005",
            category: "syntax",
            title: "Malformed construct",
            description: "Syntax structure is malformed or doesn't follow the language grammar.",
            example: "start:\n\tfunction = (x) => x + 1",
            fix: "Review the language specification for the correct syntax of this construct.",
        },
        ErrorCodeEntry {
            code: "SYN006",
            category: "syntax",
            title: "Indentation error",
            description: "Invalid indentation or inconsistent code block structure.",
            example: "start:\n\tif (x > 0)\nprint(\"positive\")",
            fix: "Ensure proper indentation (tabs) for nested blocks.",
        },

        // Semantic Errors (SEM)
        ErrorCodeEntry {
            code: "SEM001",
            category: "semantic",
            title: "Type mismatch",
            description: "Expression type doesn't match the expected type in this context.",
            example: "start:\n\tinteger x = \"hello\"",
            fix: "Ensure types match or use explicit type conversion functions.",
        },
        ErrorCodeEntry {
            code: "SEM002",
            category: "semantic",
            title: "Undefined symbol",
            description: "Variable, function, or type referenced but not defined in current scope.",
            example: "start:\n\tinteger x = unknownVariable",
            fix: "Define the symbol before use or check for typos in the name.",
        },
        ErrorCodeEntry {
            code: "SEM003",
            category: "semantic",
            title: "Symbol redefinition",
            description: "Variable or function name already declared in the current scope.",
            example: "start:\n\tinteger x = 5\n\tinteger x = 10",
            fix: "Use a different name or remove the duplicate declaration.",
        },
        ErrorCodeEntry {
            code: "SEM004",
            category: "semantic",
            title: "Invalid operation",
            description: "Operation is not valid for the given type(s).",
            example: "start:\n\tstring s = \"hello\"\n\tinteger x = s * 2",
            fix: "Use operations that are valid for the type or convert to an appropriate type.",
        },
        ErrorCodeEntry {
            code: "SEM005",
            category: "semantic",
            title: "Access violation",
            description: "Attempting to access a private member or violating visibility rules.",
            example: "// Accessing private field from outside class",
            fix: "Use public interfaces or accessor methods instead of accessing private members.",
        },
        ErrorCodeEntry {
            code: "SEM006",
            category: "semantic",
            title: "Inheritance error",
            description: "Problem with class inheritance (e.g., invalid base class, missing base constructor call).",
            example: "class Derived extends NonExistent:\n\tstart:",
            fix: "Ensure base class exists and is properly imported. Call base() in constructor if needed.",
        },
        ErrorCodeEntry {
            code: "SEM007",
            category: "semantic",
            title: "Generic error",
            description: "Generic type constraint or parameter error.",
            example: "// Invalid generic type parameter usage",
            fix: "Check generic type constraints and ensure type parameters are used correctly.",
        },
        ErrorCodeEntry {
            code: "SEM008",
            category: "semantic",
            title: "Inheritance cycle",
            description: "Circular inheritance detected (class inheriting from itself directly or indirectly).",
            example: "class A extends B:\nclass B extends A:",
            fix: "Remove the circular dependency in the inheritance chain.",
        },
        ErrorCodeEntry {
            code: "SEM009",
            category: "semantic",
            title: "Invalid type",
            description: "Type specification is invalid or malformed.",
            example: "start:\n\tUnknownType x = 5",
            fix: "Use a valid type name (integer, string, boolean, number, or defined class).",
        },

        // Compilation Errors (COM)
        ErrorCodeEntry {
            code: "COM001",
            category: "compilation",
            title: "WASM generation error",
            description: "Failed to generate WebAssembly code from the program.",
            example: "// Internal compiler error during WASM generation",
            fix: "Report this as a compiler bug with a minimal reproduction case.",
        },
        ErrorCodeEntry {
            code: "COM002",
            category: "compilation",
            title: "Optimization error",
            description: "Error occurred during WebAssembly optimization pass.",
            example: "// wasm-opt failed with error",
            fix: "Try compiling with a lower optimization level (--opt 0 or --opt 1).",
        },
        ErrorCodeEntry {
            code: "COM003",
            category: "compilation",
            title: "Memory layout error",
            description: "Problem with memory allocation or layout calculation.",
            example: "// Memory alignment or allocation issue",
            fix: "Check for excessively large allocations or report as compiler bug.",
        },
        ErrorCodeEntry {
            code: "COM004",
            category: "compilation",
            title: "Module resolution error",
            description: "Failed to resolve imported module or dependency.",
            example: "plugins:\n\t- nonexistent_plugin",
            fix: "Ensure the plugin or module exists and is properly installed.",
        },
        ErrorCodeEntry {
            code: "COM005",
            category: "compilation",
            title: "Target error",
            description: "Target-specific compilation issue (e.g., WASM target constraints).",
            example: "// Feature not supported in WASM target",
            fix: "Avoid features that are not supported in the WebAssembly target.",
        },
        ErrorCodeEntry {
            code: "COM006",
            category: "compilation",
            title: "Function not found",
            description: "Function referenced during code generation but definition not found.",
            example: "start:\n\tcall_to_undefined_function()",
            fix: "Define the function or check for typos in the function name.",
        },

        // Runtime Errors (RUN)
        ErrorCodeEntry {
            code: "RUN001",
            category: "runtime",
            title: "Memory violation",
            description: "Out-of-bounds memory access during program execution.",
            example: "// Accessing array element beyond bounds",
            fix: "Add bounds checking or ensure indices are within valid range.",
        },
        ErrorCodeEntry {
            code: "RUN002",
            category: "runtime",
            title: "Stack error",
            description: "Stack overflow or underflow during execution.",
            example: "// Infinite recursion without base case",
            fix: "Add proper base case to recursive functions or reduce recursion depth.",
        },
        ErrorCodeEntry {
            code: "RUN003",
            category: "runtime",
            title: "Arithmetic error",
            description: "Division by zero or arithmetic overflow/underflow.",
            example: "start:\n\tinteger x = 10 / 0",
            fix: "Add checks to prevent division by zero or handle overflow conditions.",
        },
        ErrorCodeEntry {
            code: "RUN004",
            category: "runtime",
            title: "Reference error",
            description: "Null or invalid reference accessed during execution.",
            example: "// Accessing null object reference",
            fix: "Check for null before accessing object members.",
        },
        ErrorCodeEntry {
            code: "RUN005",
            category: "runtime",
            title: "Assertion failure",
            description: "Runtime assertion failed during program execution.",
            example: "// assert(condition) failed",
            fix: "Fix the condition that caused the assertion to fail.",
        },

        // System Errors (SYS)
        ErrorCodeEntry {
            code: "SYS001",
            category: "system",
            title: "File system error",
            description: "Cannot read or write file (permissions, file not found, etc.).",
            example: "// Cannot open input.cln: No such file or directory",
            fix: "Check file path, permissions, and ensure the file exists.",
        },
        ErrorCodeEntry {
            code: "SYS002",
            category: "system",
            title: "Network error",
            description: "HTTP or network operation failed.",
            example: "// Connection refused or timeout",
            fix: "Check network connectivity and ensure the target service is accessible.",
        },
        ErrorCodeEntry {
            code: "SYS003",
            category: "system",
            title: "External tool error",
            description: "External tool (like wasm-opt) execution failed.",
            example: "// wasm-opt not found in PATH",
            fix: "Install the required tool or ensure it's in your PATH environment variable.",
        },
        ErrorCodeEntry {
            code: "SYS004",
            category: "system",
            title: "Resource exhaustion",
            description: "Out of memory or other system resource.",
            example: "// Cannot allocate memory",
            fix: "Reduce program memory usage or increase available system resources.",
        },

        // User Errors (USR)
        ErrorCodeEntry {
            code: "USR001",
            category: "user",
            title: "Invalid arguments",
            description: "Command-line arguments are invalid or malformed.",
            example: "// cln compile --invalid-flag",
            fix: "Check CLI usage with 'cln help' and use valid arguments.",
        },
        ErrorCodeEntry {
            code: "USR002",
            category: "user",
            title: "Configuration error",
            description: "Invalid project configuration (package.clean.toml issues).",
            example: "// Missing required field in package.clean.toml",
            fix: "Review the package configuration format and add missing required fields.",
        },
        ErrorCodeEntry {
            code: "USR003",
            category: "user",
            title: "Missing input",
            description: "Required input file not provided to compiler.",
            example: "// cln compile -o output.wasm (missing input file)",
            fix: "Provide the input .cln file: 'cln compile input.cln -o output.wasm'",
        },
        ErrorCodeEntry {
            code: "USR004",
            category: "user",
            title: "Invalid project structure",
            description: "Project directory structure is incorrect or incomplete.",
            example: "// Missing src/ directory in project",
            fix: "Follow the standard Clean Language project structure with src/ directory.",
        },

        // Legacy E-codes
        ErrorCodeEntry {
            code: "E001",
            category: "syntax",
            title: "General syntax error",
            description: "Legacy code for general syntax errors.",
            example: "Various syntax errors",
            fix: "Check syntax against language specification.",
        },
        ErrorCodeEntry {
            code: "E002",
            category: "semantic",
            title: "Type error",
            description: "Legacy code for type errors.",
            example: "Type mismatch or invalid type usage",
            fix: "Ensure types are used correctly.",
        },
        ErrorCodeEntry {
            code: "E003",
            category: "semantic",
            title: "Method suggestion",
            description: "Suggested method name when method not found.",
            example: "Did you mean 'method_name'?",
            fix: "Use the suggested method name or define the method.",
        },
        ErrorCodeEntry {
            code: "E004",
            category: "syntax",
            title: "Indentation error",
            description: "Legacy indentation error code.",
            example: "Inconsistent indentation",
            fix: "Use consistent tab indentation.",
        },
        ErrorCodeEntry {
            code: "E005",
            category: "syntax",
            title: "Missing block",
            description: "Expected code block but none found.",
            example: "Function or control structure missing body",
            fix: "Add the required code block with proper indentation.",
        },
        ErrorCodeEntry {
            code: "E006",
            category: "runtime",
            title: "Memory error",
            description: "Legacy memory-related error code.",
            example: "Memory allocation or access issue",
            fix: "Check memory operations and bounds.",
        },
        ErrorCodeEntry {
            code: "E007",
            category: "compilation",
            title: "Codegen error",
            description: "Legacy code generation error.",
            example: "Failed to generate code",
            fix: "Report as compiler bug.",
        },
        ErrorCodeEntry {
            code: "E008",
            category: "system",
            title: "I/O error",
            description: "Legacy I/O operation error.",
            example: "File read/write failed",
            fix: "Check file permissions and paths.",
        },
        ErrorCodeEntry {
            code: "E009",
            category: "runtime",
            title: "Runtime error",
            description: "Legacy runtime error code.",
            example: "Error during program execution",
            fix: "Debug the runtime condition causing the error.",
        },
        ErrorCodeEntry {
            code: "E010",
            category: "semantic",
            title: "Validation error",
            description: "Legacy validation error.",
            example: "Invalid program structure",
            fix: "Ensure program structure follows language rules.",
        },
        ErrorCodeEntry {
            code: "E011",
            category: "compilation",
            title: "Function not found",
            description: "Legacy function not found error.",
            example: "Call to undefined function",
            fix: "Define the function or check for typos.",
        },
        ErrorCodeEntry {
            code: "E012",
            category: "semantic",
            title: "Variable not found",
            description: "Legacy variable not found error.",
            example: "Reference to undefined variable",
            fix: "Define the variable before use.",
        },
        ErrorCodeEntry {
            code: "E013",
            category: "compilation",
            title: "Module error",
            description: "Legacy module-related error.",
            example: "Module loading or resolution failed",
            fix: "Check module paths and dependencies.",
        },
        ErrorCodeEntry {
            code: "E014",
            category: "compilation",
            title: "Import error",
            description: "Legacy import error.",
            example: "Failed to import module",
            fix: "Ensure imported modules exist and are accessible.",
        },
        ErrorCodeEntry {
            code: "E015",
            category: "semantic",
            title: "Symbol error",
            description: "Legacy symbol-related error.",
            example: "Symbol resolution failed",
            fix: "Check symbol names and scopes.",
        },
        ErrorCodeEntry {
            code: "E016",
            category: "compilation",
            title: "Memory allocation error",
            description: "Legacy memory allocation error.",
            example: "Failed to allocate memory",
            fix: "Reduce memory usage or report as bug.",
        },
        ErrorCodeEntry {
            code: "E017",
            category: "semantic",
            title: "Detailed type error",
            description: "Legacy detailed type error.",
            example: "Specific type mismatch details",
            fix: "Fix the type mismatch described in the error.",
        },
        ErrorCodeEntry {
            code: "E018",
            category: "runtime",
            title: "Division by zero",
            description: "Legacy division by zero error.",
            example: "x / 0",
            fix: "Add check to prevent division by zero.",
        },
        ErrorCodeEntry {
            code: "E019",
            category: "semantic",
            title: "Testing error",
            description: "Legacy testing-related error.",
            example: "Test execution failed",
            fix: "Fix the test or the code being tested.",
        },
    ]
}

/// Run the MCP server main loop
///
/// Reads newline-delimited JSON-RPC requests from stdin and writes responses to stdout.
/// Uses stderr for debug logging (stdout is reserved for protocol messages).
pub async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!(
        "[MCP] Starting Clean Language Compiler MCP server v{}",
        VERSION
    );
    eprintln!("[MCP] Protocol: JSON-RPC 2.0");
    eprintln!("[MCP] Reading from stdin, writing to stdout");

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            eprintln!("[MCP] EOF detected, shutting down");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Handle Content-Length framed messages (MCP stdio transport spec)
        let json_body = if trimmed.starts_with("Content-Length:") {
            let content_length: usize = match trimmed
                .strip_prefix("Content-Length:")
                .and_then(|s| s.trim().parse().ok())
            {
                Some(len) => len,
                None => {
                    eprintln!("[MCP] Invalid Content-Length header: {}", trimmed);
                    continue;
                }
            };

            // Read and discard header lines until we hit an empty line (end of headers)
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await?;
                if n == 0 || line.trim().is_empty() {
                    break;
                }
                // Skip additional headers (e.g., Content-Type)
            }

            // Read exactly content_length bytes as the JSON body
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body).await?;
            match String::from_utf8(body) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[MCP] Invalid UTF-8 in message body: {}", e);
                    continue;
                }
            }
        } else {
            // Bare JSON line (no Content-Length framing)
            trimmed.to_string()
        };

        eprintln!("[MCP] <- {}", json_body.trim());

        // Parse JSON-RPC request
        let request: JsonRpcRequest = match serde_json::from_str(json_body.trim()) {
            Ok(req) => req,
            Err(e) => {
                eprintln!("[MCP] Parse error: {}", e);
                let error_response = JsonRpcResponse::error(
                    json!(null),
                    error_codes::PARSE_ERROR,
                    format!("Parse error: {}", e),
                );
                write_response(&mut stdout, &error_response).await?;
                continue;
            }
        };

        // Check if this is a notification (no id field) — per JSON-RPC 2.0, no response
        let is_notification = request.id.is_none();

        if is_notification {
            eprintln!("[MCP] Notification: {}", request.method);
            handle_notification(&request.method);
            continue;
        }

        // Handle the request (has id, expects a response)
        let response = handle_request(request).await;
        write_response(&mut stdout, &response).await?;
    }

    Ok(())
}

/// Write a JSON-RPC response to stdout with Content-Length framing (MCP stdio transport spec)
async fn write_response(
    stdout: &mut tokio::io::Stdout,
    response: &JsonRpcResponse,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(response)?;
    eprintln!("[MCP] -> {}", json);
    let header = format!("Content-Length: {}\r\n\r\n", json.len());
    stdout.write_all(header.as_bytes()).await?;
    stdout.write_all(json.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}

/// Handle a JSON-RPC notification (no id, no response expected)
fn handle_notification(method: &str) {
    match method {
        "notifications/initialized" | "initialized" => {
            eprintln!("[MCP] Client initialized");
        }
        "notifications/cancelled" => {
            eprintln!("[MCP] Request cancelled");
        }
        _ => {
            eprintln!("[MCP] Unknown notification: {}", method);
        }
    }
}

/// Handle a JSON-RPC request and return the appropriate response
async fn handle_request(request: JsonRpcRequest) -> JsonRpcResponse {
    let id = request.id.unwrap_or(json!(null));
    match request.method.as_str() {
        "initialize" => handle_initialize(id, request.params.as_ref()),
        "tools/list" => handle_tools_list(id),
        "tools/call" => handle_tools_call(id, request.params),
        _ => JsonRpcResponse::error(
            id,
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    }
}

/// Handle the 'initialize' method
fn handle_initialize(id: serde_json::Value, params: Option<&serde_json::Value>) -> JsonRpcResponse {
    // Supported protocol versions (newest first)
    const SUPPORTED_VERSIONS: &[&str] = &["2025-03-26", "2024-11-05"];

    // Pick the best version: use the client's requested version if we support it,
    // otherwise fall back to our latest supported version
    let client_version = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let protocol_version = if SUPPORTED_VERSIONS.contains(&client_version) {
        client_version
    } else {
        SUPPORTED_VERSIONS[0]
    };

    eprintln!(
        "[MCP] Client requested protocol {}, responding with {}",
        client_version, protocol_version
    );

    let result = json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "cln",
            "version": VERSION
        },
        "instructions": "You are working with the Clean Language compiler MCP server. Clean Language is a type-safe language that compiles to WebAssembly.\n\n## GETTING STARTED (call these tools in order)\n1. `get_quick_reference` — Learn syntax, types, patterns (CALL FIRST)\n2. `get_stack_recommendation` — Get recommended plugins for your project type\n3. `list_plugins` — See installed plugins with full DSL syntax\n4. `get_plugin_examples` — Read real example files from plugins\n\n## BEST PRACTICES FOR AI ASSISTANTS\n- ALWAYS call `get_quick_reference` at the start of a session before writing any Clean Language code\n- ALWAYS call `list_plugins` before using framework features (data:, endpoints:, component:, etc.)\n- Use `check` for fast type-checking during development (no WASM generation)\n- Use `get_specification` to look up detailed language rules\n- Use `get_architecture` to understand the execution model and host bridge\n- Clean Language applications use Clean for ALL layers — server, database, UI, graphics. NEVER use JavaScript when a Clean plugin exists.\n- Use `report_error` when you encounter what appears to be a compiler bug\n\n## KEY RULES\n- File extension: .cln\n- Indentation: tabs only (not spaces)\n- Entry point: `start:` block\n- Types: integer, number, string, boolean, list<T>, matrix<T>, pairs\n- Functions declared in `functions:` block with return type first\n- No semicolons, no curly braces\n- `return value` (no parentheses)\n- One way to do things — follow the spec exactly"
    });
    JsonRpcResponse::success(id, result)
}

/// Handle the 'tools/list' method
fn handle_tools_list(id: serde_json::Value) -> JsonRpcResponse {
    let tools = get_available_tools();
    JsonRpcResponse::success(id, json!({ "tools": tools }))
}

/// Get the list of available tools
fn get_available_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "check".to_string(),
            description: "Type-check a .cln file (fast, no WASM generation). Returns type information and diagnostics.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "The Clean Language source code to type-check"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the source file (used for error reporting)"
                    }
                }),
                required: vec!["source".to_string(), "file_path".to_string()],
            },
        },
        Tool {
            name: "compile".to_string(),
            description: "Compile Clean Language source to WebAssembly. Returns the compiled WASM as base64.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "The Clean Language source code to compile"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the source file (used for error reporting)"
                    },
                    "opt_level": {
                        "type": "number",
                        "description": "Optimization level (0-3). Default: 2",
                        "minimum": 0,
                        "maximum": 3
                    }
                }),
                required: vec!["source".to_string(), "file_path".to_string()],
            },
        },
        Tool {
            name: "parse".to_string(),
            description: "Parse Clean Language source and return the AST as JSON. Only performs lexing and parsing (no type checking).".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "The Clean Language source code to parse"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the source file (used for error reporting)"
                    }
                }),
                required: vec!["source".to_string(), "file_path".to_string()],
            },
        },
        Tool {
            name: "diagnostics".to_string(),
            description: "Get detailed diagnostics (errors/warnings) for a source file. Same as check but focuses on diagnostic output.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "The Clean Language source code to analyze"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the source file (used for error reporting)"
                    }
                }),
                required: vec!["source".to_string(), "file_path".to_string()],
            },
        },
        Tool {
            name: "explain_error".to_string(),
            description: "Explain a Clean Language error code in detail. Provides description, examples, and fixes.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "code": {
                        "type": "string",
                        "description": "The error code to explain (e.g., SYN001, TYP001)"
                    }
                }),
                required: vec!["code".to_string()],
            },
        },
        Tool {
            name: "list_functions".to_string(),
            description: "List all functions defined in a source file with their signatures. Returns function names, parameters, and return types.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "The Clean Language source code to analyze"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the source file (used for error reporting)"
                    }
                }),
                required: vec!["source".to_string(), "file_path".to_string()],
            },
        },
        Tool {
            name: "list_types".to_string(),
            description: "List all type definitions (classes) in a source file. Returns class names and their properties.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "The Clean Language source code to analyze"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the source file (used for error reporting)"
                    }
                }),
                required: vec!["source".to_string(), "file_path".to_string()],
            },
        },
        Tool {
            name: "list_plugins".to_string(),
            description: "List all available plugins with their AI context. Returns plugin names, descriptions, block types, bridge functions, and AI-specific metadata (description, examples, constraints).".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "project_dir": {
                        "type": "string",
                        "description": "Optional project directory to search for project-specific plugins (in addition to global ~/.cleen/plugins/)"
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "get_specification".to_string(),
            description: "Query the Clean Language Specification. Returns table of contents if no params, specific section by name, or sections matching a search query.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "section": {
                        "type": "string",
                        "description": "Section name (partial, case-insensitive)"
                    },
                    "query": {
                        "type": "string",
                        "description": "Search term to find across all sections"
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "list_builtins".to_string(),
            description: "List all built-in functions, classes, and namespaces. Includes print, Math.sqrt, string.length, etc.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "category": {
                        "type": "string",
                        "description": "Filter by category (io, math, string, list, type, json, compare, logical, conditional)"
                    },
                    "namespace": {
                        "type": "string",
                        "description": "Filter by namespace (math, string, list, compare, conditional, logical, json)"
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "list_error_codes".to_string(),
            description: "List all compiler error codes with descriptions. Covers syntax (SYN), semantic (SEM), compilation (COM), runtime (RUN), system (SYS), and user (USR) errors.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "category": {
                        "type": "string",
                        "description": "Filter by category (syntax, semantic, compilation, runtime, system, user)"
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "get_quick_reference".to_string(),
            description: "CALL THIS FIRST. Returns a concise cheat sheet with everything needed to write Clean Language programs: syntax, types, control flow, built-in functions, and example programs. One call to start coding.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({}),
                required: vec![],
            },
        },
        Tool {
            name: "get_plugin_examples".to_string(),
            description: "Read example source files from an installed plugin. Plugins declare example files in their [ai] section — this tool reads and returns their contents so you can learn the plugin's DSL syntax.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "plugin_name": {
                        "type": "string",
                        "description": "Plugin name (e.g., 'frame.data', 'frame.endpoints')"
                    },
                    "project_dir": {
                        "type": "string",
                        "description": "Optional project directory for project-specific plugins"
                    }
                }),
                required: vec!["plugin_name".to_string()],
            },
        },
        Tool {
            name: "list_ecosystem".to_string(),
            description: "List ALL plugins available in the Clean Language ecosystem, whether installed or not. Returns the full catalog with descriptions, blocks, bridge functions, and install status. Use this to discover what plugins exist.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "category": {
                        "type": "string",
                        "description": "Filter by category: 'server', 'data', 'auth', 'ui', 'canvas', or 'all' (default)"
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "get_stack_recommendation".to_string(),
            description: "Get the recommended Clean Language plugin stack for a project type. Returns plugins, file structure, and explicit 'do not use' list. Call this when starting a new project to ensure you use Clean Language for all layers instead of JavaScript.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "project_type": {
                        "type": "string",
                        "description": "Project type: 'web-app', 'api', 'game', or 'cli'"
                    }
                }),
                required: vec!["project_type".to_string()],
            },
        },
        // ====================================================================
        // Error Reporting & Fix Notification Tools
        // ====================================================================
        Tool {
            name: "report_error".to_string(),
            description: "Report a compiler or runtime error to the Clean Language team. Generates a structured bug report with AI-enhanced context. Requires explicit user consent before sending. The report is anonymous by default and never includes the user's actual source code — only an AI-generated minimal reproduction. Call this when you detect a likely compiler bug (not a user code error).".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "error_code": {
                        "type": "string",
                        "description": "The error code (e.g., 'SYN001', 'SEM003'). Use error codes from the compiler diagnostics."
                    },
                    "error_message": {
                        "type": "string",
                        "description": "The error message as reported by the compiler."
                    },
                    "component": {
                        "type": "string",
                        "description": "Which component produced the error.",
                        "enum": ["parser", "semantic", "codegen", "runtime", "plugin", "cli", "unknown"]
                    },
                    "severity": {
                        "type": "string",
                        "description": "Severity classification of the error.",
                        "enum": ["bug", "crash", "regression", "unexpected_behavior"]
                    },
                    "minimal_repro": {
                        "type": "string",
                        "description": "Minimal Clean Language code that reproduces the error. IMPORTANT: This must be an AI-generated minimal reproduction, NOT the user's actual source code."
                    },
                    "expected_behavior": {
                        "type": "string",
                        "description": "What the correct behavior should be according to the Language Specification."
                    },
                    "actual_behavior": {
                        "type": "string",
                        "description": "What actually happens when the code is compiled or executed."
                    },
                    "spec_reference": {
                        "type": "string",
                        "description": "Reference to the relevant Language Specification section, if applicable."
                    },
                    "ai_analysis": {
                        "type": "string",
                        "description": "AI's analysis of the root cause and potential fix."
                    },
                    "suggested_component_file": {
                        "type": "string",
                        "description": "The source file in the compiler that likely needs fixing (e.g., 'parser/token_parser.rs')."
                    },
                    "consent_level": {
                        "type": "string",
                        "description": "What level of detail the user consented to share. Default: error_with_code.",
                        "enum": ["error_only", "error_with_code", "full"]
                    },
                    "user_contact": {
                        "type": "string",
                        "description": "Optional contact info if the user wants follow-up. Only include if explicitly provided by the user."
                    }
                }),
                required: vec![
                    "error_code".to_string(),
                    "error_message".to_string(),
                    "component".to_string(),
                    "severity".to_string(),
                ],
            },
        },
        Tool {
            name: "check_reported_fixes".to_string(),
            description: "Check if any previously reported errors have been fixed. Returns a list of resolved errors with the version that includes the fix. Call this at the start of a session to inform the user about fixes to bugs they reported. Only checks errors reported from this machine.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "include_all": {
                        "type": "boolean",
                        "description": "If true, returns all tracked reports regardless of status. If false (default), returns only reports with status changes since last check."
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "get_architecture".to_string(),
            description: "Get the Clean Language platform architecture: execution layers, host bridge functions, and memory model. Essential for understanding where functions execute (compiler vs runtime vs host).".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "section": {
                        "type": "string",
                        "description": "Section to return: 'layers' (execution layers), 'bridge' (host bridge functions), 'memory' (WASM memory model), or 'all' (default)"
                    }
                }),
                required: vec![],
            },
        },
    ]
}

/// Handle the 'tools/call' method
fn handle_tools_call(id: serde_json::Value, params: Option<serde_json::Value>) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing params".to_string(),
            )
        }
    };

    // Extract tool name and arguments
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(name) => name,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing tool name".to_string(),
            )
        }
    };

    let arguments = match params.get("arguments") {
        Some(args) => args,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing tool arguments".to_string(),
            )
        }
    };

    eprintln!("[MCP] Calling tool: {}", tool_name);

    // Dispatch to appropriate tool handler
    match tool_name {
        "check" => tool_check(id, arguments),
        "compile" => tool_compile(id, arguments),
        "parse" => tool_parse(id, arguments),
        "diagnostics" => tool_diagnostics(id, arguments),
        "explain_error" => tool_explain_error(id, arguments),
        "list_functions" => tool_list_functions(id, arguments),
        "list_types" => tool_list_types(id, arguments),
        "list_plugins" => tool_list_plugins(id, arguments),
        "get_specification" => tool_get_specification(id, arguments),
        "list_builtins" => tool_list_builtins(id, arguments),
        "list_error_codes" => tool_list_error_codes(id, arguments),
        "get_quick_reference" => tool_get_quick_reference(id),
        "get_plugin_examples" => tool_get_plugin_examples(id, arguments),
        "list_ecosystem" => tool_list_ecosystem(id, arguments),
        "get_stack_recommendation" => tool_get_stack_recommendation(id, arguments),
        "report_error" => tool_report_error(id, arguments),
        "check_reported_fixes" => tool_check_reported_fixes(id, arguments),
        "get_architecture" => tool_get_architecture(id, arguments),
        _ => JsonRpcResponse::error(
            id,
            error_codes::METHOD_NOT_FOUND,
            format!("Unknown tool: {}", tool_name),
        ),
    }
}

/// Tool: check - Type-check a source file
fn tool_check(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'source' parameter".to_string(),
            )
        }
    };

    let file_path = args
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("<stdin>");

    match type_check(source, file_path) {
        Ok(result) => {
            let diagnostics: Vec<serde_json::Value> = result
                .diagnostics
                .iter()
                .map(|e| error_to_json(e))
                .collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "success": result.success,
                    "function_count": result.function_count,
                    "type_count": result.type_count,
                    "duration_ms": result.duration_ms,
                    "diagnostics": diagnostics
                }),
            )
        }
        Err(errors) => {
            let diagnostics: Vec<serde_json::Value> =
                errors.iter().map(|e| error_to_json(e)).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "diagnostics": diagnostics
                }),
            )
        }
    }
}

/// Tool: compile - Compile source to WASM
fn tool_compile(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'source' parameter".to_string(),
            )
        }
    };

    let file_path = args
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("<stdin>");

    let opt_level = args.get("opt_level").and_then(|v| v.as_u64()).unwrap_or(2) as u8;

    match compile_with_opt_level(source, file_path, opt_level) {
        Ok(wasm_bytes) => {
            let base64 = base64_encode(&wasm_bytes);
            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "wasm_base64": base64,
                    "size_bytes": wasm_bytes.len()
                }),
            )
        }
        Err(errors) => {
            let diagnostics: Vec<serde_json::Value> =
                errors.iter().map(|e| error_to_json(e)).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "diagnostics": diagnostics
                }),
            )
        }
    }
}

/// Tool: parse - Parse source to AST
fn tool_parse(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'source' parameter".to_string(),
            )
        }
    };

    let file_path = args
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("<stdin>");

    match parse_to_ast(source, file_path) {
        Ok(ast) => match serde_json::to_value(&ast) {
            Ok(ast_json) => JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "ast": ast_json
                }),
            ),
            Err(e) => JsonRpcResponse::error(
                id,
                error_codes::INTERNAL_ERROR,
                format!("Failed to serialize AST: {}", e),
            ),
        },
        Err(errors) => {
            let diagnostics: Vec<serde_json::Value> =
                errors.iter().map(|e| error_to_json(e)).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "diagnostics": diagnostics
                }),
            )
        }
    }
}

/// Tool: diagnostics - Get diagnostics for source
fn tool_diagnostics(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    // Same as check but only returns diagnostics
    tool_check(id, args)
}

/// Tool: explain_error - Explain an error code
fn tool_explain_error(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c.to_uppercase(),
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'code' parameter".to_string(),
            )
        }
    };

    let explanation = get_error_explanation(&code);

    JsonRpcResponse::success(
        id,
        json!({
            "code": code,
            "title": explanation.title,
            "description": explanation.description,
            "example": explanation.example,
            "fix": explanation.fix,
            "spec_guidance": "If your code follows the specification exactly and still produces this error, it may be a compiler bug. Call `report_error` to report it. Do NOT work around the error by using alternative syntax, underscore bridge names, or non-spec patterns.",
            "workaround": null
        }),
    )
}

/// Tool: list_functions - List all functions in source
fn tool_list_functions(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'source' parameter".to_string(),
            )
        }
    };

    let file_path = args
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("<stdin>");

    match parse_to_ast(source, file_path) {
        Ok(ast) => {
            let functions: Vec<serde_json::Value> = ast
                .functions
                .iter()
                .map(|f| {
                    json!({
                        "name": f.name,
                        "parameters": f.parameters.iter().map(|p| json!({
                            "name": p.name,
                            "type": format!("{:?}", p.type_)
                        })).collect::<Vec<_>>(),
                        "return_type": format!("{:?}", f.return_type),
                        "modifier": format!("{:?}", f.modifier)
                    })
                })
                .collect();

            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "functions": functions
                }),
            )
        }
        Err(errors) => {
            let diagnostics: Vec<serde_json::Value> =
                errors.iter().map(|e| error_to_json(e)).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "diagnostics": diagnostics
                }),
            )
        }
    }
}

/// Tool: list_types - List all type definitions in source
fn tool_list_types(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'source' parameter".to_string(),
            )
        }
    };

    let file_path = args
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("<stdin>");

    match parse_to_ast(source, file_path) {
        Ok(ast) => {
            let types: Vec<serde_json::Value> = ast
                .classes
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name,
                        "fields": c.fields.iter().map(|f| json!({
                            "name": f.name,
                            "type": format!("{:?}", f.type_)
                        })).collect::<Vec<_>>(),
                        "methods": c.methods.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
                        "base_class": c.base_class
                    })
                })
                .collect();

            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "types": types
                }),
            )
        }
        Err(errors) => {
            let diagnostics: Vec<serde_json::Value> =
                errors.iter().map(|e| error_to_json(e)).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "diagnostics": diagnostics
                }),
            )
        }
    }
}

/// Tool: list_plugins - List available plugins with AI context
fn tool_list_plugins(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let project_dir = args.get("project_dir").and_then(|v| v.as_str());

    let mut discovery = PluginDiscovery::new();
    if let Some(dir) = project_dir {
        discovery = discovery.with_project_dir(dir);
    }

    match discovery.discover_all() {
        Ok(manifests) => {
            let plugins: Vec<serde_json::Value> = manifests
                .iter()
                .map(|(name, manifest)| {
                    let bridge_fns: Vec<serde_json::Value> = manifest
                        .bridge
                        .functions
                        .iter()
                        .map(|f| {
                            json!({
                                "name": f.name,
                                "params": f.params,
                                "returns": f.returns,
                                "description": f.description
                            })
                        })
                        .collect();

                    let lang = &manifest.language;
                    let keywords: Vec<serde_json::Value> = lang
                        .keywords
                        .iter()
                        .map(|k| {
                            json!({
                                "name": k.name,
                                "description": k.description,
                                "context": k.context
                            })
                        })
                        .collect();
                    let types: Vec<serde_json::Value> = lang
                        .types
                        .iter()
                        .map(|t| {
                            json!({
                                "name": t.name,
                                "description": t.description
                            })
                        })
                        .collect();
                    let functions: Vec<serde_json::Value> = lang
                        .functions
                        .iter()
                        .map(|f| {
                            json!({
                                "name": f.name,
                                "signature": f.signature,
                                "description": f.description
                            })
                        })
                        .collect();

                    let language_json = json!({
                        "keywords": keywords,
                        "types": types,
                        "functions": functions
                    });

                    let mut plugin_json = json!({
                        "name": name,
                        "version": manifest.plugin.version,
                        "description": manifest.plugin.description,
                        "blocks": manifest.handles.blocks,
                        "bridge_functions": bridge_fns,
                        "language": language_json
                    });

                    // Add AI context if present
                    let ai = &manifest.ai;
                    if ai.description.is_some()
                        || !ai.examples.is_empty()
                        || !ai.constraints.is_empty()
                    {
                        plugin_json["ai"] = json!({
                            "description": ai.description,
                            "examples": ai.examples,
                            "constraints": ai.constraints
                        });
                    }

                    plugin_json
                })
                .collect();

            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "plugins": plugins
                }),
            )
        }
        Err(e) => JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "plugins": [],
                "note": format!("Plugin discovery skipped: {}", e)
            }),
        ),
    }
}

/// Tool: get_plugin_examples - Read example files from an installed plugin
fn tool_get_plugin_examples(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let plugin_name = match args.get("plugin_name").and_then(|v| v.as_str()) {
        Some(name) => name,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'plugin_name' parameter".to_string(),
            )
        }
    };
    let project_dir = args.get("project_dir").and_then(|v| v.as_str());

    let mut discovery = PluginDiscovery::new();
    if let Some(dir) = project_dir {
        discovery = discovery.with_project_dir(dir);
    }

    // Load the specific plugin
    match discovery.load_plugin(plugin_name) {
        Ok(manifest) => {
            let example_paths = &manifest.ai.examples;

            if example_paths.is_empty() {
                return JsonRpcResponse::success(
                    id,
                    json!({
                        "success": true,
                        "plugin": plugin_name,
                        "examples": [],
                        "note": "This plugin has no example files declared in its [ai] section."
                    }),
                );
            }

            // Resolve example paths relative to the plugin directory
            // Try project dir first, then global
            let plugin_dir = find_plugin_dir(plugin_name, project_dir);

            let mut examples: Vec<serde_json::Value> = Vec::new();
            for example_path in example_paths {
                let full_path = if let Some(ref pdir) = plugin_dir {
                    pdir.join(example_path)
                } else {
                    std::path::PathBuf::from(example_path)
                };

                match std::fs::read_to_string(&full_path) {
                    Ok(content) => {
                        examples.push(json!({
                            "path": example_path,
                            "content": content
                        }));
                    }
                    Err(e) => {
                        examples.push(json!({
                            "path": example_path,
                            "error": format!("Could not read file: {}", e)
                        }));
                    }
                }
            }

            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "plugin": plugin_name,
                    "examples": examples,
                    "language": {
                        "blocks": manifest.handles.blocks,
                        "keywords": manifest.language.keywords.iter()
                            .map(|k| json!({"name": k.name, "description": k.description}))
                            .collect::<Vec<_>>(),
                        "types": manifest.language.types.iter()
                            .map(|t| json!({"name": t.name, "description": t.description}))
                            .collect::<Vec<_>>(),
                    }
                }),
            )
        }
        Err(e) => JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": format!("Plugin '{}' not found: {}", plugin_name, e)
            }),
        ),
    }
}

/// Find the directory where a plugin is installed
fn find_plugin_dir(name: &str, project_dir: Option<&str>) -> Option<std::path::PathBuf> {
    let dir_names = [name.to_string(), name.replace('.', "-")];

    // Check project directory first
    if let Some(pdir) = project_dir {
        for dir_name in &dir_names {
            let path = std::path::Path::new(pdir).join("plugins").join(dir_name);
            if path.exists() {
                return Some(path);
            }
        }
    }

    // Check global directory
    if let Some(home) = dirs::home_dir() {
        for dir_name in &dir_names {
            let path = home.join(".cleen").join("plugins").join(dir_name);
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}

/// Tool: list_ecosystem - List all plugins in the Clean Language ecosystem
///
/// MAINTENANCE: When a new plugin is added to the Clean Language ecosystem,
/// add it to the `get_ecosystem_catalog()` function below. Each entry needs:
/// name, version, category, description, blocks, key_features, bridge_function_count,
/// install command, and status.
fn tool_list_ecosystem(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let category_filter = args.get("category").and_then(|v| v.as_str());

    let catalog = get_ecosystem_catalog();

    let mut plugins: Vec<serde_json::Value> = Vec::new();
    for entry in &catalog {
        if let Some(cat) = category_filter {
            if !cat.eq_ignore_ascii_case("all") && !entry.category.eq_ignore_ascii_case(cat) {
                continue;
            }
        }

        plugins.push(json!({
            "name": entry.name,
            "version": entry.version,
            "category": entry.category,
            "description": entry.description,
            "blocks": entry.blocks,
            "key_features": entry.key_features,
            "bridge_function_count": entry.bridge_function_count,
            "install": entry.install,
            "status": entry.status,
            "auto_detect_paths": entry.auto_detect_paths,
            "replaces": entry.replaces,
            "permissions": entry.permissions,
        }));
    }

    // Check which ones are actually installed
    let discovery = PluginDiscovery::new();
    let installed = discovery.discover_all().unwrap_or_default();
    let installed_names: Vec<String> = installed.keys().cloned().collect();

    for plugin in plugins.iter_mut() {
        let name = plugin["name"].as_str().unwrap_or("");
        plugin["installed"] = json!(installed_names.iter().any(|n| n == name));
    }

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "ecosystem": plugins,
            "total": catalog.len(),
            "installed_count": installed_names.len(),
            "note": "Use 'list_plugins' with project_dir to see installed plugins with full DSL details. Use 'get_plugin_examples' to read example code."
        }),
    )
}

struct EcosystemPlugin {
    name: &'static str,
    version: &'static str,
    category: &'static str,
    description: &'static str,
    blocks: &'static [&'static str],
    key_features: &'static [&'static str],
    bridge_function_count: usize,
    install: &'static str,
    status: &'static str,
    auto_detect_paths: &'static [&'static str],
    replaces: &'static str,
    /// Bridge function names this plugin declares in its [bridge] section.
    /// These are the only functions the plugin is permitted to call at runtime.
    permissions: &'static [&'static str],
}

/// ECOSYSTEM CATALOG — Add new plugins here
///
/// When a new plugin is created in the Clean Language ecosystem:
/// 1. Add an EcosystemPlugin entry to this function
/// 2. Include accurate block names, feature list, and bridge function count
/// 3. Set status to "stable", "beta", or "planned"
/// 4. Run `cargo test` to verify compilation
fn get_ecosystem_catalog() -> Vec<EcosystemPlugin> {
    vec![
        EcosystemPlugin {
            name: "frame.httpserver",
            version: "2.8.0",
            category: "server",
            description: "HTTP server plugin — routing, request context, response helpers, authentication guards, database migrations, and multi-tenant support. Define REST APIs with endpoints: blocks.",
            blocks: &["server", "endpoints", "migrate"],
            key_features: &[
                "HTTP route registration (GET, POST, PUT, PATCH, DELETE)",
                "Protected routes with role requirements",
                "Request context (params, query, headers, body, cookies)",
                "Response helpers (JSON, HTML, redirect, error)",
                "Response headers and caching control",
                "Authentication guard integration",
                "Database migration blocks (migrate:)",
                "Multi-tenant request routing",
                "Plugin lifecycle hooks (register_server, register_cli, register_build)",
                "Permission enforcement — only declared bridge functions may be called",
            ],
            bridge_function_count: 52,
            install: "cleen plugin add frame.httpserver",
            status: "stable",
            auto_detect_paths: &["/api/", "/backend/api/", "/server/api/", "/endpoints/"],
            replaces: "Express.js, Fastify, Koa, or any Node.js HTTP framework. Use frame.httpserver for all server-side HTTP handling.",
            permissions: &[
                "_http_route", "_http_get_param", "_http_get_query", "_http_get_header",
                "_http_get_body", "_http_get_cookie", "_http_set_cookie", "_http_respond_json",
                "_http_respond_html", "_http_respond_redirect", "_http_respond_error",
                "_http_set_header", "_http_set_cache", "_http_require_auth",
                "_http_tenant_id", "_http_tenant_config",
                "_migrate_run", "_migrate_rollback", "_migrate_status",
            ],
        },
        EcosystemPlugin {
            name: "frame.data",
            version: "2.8.0",
            category: "data",
            description: "ORM and database plugin — data models, CRUD operations, query builder, transactions, raw SQL, migrations, and validation constraints. Define models with data: blocks.",
            blocks: &["data", "migrate"],
            key_features: &[
                "Model definition with typed fields",
                "Query builder (find, first, count, insert, update, delete)",
                "Filtering, sorting, and pagination",
                "Transaction support (Data.tx:)",
                "Raw SQL queries (db.query:, db.queryAs)",
                "Database migrations (migrate: block)",
                "Field validation constraints (required, min, max, pattern, unique)",
                "Plugin lifecycle hooks (register_data, register_build)",
                "Permission enforcement — only declared bridge functions may be called",
            ],
            bridge_function_count: 8,
            install: "cleen plugin add frame.data",
            status: "stable",
            auto_detect_paths: &["/data/", "/models/", "/server/models/"],
            replaces: "Sequelize, Prisma, Knex, or any JS/TS ORM. Use frame.data for all database operations.",
            permissions: &[
                "_db_query", "_db_execute",
                "_db_migrate_run", "_db_migrate_rollback", "_db_migrate_status",
                "_db_transaction_begin", "_db_transaction_commit", "_db_transaction_rollback",
            ],
        },
        EcosystemPlugin {
            name: "frame.auth",
            version: "2.8.0",
            category: "auth",
            description: "Authentication and authorization plugin — sessions, JWT tokens, password hashing, roles/permissions, CSRF protection, and multi-tenant support. Configure with auth: blocks.",
            blocks: &["auth", "protected", "login", "roles", "tenant"],
            key_features: &[
                "Session management (store, get, delete)",
                "JWT token operations (sign, verify, decode)",
                "Password hashing (Argon2id)",
                "CSRF token management",
                "Role-based access control (RBAC)",
                "Permission checking",
                "Cookie management",
                "Environment variable access for secrets",
                "Multi-tenant auth (tenant: sub-block)",
                "Tenant isolation and per-tenant role configuration",
                "Plugin lifecycle hooks (register_server)",
                "Permission enforcement — only declared bridge functions may be called",
            ],
            bridge_function_count: 24,
            install: "cleen plugin add frame.auth",
            status: "stable",
            auto_detect_paths: &["/auth/", "/config/auth/"],
            replaces: "Passport.js, Auth0 SDK, or any JS authentication library. Use frame.auth for all authentication and authorization.",
            permissions: &[
                "_session_store", "_session_get", "_session_delete",
                "_jwt_sign", "_jwt_verify", "_jwt_decode",
                "_crypto_hash_password", "_crypto_verify_password",
                "_csrf_token_generate", "_csrf_token_verify",
                "_auth_check_role", "_auth_check_permission",
                "_cookie_set", "_cookie_get", "_cookie_delete",
                "_env_get",
                "_tenant_get_id", "_tenant_get_config",
                "_tenant_check_role", "_tenant_check_permission",
            ],
        },
        EcosystemPlugin {
            name: "frame.ui",
            version: "2.8.0",
            category: "ui",
            description: "HTML-first UI framework — components, screens, theming, event handling, two-way binding, and hydration. Define components with component: blocks and HTML templates.",
            blocks: &["component", "screen", "page", "styles", "ui"],
            key_features: &[
                "Component definitions with properties and slots",
                "HTML directives (cl-if, cl-else, cl-each, cl-bind, cl-show, cl-validate, cl-slot)",
                "Event handling (onclick, oninput, onsubmit, etc.)",
                "Event modifiers (prevent, stop, once, enter, escape)",
                "Interpolation ({{escaped}} and {{{raw}}})",
                "Hydration modes (off, on, visible, idle, only)",
                "Two-way data binding",
                "State management for components",
                "Global UI theming (ui: block)",
                "Plugin lifecycle hooks (register_server, register_build)",
                "Permission enforcement — only declared bridge functions may be called",
            ],
            bridge_function_count: 14,
            install: "cleen plugin add frame.ui",
            status: "stable",
            auto_detect_paths: &["/ui/", "/components/", "/screens/"],
            replaces: "JavaScript DOM manipulation, jQuery, vanilla JS event handlers, JS form validation, JS state management. Use frame.ui instead of writing any .js files for client-side interactivity.",
            permissions: &[
                "_ui_render", "_ui_hydrate", "_ui_bind", "_ui_emit_event",
                "_ui_get_state", "_ui_set_state",
                "_dom_get_element", "_dom_set_attribute", "_dom_add_class", "_dom_remove_class",
                "_ui_theme_get", "_ui_theme_set",
                "_ui_validate_field", "_ui_slot_render",
            ],
        },
        EcosystemPlugin {
            name: "frame.canvas",
            version: "2.8.0",
            category: "canvas",
            description: "Canvas rendering and game development plugin — drawing, animation, audio, sprites, pointer/keyboard input, collision detection, and easing functions. Define scenes with canvasScene: blocks.",
            blocks: &["canvasScene", "draw", "onFrame", "onPointerDown", "onPointerMove", "onKeyDown"],
            key_features: &[
                "Drawing primitives (circles, rectangles, lines, polygons)",
                "Text and image rendering",
                "Transform operations (translate, rotate, scale)",
                "Animation frame management",
                "Audio (sound effects and music with volume/pan control)",
                "Sprite sheet support",
                "Input handling (mouse, keyboard, touch, gamepad)",
                "Pointer event blocks (onPointerDown, onPointerMove)",
                "Keyboard event blocks (onKeyDown)",
                "Collision detection (7 types including raycasting)",
                "Camera and viewport control",
                "20 easing functions",
                "Scene management",
                "Plugin lifecycle hooks (register_server, register_build)",
                "Permission enforcement — only declared bridge functions may be called",
            ],
            bridge_function_count: 134,
            install: "cleen plugin add frame.canvas",
            status: "stable",
            auto_detect_paths: &["/canvas/"],
            replaces: "JavaScript Canvas API, JS game libraries (Phaser, PixiJS). Use frame.canvas instead of writing JS for any canvas/graphics work.",
            permissions: &[
                "_canvas_clear", "_canvas_fill_rect", "_canvas_stroke_rect",
                "_canvas_fill_circle", "_canvas_stroke_circle",
                "_canvas_fill_text", "_canvas_draw_image",
                "_canvas_translate", "_canvas_rotate", "_canvas_scale",
                "_canvas_save", "_canvas_restore",
                "_canvas_set_fill_color", "_canvas_set_stroke_color", "_canvas_set_line_width",
                "_canvas_request_frame", "_canvas_get_time",
                "_audio_play", "_audio_stop", "_audio_set_volume",
                "_input_mouse_x", "_input_mouse_y", "_input_mouse_button",
                "_input_key_pressed", "_input_key_held",
                "_input_pointer_x", "_input_pointer_y", "_input_pointer_down",
                "_input_key_down", "_input_key_code",
                "_sprite_draw", "_sprite_set_frame",
                "_collision_check_rect", "_collision_check_circle",
                "_collision_raycast",
                "_camera_set_position", "_camera_set_zoom",
                "_scene_push", "_scene_pop",
            ],
        },
    ]
}

/// Tool: get_specification - Query the Clean Language Specification
fn tool_get_specification(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let section_query = args.get("section").and_then(|v| v.as_str());
    let search_query = args.get("query").and_then(|v| v.as_str());

    // Get or initialize the specification cache
    let spec_path = match get_spec_path() {
        Some(path) => path,
        None => {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": "Specification file not found. Set CLN_SPEC_PATH or ensure documentation/Clean_Language_Specification.md exists."
                }),
            );
        }
    };

    let mut cache = SPEC_CACHE.lock().unwrap();
    if cache.is_none() {
        *cache = SpecificationCache::load(&spec_path);
    }

    let cache_ref = match cache.as_mut() {
        Some(c) => c,
        None => {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": "Failed to load specification file."
                }),
            );
        }
    };

    let sections = cache_ref.get_or_reload();

    // If no parameters, return table of contents
    if section_query.is_none() && search_query.is_none() {
        let toc: Vec<serde_json::Value> = sections
            .iter()
            .map(|s| {
                json!({
                    "title": s.title,
                    "line": s.line_start
                })
            })
            .collect();

        return JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "table_of_contents": toc,
                "total_sections": sections.len()
            }),
        );
    }

    // Filter by section name
    let mut results: Vec<&SpecSection> = sections.iter().collect();

    if let Some(section) = section_query {
        let section_lower = section.to_lowercase();
        results.retain(|s| s.title.to_lowercase().contains(&section_lower));
    }

    // Filter by search query
    if let Some(query) = search_query {
        let query_lower = query.to_lowercase();
        results.retain(|s| s.content.to_lowercase().contains(&query_lower));
    }

    let result_json: Vec<serde_json::Value> = results
        .iter()
        .map(|s| {
            json!({
                "title": s.title,
                "content": s.content,
                "line": s.line_start
            })
        })
        .collect();

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "sections": result_json,
            "count": result_json.len()
        }),
    )
}

/// Tool: list_builtins - List all built-in functions, classes, and namespaces
fn tool_list_builtins(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let category_filter = args.get("category").and_then(|v| v.as_str());
    let namespace_filter = args.get("namespace").and_then(|v| v.as_str());

    let registry = BuiltinRegistry::new();
    let (global_count, class_count, namespace_count, total_methods) = registry.stats();

    // Build global functions list
    let mut global_functions = Vec::new();
    for (name, func) in registry.functions.iter() {
        // Filter by category if specified
        if let Some(cat) = category_filter {
            let cat_lower = cat.to_lowercase();
            let func_cat = format!("{:?}", func.category).to_lowercase();
            if !func_cat.contains(&cat_lower) {
                continue;
            }
        }

        let params: Vec<String> = func.parameters.iter().map(|p| format!("{:?}", p)).collect();

        global_functions.push(json!({
            "name": name,
            "parameters": params,
            "return_type": format!("{:?}", func.return_type),
            "category": format!("{:?}", func.category)
        }));
    }

    // Build namespaces list
    let mut namespaces = Vec::new();
    for (ns_name, ns) in registry.namespaces.iter() {
        // Filter by namespace if specified
        if let Some(ns_filter) = namespace_filter {
            if !ns_name.eq_ignore_ascii_case(ns_filter) {
                continue;
            }
        }

        let mut ns_functions = Vec::new();
        for func in ns.functions.iter() {
            // Filter by category if specified
            if let Some(cat) = category_filter {
                let cat_lower = cat.to_lowercase();
                let func_cat = format!("{:?}", func.category).to_lowercase();
                if !func_cat.contains(&cat_lower) {
                    continue;
                }
            }

            let params: Vec<String> = func.parameters.iter().map(|p| format!("{:?}", p)).collect();

            ns_functions.push(json!({
                "name": func.name,
                "parameters": params,
                "return_type": format!("{:?}", func.return_type),
                "category": format!("{:?}", func.category)
            }));
        }

        if !ns_functions.is_empty() || namespace_filter.is_some() {
            namespaces.push(json!({
                "name": ns_name,
                "functions": ns_functions
            }));
        }
    }

    // Build classes list
    let mut classes = Vec::new();
    for (class_name, class) in registry.classes.iter() {
        let mut methods = Vec::new();
        for method in class.methods.iter() {
            let params: Vec<String> = method
                .parameters
                .iter()
                .map(|p| format!("{:?}", p))
                .collect();

            methods.push(json!({
                "name": method.name,
                "parameters": params,
                "return_type": format!("{:?}", method.return_type),
                "is_static": method.is_static
            }));
        }

        classes.push(json!({
            "name": class_name,
            "methods": methods
        }));
    }

    let summary = json!({
        "global_functions": global_count,
        "classes": class_count,
        "namespaces": namespace_count,
        "total_methods": total_methods
    });

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "summary": summary,
            "global_functions": global_functions,
            "namespaces": namespaces,
            "classes": classes
        }),
    )
}

/// Tool: list_error_codes - List all compiler error codes
fn tool_list_error_codes(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let category_filter = args.get("category").and_then(|v| v.as_str());

    let catalog = get_error_catalog();

    let mut filtered_codes = Vec::new();
    for entry in catalog.iter() {
        // Filter by category if specified
        if let Some(cat) = category_filter {
            if !entry.category.eq_ignore_ascii_case(cat) {
                continue;
            }
        }

        filtered_codes.push(json!({
            "code": entry.code,
            "category": entry.category,
            "title": entry.title,
            "description": entry.description,
            "example": entry.example,
            "fix": entry.fix
        }));
    }

    // Count by category
    let mut category_counts = std::collections::HashMap::new();
    for entry in catalog.iter() {
        *category_counts.entry(entry.category).or_insert(0) += 1;
    }

    let summary = json!({
        "total_codes": catalog.len(),
        "by_category": category_counts
    });

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "summary": summary,
            "error_codes": filtered_codes,
            "count": filtered_codes.len()
        }),
    )
}

/// Tool: get_quick_reference - Concise cheat sheet for writing Clean Language programs
fn tool_get_quick_reference(id: serde_json::Value) -> JsonRpcResponse {
    let quick_ref = r#"# Clean Language Quick Reference

## Specification Authority

This reference defines how Clean Language code MUST be written.
- Every pattern shown here is the ONLY correct way to perform that operation
- Clean Language follows the "one way to do things" principle — for each operation, there is exactly one correct syntax
- Do NOT use alternative syntax, underscore bridge names, or workarounds
- Do NOT invent shortcuts, fallbacks, or compatibility patterns
- If spec-correct code does not compile, it is a COMPILER BUG — not a code problem
- When spec-correct code fails: call `report_error` to report the bug, then write the code per spec anyway
- NEVER modify application code to accommodate compiler limitations
- NEVER use underscore bridge names (_db_query, _http_respond) in application code — always use dot notation (db.query, http.respond)

## Recommended Application Stack
Clean Language applications should use Clean Language for ALL layers. Do NOT use JavaScript, TypeScript, or other languages when a Clean Language plugin provides the same capability.

| Layer            | Plugin           | Purpose                                         | Replaces                          |
|------------------|------------------|-------------------------------------------------|-----------------------------------|
| HTTP Server      | frame.httpserver | Routing, request/response, middleware, auth      | Express.js, Fastify, Koa          |
| Database / ORM   | frame.data       | Queries, CRUD, migrations, transactions          | Sequelize, Prisma, Knex           |
| Frontend UI      | frame.ui         | DOM manipulation, events, hydration, components  | jQuery, vanilla JS, React         |
| Graphics / Games | frame.canvas     | Canvas rendering, animation, sprites, input      | Phaser, PixiJS, JS Canvas API     |
| Authentication   | frame.auth       | Sessions, JWT, roles, CSRF, password hashing     | Passport.js, Auth0 SDK            |

### When to use JavaScript
- NEVER for DOM manipulation (use frame.ui)
- NEVER for event handling (use frame.ui directives: onclick, oninput, onsubmit)
- NEVER for form validation (use frame.ui)
- NEVER for canvas/graphics (use frame.canvas)
- NEVER for HTTP servers (use frame.httpserver)
- NEVER for database access (use frame.data)
- ONLY as a last resort when no Clean Language plugin covers the specific browser API needed

### Naming Convention — IMPORTANT
Clean Language uses **dot notation** for all function calls: `string.length()`, `math.abs()`, `list.add()`.
NEVER use underscore-style names like `_ui_update_element` or `string_compare` in Clean Language code.
Underscore names are internal host bridge identifiers — they are NOT part of the Clean Language API.
Always use the dot-notation form shown in the plugin documentation and this reference.
Use `list.length()` (not `list.size()`) to get the number of elements in a list.

Call `get_stack_recommendation` with a project type for a tailored plugin list and file structure.

## Program Structure
Every program needs a `start:` block as entry point. Use tabs for indentation.

```
start:
	print("Hello, World!")
```

## Types
- `integer` — 32-bit signed integer (use `integer:64` for large values)
- `number` — 64-bit float
- `string` — UTF-8 text
- `boolean` — true/false
- `list<T>` — typed resizable list (e.g., `list<integer>`, `list<string>`)
- `matrix<T>` — 2D list (e.g., `matrix<number>`)
- `pairs` — key-value associative container (e.g., `pairs<string, integer>`)
- `any` — generic type parameter used in function/class definitions

## Variables
```
integer age = 25
number pi = 3.14159
string name = "Clean"
boolean active = true
list<integer> nums = [1, 2, 3]
matrix<number> grid = [[1.0, 2.0], [3.0, 4.0]]
pairs<string, integer> scores = {}
```

## Apply-Blocks
Apply a function or type to each indented item:
```
// Call print on multiple values
print:
	"Hello"
	"World"

// Declare multiple variables of the same type
integer:
	count = 0
	maxSize = 100

// Add multiple items to a list
nums.add:
	10
	20
	30
```

## Functions
Declared in a `functions:` block. Return type comes first.
```
functions:
	integer add(integer a, integer b)
		return a + b

	string greet(string name)
		return "Hello, " + name

start:
	print(add(3, 4).toString())
	print(greet("World"))
```

## Control Flow
```
// If/else — no parentheses required around condition       // ✓ v0.28.0+
if x > 0
	print("positive")
else if x == 0
	print("zero")
else
	print("negative")

// Iterate over a range                                     // ✓ v0.30.0+
iterate i in 1 to 10
	print(i.toString())

// Iterate with step                                        // ✓ v0.30.0+
iterate k in 10 to 1 step -2
	print(k.toString())

// Iterate over a list                                      // ✓ v0.30.0+
iterate item in myList
	print(item.toString())

// While loop (condition-based)                             // ✓ v0.28.0+
integer i = 0
while i < 10
	print(i.toString())
	i = i + 1

// Repeat loop (infinite — use break to exit)               // ✓ v0.28.0+
repeat
	string line = input("Enter text: ")
	if line == "quit"
		break
	print(line)
```

## Null Handling
```
// Null-coalescing with 'default' operator
string username = userData.name default "Guest"
integer count = config.maxItems default 100

// Chain defaults
string value = primary default secondary default "fallback"

// Required assertion — fails at runtime if null
string name = maybeNull!

// Safe navigation with ?.
string city = user?.address?.city default "Unknown"

// Null-coalescing operator ??
string display = name ?? "Anonymous"
```

## String Operations
```
string s = "hello world"
integer len = s.length()
string upper = s.toUpperCase()
string lower = s.toLowerCase()
string sub = s.substring(0, 5)
boolean has = s.contains("world")
string replaced = s.replace("world", "Clean")
list<string> parts = s.split(" ")
string trimmed = s.trim()
integer idx = s.indexOf("world")
```

## Math Operations
```
number result = math.sqrt(16.0)
number sine = math.sin(3.14)
number cosine = math.cos(0.0)
number power = math.pow(2.0, 10.0)
number absolute = math.abs(-5.0)
number rounded = math.round(3.7)
number floored = math.floor(3.9)
number ceiled = math.ceil(3.1)
number minimum = math.min(5.0, 3.0)
number maximum = math.max(5.0, 3.0)
```

## Type Conversions
```
integer x = 42
string s = x.toString()
number n = x.toNumber()
integer back = n.toInteger()
integer parsed = "123".toInteger()
number fparsed = "3.14".toNumber()
```

## Print Output
```
print("text")              // print without newline
print("value: " + x.toString())  // string concatenation
print(x.toString()) +      // print with newline (note the '+' after the closing paren)
printl("text")             // print with newline (alternate form)
```

## Lists
```
list<integer> nums = [1, 2, 3, 4, 5]
integer first = nums[0]
integer len = nums.length()
nums.add(6)                // add to end
nums.remove(0)             // remove at index
boolean has = nums.contains(3)
list<integer> sorted = nums.sort()

// List behaviors (set .type property)
list<string> queue = []
queue.type = "line"        // FIFO queue: add to back, remove from front
list<string> stack = []
stack.type = "pile"        // LIFO stack: add/remove from top
list<string> unique = []
unique.type = "unique"     // Set: no duplicates allowed
```

## Classes
```
class Person
	string name
	integer age

	constructor(string nameParam, integer ageParam)
		name = nameParam
		age = ageParam

	functions:
		string greet()
			return "I'm " + name

start:
	Person p = Person("Alice", 30)
	print(p.greet())
	print(p.name)
```

## Inheritance
```
class Animal
	string name

	constructor(string nameParam)
		name = nameParam

	functions:
		string speak()
			return name + " speaks"

class Dog is Animal
	string breed

	constructor(string nameParam, string breedParam)
		base(nameParam)
		breed = breedParam

	functions:
		string speak()
			return name + " barks"
```

## State Management
Top-level reactive state — persists for app lifetime.
```
state:
	integer count = 0
	string username = ""

	// Invariants checked at operation boundaries
	rules:
		count >= 0

	// Computed (auto-updates when dependencies change)
	computed:
		string display
			return "Count: " + count.toString()

// Watch for changes
watch count:
	print("Count changed to: " + count.toString())

watch (firstName, lastName):
	print("Name changed")
```

## Tests Block
```
tests:
	"adds numbers": add(2, 3) = 5
	"squares a number": square(4) = 16
	"hi".toUpperCase() = "HI"
	math.abs(-42) = 42
```

## Contracts (require)
```
functions:
	integer divide(integer a, integer b)
		require b != 0
		return a / b

	void setAge(integer age)
		require age >= 0
		require age <= 150
		// implementation
```

## Error Handling (onError)
```
functions:
	integer divide(integer a, integer b)
		if b == 0
			error("Cannot divide by zero")
		return a / b

start:
	integer result = divide(10, 0) onError 0
```

## Computed Properties (on classes)
```
class Circle
	number radius

	constructor(number radiusParam)
		radius = radiusParam

	computed:
		number area = 3.14159 * radius * radius
		number circumference = 2.0 * 3.14159 * radius
```

## JSON Operations                                          // ✓ v0.30.0+
```
string jsonStr = json.dataToText(data)
any parsed = json.textToData(jsonStr)
any safe = json.tryTextToData(maybeJson)
```

## Description (documentation)
```
functions:
	integer factorial(integer n)
		description "Calculates the factorial of n"
		require n >= 0
		if n <= 1
			return 1
		return n * factorial(n - 1)
```

## AI Metadata (spec/intent)
```
functions:
	integer divide(integer a, integer b)
		spec "math/division"
		intent "Safely divides two integers"
		require b != 0
		return a / b
```

## Available MCP Tools
- `get_quick_reference` — This guide (call first)
- `get_architecture` — Execution layers, host bridge, memory model
- `check` — Type-check code (fast, no WASM)
- `compile` — Compile to WebAssembly
- `parse` — Get AST as JSON
- `diagnostics` — Get detailed errors
- `explain_error` — Explain an error code
- `list_functions` — List functions in source
- `list_types` — List classes in source
- `list_builtins` — All built-in functions/methods
- `get_specification` — Full language spec by section
- `list_error_codes` — All error codes
- `list_plugins` — Installed plugins with full DSL syntax
- `list_ecosystem` — ALL available plugins (installed or not)
- `get_plugin_examples` — Read plugin example files
- `get_stack_recommendation` — Recommended plugin stack for a project type
- `report_error` — Report a likely compiler bug
- `check_reported_fixes` — Check if your reported bugs are fixed

## Workflow
1. Call `get_quick_reference` (this tool) to learn base syntax
2. Call `get_stack_recommendation` with your project type (web-app, api, game, cli) — includes best practices for web apps
3. Call `list_ecosystem` to see ALL available plugins in the ecosystem
4. Call `list_plugins` to see installed plugins with full DSL details
5. Call `get_plugin_examples` to see plugin usage patterns
6. Write .cln code following the patterns above — spec-correct code ONLY
7. Call `check` to type-check (fast feedback loop)
8. Call `compile` when ready for WASM output
9. If errors occur, call `explain_error` with the code
10. If the code follows the spec but doesn't compile, call `report_error` immediately — do NOT write workaround code
11. Use `get_specification` for detailed docs on specific features
12. Call `check_reported_fixes` at session start to see if previously reported bugs are now fixed

## Plugin Syntax Discovery
Plugins (like Frame) add custom blocks, keywords, and types.
Call `list_plugins` to see what each plugin provides:
- blocks: DSL block names (endpoints:, data:, component:)
- keywords: Custom keywords (find, where, route)
- types: Custom types (Model, Endpoint)
- functions: Plugin functions (Data.tx, Http.route)
- ai.examples: Example files you can read with `get_plugin_examples`

## Web Application Patterns

### Feature Status (v0.30.27)
- html: blocks in functions ✓
- Plugin dot-notation (req.query, http.respond, db.query) ✓
- iterate i in 0 to count ✓
- iterate item in list ✓
- json.get() for JSON field access ✓
- Response helpers (json(), html(), redirect()) ✓
- html: block as implicit return ✓
- {var} and {!var} interpolation in html: blocks ✓

### HTML Generation (Server-Side Rendering)

NEVER build HTML via string concatenation:
    // WRONG — anti-pattern
    string page = "<html><head><title>" + title + "</title></head><body>" + content + "</body></html>"

ALWAYS use html: blocks from frame.ui:
    // CORRECT — spec-compliant
    html:
        <html>
        <head><title>{title}</title></head>
        <body>{!content}</body>
        </html>

Rules:
- {var} for escaped text interpolation (safe for user content)
- {!var} for raw HTML insertion (pre-built HTML fragments from other html: blocks)
- html: block at the end of a function is the implicit return value
- For intermediate HTML fragments, use helper functions that each return an html: block
- Double quotes for HTML attributes inside html: blocks (single quotes cause lexer errors)

### Database Queries — Data Only

NEVER generate HTML inside SQL queries:
    // WRONG — HTML in SQL
    string sql = "SELECT CONCAT('<div class=''card''><h3>', title, '</h3></div>') as html FROM ..."

ALWAYS query data and render with html: blocks:
    // CORRECT — data-only SQL
    string sql = "SELECT CAST(title AS CHAR) as title, CAST(description AS CHAR) as description FROM ..."
    string result = db.query(sql, params)

    // Render with html: block
    html:
        <div class="card">
            <h3>{title}</h3>
            <p>{description}</p>
        </div>

### Iterating Over Database Results

For sections with repeating items (cards, lists, grids):

1. Query data rows (no HTML):
    string items_sql = "SELECT title, description FROM ... JSON_TABLE(...) ORDER BY ord"
    string items = db.query(items_sql, params)

2. Query count:
    string count_sql = "SELECT CAST(COUNT(*) AS CHAR) as cnt FROM ... JSON_TABLE(...)"
    integer count = json_get(db.query(count_sql, params), "cnt").toInteger()

3. Iterate and render:
    string items_html = ""
    iterate i in 0 to count
        string idx = i.toString()
        string title = json.get(items, idx + ".title")
        string desc = json.get(items, idx + ".description")
        items_html = items_html + render_card(title, desc)

4. Insert into page template:
    html:
        <div class="grid">
            {!items_html}
        </div>

### Reusable HTML Components

Create small functions with html: blocks for repeated UI patterns:

    string render_card(string title, string description)
        html:
            <div class="card">
                <h3 class="card-title">{title}</h3>
                <p class="card-description">{description}</p>
            </div>

Use from page templates:
    items_html = items_html + render_card(title, desc)

### Page File Structure

Each page should be a separate .cln file in app/pages/:

    app/pages/
        helpers.cln      — shared HTML helpers (build_head, build_nav, component renderers)
        home.cln         — render_home() function
        about.cln        — render_about() function
        ...

Each page function:
1. Fetches data from database (data-only SQL)
2. Extracts values with json_get() or json.get()
3. Builds section HTML via iterate loops + component helpers
4. Returns full page via html: block at end of function

    string render_home()
        string lang = get_lang()
        // ... fetch data, build section HTML fragments ...
        string head = build_head(lang, title, desc, "/")
        string nav = build_nav(lang, "home")
        string footer = build_footer(lang)
        string scripts = build_page_scripts()

        html:
            {!head}
            {!nav}
            <main>
                <section class="hero">
                    <h1>{hero_title}</h1>
                </section>
                <section class="section">
                    <div class="container">
                        {!features_html}
                    </div>
                </section>
            </main>
            {!footer}
            {!scripts}

### HTTP Response Pattern

For JSON API responses:
    return json(body)                              // 200 JSON
    return error(code, message)                    // Error JSON

For HTML page responses:
    html:                                          // html: block is implicit return
        <html>...</html>

For other content types:
    return http.respond(200, "application/xml", xml)   // XML, text/plain, etc.
"#;

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "quick_reference": quick_ref,
            "version": crate::VERSION,
            "tools_available": 18,
            "tip": "Use 'check' for fast iteration, 'compile' when ready for WASM. If spec-correct code fails, call 'report_error' immediately — never write workarounds. Call 'check_reported_fixes' at session start to see resolved bugs."
        }),
    )
}

/// Tool: get_architecture - Return platform architecture, execution layers, host bridge summary
fn tool_get_architecture(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let section = args
        .get("section")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    let layers_content = r#"## Execution Layers

| Layer | Component | Responsibility |
|-------|-----------|----------------|
| Layer 0 | Compiler | Parse, analyze, generate WASM imports (NOT implementations) |
| Layer 1 | WASM Runtime | Pure computation (math intrinsics, memory ops) |
| Layer 2 | Host Bridge | Portable I/O (console, file, HTTP client, DB, crypto) |
| Layer 3 | Server Extensions | Server-only (HTTP routing, request context, sessions) |
| Layer 4 | Plugins | Custom bridge functions via plugin.toml |
| Layer 5 | Framework/Apps | High-level abstractions |

**Rule:** If a function needs external I/O, it belongs in Layer 2+, NOT in the compiler.

### Compiler Registry (Layer 0 — what belongs here)
- Language built-ins: print, printl, input
- Math operations: math.abs, math.sqrt, math.pow, math.sin, math.cos
- Type conversions: toString, toInteger, toNumber
- Namespaces: math.*, string.*, list.*, json.*
- Pure WASM operations

### What does NOT belong in the compiler
- HTTP routing → Layer 3 / frame.httpserver plugin
- Database queries → Layer 2 / frame.data plugin
- File I/O → Layer 2 host bridge
- Session management → Layer 3 / frame.auth plugin
- DOM manipulation → Layer 4 / frame.ui plugin
- Canvas drawing → Layer 4 / frame.canvas plugin
"#;

    let bridge_content = r#"## Host Bridge Function Categories (Layer 2)

### Console I/O
- `print(ptr, len)` — Print string without newline
- `printl(ptr, len)` — Print string with newline
- `print_integer(value: i64)` — Print integer
- `print_float(value: f64)` — Print float
- `print_boolean(value: i32)` — Print "true" or "false"
- `input(prompt_ptr, prompt_len) -> i32` — Read line from user
- `input_integer(prompt_ptr, prompt_len) -> i64` — Read and parse integer
- `input_float(prompt_ptr, prompt_len) -> f64` — Read and parse float
- `input_yesno(prompt_ptr, prompt_len) -> i32` — Read yes/no as boolean

### Math (30+ functions)
- All math functions use f64: `math_sin`, `math_cos`, `math_tan`, `math_sqrt`, `math_pow`
- Available as both dot notation (`math.sqrt`) and underscore form (`math_sqrt`)
- Signature conventions: `(x: f64) -> f64` for unary, `(a: f64, b: f64) -> f64` for binary

### String Operations (25+ functions)
- String parameters always passed as `(ptr: i32, len: i32)` pairs
- String returns are i32 pointers to length-prefixed data
- Functions: concat, substring, length, toUpperCase, toLowerCase, trim, split, replace, etc.
- Integer operations use `i64`: `print_integer`, `int_to_string`, `string_to_int`

### File I/O
- `file_read(path_ptr, path_len) -> i32` — Read file to string
- `file_write(path_ptr, path_len, data_ptr, data_len)` — Write string to file
- `file_exists(path_ptr, path_len) -> i32` — Check file existence
- `file_delete(path_ptr, path_len)` — Delete file

### HTTP Client
- `http_get(url_ptr, url_len) -> i32` — HTTP GET request
- `http_post(url_ptr, url_len, body_ptr, body_len) -> i32` — HTTP POST request
- `http_put`, `http_delete`, `http_patch` — Other HTTP methods

### Crypto
- `crypto_hash(data_ptr, data_len) -> i32` — Hash data (SHA-256)
- `crypto_random_bytes(len: i32) -> i32` — Generate random bytes
- `crypto_uuid() -> i32` — Generate UUID

### JSON
- `json_parse(ptr, len) -> i32` — Parse JSON string to internal representation
- `json_stringify(ptr, len) -> i32` — Serialize to JSON string

### WAT Spec Compliance
- String parameters: `(ptr: i32, len: i32)` pairs
- Integer values: `i64` for print_integer, int_to_string, string_to_int
- Reference: clean-server/host-bridge/tests/spec_compliance.wat
"#;

    let memory_content = r#"## WASM Memory Model

### Linear Memory Layout
```
[0 ...... 64KB]        Reserved / stack space
[64KB .. HEAP_START]   Static data (string pool, globals)
[HEAP_START .. top]    Dynamic heap (bump allocator)
```

### String Format
- Strings in WASM memory are length-prefixed: `[4-byte length][UTF-8 bytes]`
- Host functions receive `(ptr: i32, len: i32)` pairs pointing into WASM linear memory
- Returned strings from host functions are pointers to length-prefixed data in WASM memory

### Bump Allocator
- All heap allocations use a simple bump allocator
- Pointer advances forward on each allocation
- No garbage collection — WASM module lifetime manages memory
- The runtime provides `_mem_alloc(size: i32) -> i32` for dynamic allocation

### Imported vs Exported Memory
- WASM module imports memory from the host: `(import "env" "memory" (memory 1))`
- Host bridge functions read/write into this shared memory space
- This enables zero-copy string passing between WASM and host

### Important for Plugin Authors
- Plugin bridge functions receive string pointers into WASM memory
- Read strings from WASM memory using the shared memory view
- Write response strings back using `_mem_alloc` then writing into memory
- Never hold references to WASM memory across async boundaries
"#;

    let content = match section {
        "layers" => layers_content.to_string(),
        "bridge" => bridge_content.to_string(),
        "memory" => memory_content.to_string(),
        _ => format!(
            "{}\n\n{}\n\n{}",
            layers_content, bridge_content, memory_content
        ),
    };

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "section": section,
            "architecture": content,
            "tip": "Use 'layers' for execution layer placement rules, 'bridge' for host function signatures, 'memory' for WASM memory layout details."
        }),
    )
}

/// Tool: get_stack_recommendation - Recommended plugin stack for a project type
fn tool_get_stack_recommendation(
    id: serde_json::Value,
    args: &serde_json::Value,
) -> JsonRpcResponse {
    let project_type = match args.get("project_type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'project_type' parameter. Use: 'web-app', 'api', 'game', or 'cli'"
                    .to_string(),
            )
        }
    };

    let (plugins, structure, do_not_use, description, best_practices) = match project_type {
        "web-app" => (
            vec!["frame.httpserver", "frame.data", "frame.ui", "frame.auth"],
            json!({
                "app/server/main.cln": "Server entry point with routes and middleware",
                "app/client/main.cln": "Client-side interactivity (compiled to WASM, served to browser)",
                "app/data/models.cln": "Data models and database schema",
                "app/auth/auth.cln": "Authentication configuration and guards",
                "public/css/": "Stylesheets (CSS is acceptable, it's not logic)",
                "public/images/": "Static assets"
            }),
            vec![
                "JavaScript for DOM manipulation — use frame.ui (ui.update, ui.onEvent)",
                "JavaScript for event handling — use frame.ui directives (onclick, oninput, onsubmit)",
                "JavaScript for form validation — use frame.ui (ui.validate, ui.bindInput)",
                "JavaScript for state management — use frame.ui (ui.setState, ui.getState)",
                "Node.js/Express/Fastify — use frame.httpserver",
                "Any JS ORM (Sequelize, Prisma, Knex) — use frame.data",
                "Any JS auth library (Passport.js, Auth0 SDK) — use frame.auth",
                "Underscore-style function names — always use dot notation (string.length, math.abs, list.push)",
            ],
            "Full-stack web application with server, database, authentication, and client-side UI — all in Clean Language",
            Some(vec![
                "ALWAYS use html: blocks for HTML generation — NEVER build HTML via string concatenation",
                "Use {var} for escaped text interpolation, {!var} for raw HTML insertion in html: blocks",
                "html: block at end of function is the implicit return value",
                "SQL queries must return DATA only — NEVER generate HTML inside SQL queries",
                "For repeating items: query data, query count, iterate with helper functions, insert with {!html_var}",
                "Create small functions with html: blocks for reusable UI components (render_card, render_nav, etc.)",
                "Each page should be a separate .cln file in app/pages/ with a render_pagename() function",
                "Page functions: fetch data → extract values → build section HTML → return full page via html: block",
                "Double quotes for HTML attributes inside html: blocks (single quotes cause lexer errors)",
                "For JSON API responses use return json(body), for HTML pages use html: block as implicit return",
            ]),
        ),
        "api" => (
            vec!["frame.httpserver", "frame.data", "frame.auth"],
            json!({
                "app/server/main.cln": "Server entry point with API routes",
                "app/data/models.cln": "Data models and database schema",
                "app/auth/auth.cln": "Authentication configuration and guards",
                "app/server/middleware.cln": "Custom middleware"
            }),
            vec![
                "Node.js/Express/Fastify — use frame.httpserver",
                "Any JS ORM (Sequelize, Prisma, Knex) — use frame.data",
                "Any JS auth library (Passport.js, Auth0 SDK) — use frame.auth",
            ],
            "REST API backend with database and authentication — all in Clean Language",
            None::<Vec<&str>>,
        ),
        "game" => (
            vec!["frame.canvas"],
            json!({
                "app/game/main.cln": "Game entry point with scene setup",
                "app/game/scenes/": "Game scenes (menu, gameplay, etc.)",
                "public/assets/sprites/": "Sprite sheets and images",
                "public/assets/audio/": "Sound effects and music"
            }),
            vec![
                "JavaScript Canvas API — use frame.canvas drawing primitives",
                "JS game libraries (Phaser, PixiJS) — use frame.canvas",
                "JavaScript for input handling — use frame.canvas input functions",
                "JavaScript for animation — use frame.canvas onFrame and easing",
            ],
            "Canvas-based game or interactive graphics application — all in Clean Language",
            None::<Vec<&str>>,
        ),
        "cli" => (
            vec![],
            json!({
                "app/main.cln": "CLI entry point with start: block",
                "app/lib.cln": "Shared library functions"
            }),
            vec![
                "Node.js for CLI tools — use Clean Language with start: block",
            ],
            "Command-line application compiled to WebAssembly — no plugins needed for basic CLI",
            None::<Vec<&str>>,
        ),
        _ => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                format!(
                    "Unknown project type '{}'. Use: 'web-app', 'api', 'game', or 'cli'",
                    project_type
                ),
            )
        }
    };

    let install_commands: Vec<String> = plugins
        .iter()
        .map(|p| format!("cleen plugin add {}", p))
        .collect();

    let mut response_value = json!({
        "success": true,
        "project_type": project_type,
        "description": description,
        "plugins": plugins,
        "install_commands": install_commands,
        "structure": structure,
        "do_not_use": do_not_use,
        "important": "Clean Language applications should use Clean Language for ALL layers. Do NOT use JavaScript, TypeScript, or other languages when a Clean Language plugin provides the same capability.",
        "principle": "Write code exactly as the specification defines. If it doesn't compile, the toolchain has a bug — call report_error to report it. Never write workarounds, never use underscore bridge names, never invent alternative syntax.",
        "next_steps": [
            "Install plugins with the commands listed in 'install_commands'",
            "Call 'list_plugins' to see full DSL syntax for each plugin",
            "Call 'get_plugin_examples' with each plugin name to see usage patterns",
            "Write all application code in .cln files — no .js files needed",
            "If spec-correct code doesn't compile, call 'report_error' — do NOT write workarounds"
        ]
    });

    if let Some(practices) = best_practices {
        response_value["best_practices"] = json!(practices);
    }

    JsonRpcResponse::success(id, response_value)
}

// ============================================================================
// Error Reporting & Fix Notification Tool Handlers
// ============================================================================

/// Tool: report_error - Submit a structured error report
fn tool_report_error(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    use crate::telemetry::{
        report::{ReportAiContext, ReportError, ReportReproduction},
        ErrorReport, ReportStore,
    };

    // Extract required fields
    let error_code = match args.get("error_code").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'error_code' parameter".to_string(),
            )
        }
    };

    let error_message = match args.get("error_message").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'error_message' parameter".to_string(),
            )
        }
    };

    let component = match args.get("component").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'component' parameter".to_string(),
            )
        }
    };

    let severity = match args.get("severity").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'severity' parameter".to_string(),
            )
        }
    };

    // Extract optional fields
    let consent_level = args
        .get("consent_level")
        .and_then(|v| v.as_str())
        .unwrap_or("error_with_code");
    let user_contact = args
        .get("user_contact")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Determine error category from code prefix
    let category = if error_code.starts_with("SYN") {
        "syntax"
    } else if error_code.starts_with("SEM") {
        "semantic"
    } else if error_code.starts_with("COM") {
        "codegen"
    } else if error_code.starts_with("RUN") {
        "runtime"
    } else if error_code.starts_with("SYS") {
        "system"
    } else {
        "unknown"
    };

    // Generate report ID
    let report_id = generate_report_id();

    // Build the report
    let mut report = ErrorReport::new(
        report_id.clone(),
        ReportError {
            code: error_code,
            category: category.to_string(),
            component,
            severity,
            message: error_message,
            file_context: None,
        },
        "mcp_ai",
        consent_level,
    );

    // Set user contact: prefer explicit param, fall back to stored email
    if user_contact.is_some() {
        report.user.anonymous = false;
        report.user.contact = user_contact;
    } else {
        let config = crate::telemetry::TelemetryConfig::load();
        if let Some(ref email) = config.contact_email {
            report.user.anonymous = false;
            report.user.contact = Some(email.clone());
        }
    }

    // Add reproduction info (respecting consent level)
    if consent_level != "error_only" {
        report.reproduction = Some(ReportReproduction {
            minimal_code: args
                .get("minimal_repro")
                .and_then(|v| v.as_str())
                .map(String::from),
            expected_behavior: args
                .get("expected_behavior")
                .and_then(|v| v.as_str())
                .map(String::from),
            actual_behavior: args
                .get("actual_behavior")
                .and_then(|v| v.as_str())
                .map(String::from),
            spec_reference: args
                .get("spec_reference")
                .and_then(|v| v.as_str())
                .map(String::from),
        });
    }

    // Add AI context (only at "full" consent)
    if consent_level == "full" {
        report.ai_context = Some(ReportAiContext {
            analysis: args
                .get("ai_analysis")
                .and_then(|v| v.as_str())
                .map(String::from),
            suggested_component: args
                .get("suggested_component_file")
                .and_then(|v| v.as_str())
                .map(String::from),
            suggested_fix: None,
            confidence: None,
        });
    }

    // Store locally for fix notification tracking
    let mut store = ReportStore::load();
    store.add_report(&report);
    let _ = store.save();

    // Attempt to submit to backend (falls back to queue)
    let result = crate::telemetry::submit_report(&report);

    match result {
        crate::telemetry::SubmitResult::AlreadyFixed {
            report_id,
            fixed_in_version,
            fix_description,
            message,
        } => {
            // Update local store to mark as resolved
            store.update_status(
                &report_id,
                crate::telemetry::report::ReportStatus::Resolved,
                Some(fixed_in_version.clone()),
                fix_description.clone(),
                None,
            );
            let _ = store.save();

            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "already_fixed": true,
                    "report_id": report_id,
                    "fixed_in_version": fixed_in_version,
                    "fix_description": fix_description,
                    "upgrade_command": "cleen install latest",
                    "message": message,
                    "action": "This bug is already fixed. Do NOT write workaround code. Update the compiler with: cleen install latest"
                }),
            )
        }
        crate::telemetry::SubmitResult::Known {
            report_id,
            occurrences,
            current_status,
            message,
        } => JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "known_issue": true,
                "report_id": report_id,
                "occurrences": occurrences,
                "current_status": current_status,
                "message": message,
                "action": "This is a known issue being worked on. Write spec-correct code and note the limitation. Do NOT write workarounds."
            }),
        ),
        crate::telemetry::SubmitResult::Submitted {
            report_id,
            tracking_url,
        } => JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "report_id": report_id,
                "tracking_url": tracking_url,
                "message": "Error report submitted successfully. Thank you for helping improve Clean Language!"
            }),
        ),
        crate::telemetry::SubmitResult::Queued {
            report_id,
            local_path,
        } => JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "queued": true,
                "report_id": report_id,
                "local_path": local_path,
                "message": "Report saved locally. It will be sent when connectivity to the error reporting service is available."
            }),
        ),
        crate::telemetry::SubmitResult::Error { message } => JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": message,
                "message": "Failed to save error report. The error has been noted locally."
            }),
        ),
    }
}

/// Tool: check_reported_fixes - Check for updates on previously reported errors
fn tool_check_reported_fixes(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    use crate::telemetry::{report::ReportStatus, ReportStore};

    let include_all = args
        .get("include_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut store = ReportStore::load();
    let all_reports = store.get_all_reports();

    if all_reports.is_empty() {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "fixes": [],
                "pending": [],
                "current_version": crate::VERSION,
                "has_updates": false,
                "message": "No error reports tracked on this machine."
            }),
        );
    }

    // Collect report IDs for backend status check
    let report_ids: Vec<String> = all_reports
        .iter()
        .filter(|r| r.status != ReportStatus::Resolved || include_all)
        .map(|r| r.report_id.clone())
        .collect();

    // Try to fetch updates from backend (returns empty vec if offline)
    let updates = crate::telemetry::submit::check_report_statuses(&report_ids);

    // Apply any updates from backend
    for update in &updates {
        let status = match update.status.as_str() {
            "acknowledged" => ReportStatus::Acknowledged,
            "in_progress" => ReportStatus::InProgress,
            "resolved" => ReportStatus::Resolved,
            "wont_fix" => ReportStatus::WontFix,
            _ => continue,
        };

        store.update_status(
            &update.report_id,
            status,
            update.fixed_in_version.clone(),
            update.fix_description.clone(),
            update.fix_pr.clone(),
        );
    }

    // Save any status updates
    if !updates.is_empty() {
        let _ = store.save();
    }

    // Build response — split into fixes (resolved) and pending (everything else)
    let all_reports = store.get_all_reports();

    let fixes: Vec<serde_json::Value> = all_reports
        .iter()
        .filter(|r| r.status == ReportStatus::Resolved)
        .filter(|r| include_all || !r.notified)
        .map(|r| {
            json!({
                "report_id": r.report_id,
                "error_code": r.error_code,
                "summary": r.summary,
                "status": "resolved",
                "fixed_in_version": r.resolved_in,
                "fix_description": r.fix_description,
                "fix_pr": r.fix_pr,
                "update_command": r.resolved_in.as_ref().map(|v| format!("cleen install {}", v))
            })
        })
        .collect();

    let pending: Vec<serde_json::Value> = all_reports
        .iter()
        .filter(|r| r.status != ReportStatus::Resolved)
        .map(|r| {
            json!({
                "report_id": r.report_id,
                "error_code": r.error_code,
                "summary": r.summary,
                "status": r.status.to_string(),
                "reported_at": r.reported_at.to_rfc3339()
            })
        })
        .collect();

    let has_updates = !fixes.is_empty() || !updates.is_empty();

    // Collect IDs of resolved reports to mark as notified
    let to_notify: Vec<String> = all_reports
        .iter()
        .filter(|r| r.status == ReportStatus::Resolved && !r.notified)
        .map(|r| r.report_id.clone())
        .collect();

    // Mark new fixes as notified (separate borrow scope)
    for report_id in &to_notify {
        store.mark_notified(report_id);
    }
    let _ = store.save();

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "fixes": fixes,
            "pending": pending,
            "current_version": crate::VERSION,
            "has_updates": has_updates
        }),
    )
}

/// Generate a UUID v4-format report ID
fn generate_report_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let a: u32 = rng.gen();
    let b: u16 = rng.gen();
    let c: u16 = (rng.gen::<u16>() & 0x0FFF) | 0x4000;
    let d: u16 = (rng.gen::<u16>() & 0x3FFF) | 0x8000;
    let e: u64 = rng.gen::<u64>() & 0xFFFF_FFFF_FFFF;
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", a, b, c, d, e)
}

/// Convert a CompilerError to JSON for MCP responses
fn error_to_json(error: &crate::error::CompilerError) -> serde_json::Value {
    use crate::error::CompilerError;

    match error {
        CompilerError::Syntax { context } => json!({
            "severity": "error",
            "code": context.error_code,
            "message": context.message,
            "file": context.location.as_ref().map(|l| l.file.as_str()).unwrap_or(""),
            "line": context.location.as_ref().map(|l| l.line).unwrap_or(0),
            "column": context.location.as_ref().map(|l| l.column).unwrap_or(0),
        }),
        CompilerError::Type { context } => json!({
            "severity": "error",
            "code": context.error_code,
            "message": context.message,
            "file": context.location.as_ref().map(|l| l.file.as_str()).unwrap_or(""),
            "line": context.location.as_ref().map(|l| l.line).unwrap_or(0),
            "column": context.location.as_ref().map(|l| l.column).unwrap_or(0),
        }),
        _ => json!({
            "severity": "error",
            "message": error.to_string(),
        }),
    }
}

/// Simple base64 encoding
fn base64_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose, Engine as _};
    general_purpose::STANDARD.encode(bytes)
}

/// Get error explanation (reused from main.rs)
struct ErrorExplanation {
    title: &'static str,
    description: &'static str,
    example: &'static str,
    fix: &'static str,
}

fn get_error_explanation(code: &str) -> ErrorExplanation {
    let catalog = get_error_catalog();

    for entry in catalog.iter() {
        if entry.code == code {
            return ErrorExplanation {
                title: entry.title,
                description: entry.description,
                example: entry.example,
                fix: entry.fix,
            };
        }
    }

    // Default for unknown codes
    ErrorExplanation {
        title: "Unknown Error Code",
        description: "This error code is not recognized.",
        example: "",
        fix: "Please check the error code or use 'list_error_codes' to see all available codes.",
    }
}
