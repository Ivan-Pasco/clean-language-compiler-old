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

            if let Some(heading) = line.strip_prefix("## ") {
                // Save previous section if exists
                if !current_title.is_empty() {
                    sections.push(SpecSection {
                        title: current_title.clone(),
                        content: current_content.clone(),
                        line_start: current_line_start,
                    });
                }

                // Start new section
                current_title = heading.trim().to_string();
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

    // Background: retry locally-stored reports that were never confirmed by the
    // backend (fingerprint still null) and flush any offline queue.
    std::thread::spawn(|| {
        crate::telemetry::flush_pending_telemetry(false);
    });

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
    // Write header + body as a single buffer to avoid partial flush delays
    // Content-Length counts only the JSON body; trailing \n is a transport separator
    let message = format!("Content-Length: {}\r\n\r\n{}\n", json.len(), json);
    stdout.write_all(message.as_bytes()).await?;
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
    const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-03-26", "2024-11-05"];

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
        "instructions": "You are working with the Clean Language compiler MCP server. Clean Language is a type-safe language that compiles to WebAssembly.\n\n## GETTING STARTED (call these tools in order)\n1. `get_quick_reference` — Learn syntax, types, patterns (CALL FIRST)\n2. `get_stack_recommendation` — Get recommended plugins for your project type\n3. `list_plugins` — See installed plugins with full DSL syntax\n4. `get_plugin_examples` — Read real example files from plugins\n\n## BEST PRACTICES FOR AI ASSISTANTS\n- ALWAYS call `get_quick_reference` at the start of a session before writing any Clean Language code\n- ALWAYS call `list_plugins` before using framework features (data:, endpoints:, component:, etc.)\n- Use `check` for fast type-checking during development (no WASM generation)\n- Use `get_specification` to look up detailed language rules\n- Use `get_architecture` to understand the execution model and host bridge\n- Clean Language applications use Clean for ALL layers — server, database, UI, graphics. NEVER use JavaScript when a Clean plugin exists.\n- Use `report_error` when you encounter what appears to be a compiler bug\n- If the user reports a server runtime failure or WASM load error, call `list_server_diagnostics` BEFORE asking for reproduction details — the clean-server auto-captures structured diagnostics\n- Use `list_server_diagnostics` to surface pending compiler bugs the user may not realize are on disk\n\n## REPORT_ERROR — CRITICAL RULES\n- **SYNTHESIZE BEFORE REPORTING.** Before calling `report_error`, write a minimal reproduction and root-cause analysis. Do not file a bare error code with a plan to add details later — the team cannot fix a bug without a repro.\n- `minimal_repro`: write the smallest possible `start:` block (or function) that triggers the same compiler failure. Write it from scratch — do NOT copy from the user's file.\n- `ai_analysis`: explain which pipeline stage fails and why. This contains no user code and is always sent.\n- `suggested_fix`: if you can identify the file and line, include it (e.g. `src/codegen/mod.rs line 847: condition is inverted`).\n- The `component` field MUST identify WHERE the bug lives (the component containing the buggy code), NOT the name of the task you were running (e.g. never use 'validation', 'testing', 'docs'). Invalid component names are rejected with an error.\n- Valid components: compiler, server, node-server, framework, extension, manager, website, canvas, ui, mcp, unknown. If unsure, use 'unknown'.\n- Use `discovered_during` (optional) to record the context in which you found the bug (e.g. 'solving E0042', 'doc_coverage run') — this does not affect routing.\n- Before calling `report_error` during doc/spec validation or example checking: verify the error also reproduces when compiling the file normally with the correct plugin loaded. If the error disappears in that context, it is a false positive from missing plugin context — do NOT report it.\n- Errors discovered while solving another bug are independent reports. Report them under their correct component. Do NOT attach them to the original bug.\n\n## KEY RULES\n- File extension: .cln\n- Indentation: tabs only (not spaces)\n- Entry point: `start:` block\n- Types: integer, number, string, boolean, list<T>, matrix<T>, pairs\n- Functions declared in `functions:` block with return type first\n- No semicolons, no curly braces\n- `return value` (no parentheses)\n- One way to do things — follow the spec exactly\n\n## REPORTING RUNTIME_WASM_PARSE BUGS\n1. `list_server_diagnostics` → identify pending reports\n2. `show_server_diagnostic(sha)` → load the full payload\n3. `report_error` with the payload's fields mapped into the standard error-report schema\n4. Tell the user to run `clean-server errors publish <sha>` to mark it as forwarded"
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
            description: "Get diagnostics (errors/warnings) for a source file with optional severity filtering. Returns counts and details.".to_string(),
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
                    },
                    "severity": {
                        "type": "string",
                        "description": "Filter by severity: 'error' or 'warning'. Returns all if omitted."
                    }
                }),
                required: vec!["source".to_string()],
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
            description: "List all available plugins with their AI context. Returns plugin names, descriptions, block types, bridge functions, and AI-specific metadata (description, examples, constraints, patterns). The 'patterns' field lists named architectural recipes — call get_plugin_examples with a pattern name to get the full example and anti-pattern guidance.".to_string(),
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
            description: "Read example source files from an installed plugin. When called with a 'pattern' name (e.g. 'multilingual-site'), returns the full architectural example with use-when guidance and anti-pattern warning. Without 'pattern', returns all generic example files declared in the plugin's [ai] section.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "plugin_name": {
                        "type": "string",
                        "description": "Plugin name (e.g., 'frame.data', 'frame.server')"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Optional pattern name to load a specific architectural recipe (e.g. 'multilingual-site', 'soft-delete'). Use list_plugins to see available pattern names."
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
            description: "Report a compiler or runtime error to the Clean Language team. BEFORE calling this tool, synthesize a minimal reproduction and root-cause analysis — do not report and plan to enrich later. A report without minimal_repro and ai_analysis is only useful as an occurrence signal; a complete report is what lets the team actually fix the bug. The report is anonymous and never includes the user's actual source code — minimal_repro must be AI-generated code that isolates the bug, not copied from the user's file.".to_string(),
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
                        "description": "Which project component produced the error.",
                        "enum": ["compiler", "server", "node-server", "framework", "extension", "manager", "website", "canvas", "ui", "mcp", "unknown"]
                    },
                    "subsystem": {
                        "type": "string",
                        "description": "Optional: which subsystem within the component (e.g., parser, semantic, codegen, runtime, bridge, plugin, lsp, cli)."
                    },
                    "severity": {
                        "type": "string",
                        "description": "Severity classification of the error.",
                        "enum": ["bug", "crash", "regression", "unexpected_behavior"]
                    },
                    "minimal_repro": {
                        "type": "string",
                        "description": "SYNTHESIZE THIS BEFORE CALLING. Minimal Clean Language code that reproduces the error — written by the AI, not copied from the user. Start from scratch: the smallest start: block that triggers the same compiler failure. Omit this only if you genuinely cannot isolate the trigger."
                    },
                    "expected_behavior": {
                        "type": "string",
                        "description": "What the correct behavior should be per the Language Specification (e.g. 'should compile to WASM', 'should infer type integer')."
                    },
                    "actual_behavior": {
                        "type": "string",
                        "description": "What actually happens — paste the exact error message or crash output."
                    },
                    "spec_reference": {
                        "type": "string",
                        "description": "Relevant section of foundation/spec/grammar.ebnf or semantic-rules.md, if applicable."
                    },
                    "ai_analysis": {
                        "type": "string",
                        "description": "REQUIRED unless the bug is a pure crash with no diagnosis available. Root-cause analysis: which pipeline stage fails, why, and what the likely fix is. Does not contain user code — no privacy reason to omit it."
                    },
                    "suggested_fix": {
                        "type": "string",
                        "description": "Specific fix location and change (e.g. 'src/codegen/mod.rs line 847: string comparison is inverted, change == to !='). Include when you can identify the exact line."
                    },
                    "suggested_component_file": {
                        "type": "string",
                        "description": "Source file most likely to contain the fix (e.g. 'src/parser/token_parser.rs'). Use when you know the file but not the exact line."
                    },
                    "consent_level": {
                        "type": "string",
                        "description": "What level of detail the user consented to share. Default: error_with_code.",
                        "enum": ["error_only", "error_with_code", "full"]
                    },
                    "user_contact": {
                        "type": "string",
                        "description": "Optional contact info if the user wants follow-up. Only include if explicitly provided by the user."
                    },
                    "discovered_during": {
                        "type": "string",
                        "description": "Optional: the task or context in which this error was found (e.g., 'solving E0042', 'doc_coverage run', 'canvas example validation'). Records lineage without affecting component routing. Do NOT use this as a substitute for a correct component value."
                    },
                    "affected_component": {
                        "type": "string",
                        "description": "Optional: the specific component where the bug actually lives, when different from the reporting component. For example, a plugin compilation bug found while running the compiler might set component='compiler' but affected_component='framework'.",
                        "enum": ["compiler", "server", "node-server", "framework", "extension", "manager", "website", "canvas", "ui", "mcp", "unknown"]
                    },
                    "affected_version": {
                        "type": "string",
                        "description": "Optional: version of the affected component where the bug was observed (e.g., '2.6.1' for frame.data 2.6.1). Distinct from the compiler version, which is captured automatically."
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
            description: "Check if any previously reported errors have been fixed. Returns resolved errors with fix versions, plus a summary of open bugs for this component from ALL reporters. Call this at the start of every session.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "include_all": {
                        "type": "boolean",
                        "description": "If true, returns all tracked reports regardless of status. If false (default), returns only reports with status changes since last check."
                    },
                    "component": {
                        "type": "string",
                        "description": "Component to fetch open bugs for. Defaults to 'compiler'. Use when working on a different component (e.g. 'server', 'framework', 'extension').",
                        "enum": ["compiler", "server", "node-server", "framework", "extension", "manager", "website", "canvas", "ui", "mcp", "unknown"]
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "list_server_diagnostics".to_string(),
            description: "List RUNTIME_WASM_PARSE diagnostic reports captured by clean-server. These are auto-saved when wasmtime rejects a WASM module. Returns pending reports by default. Call this when a user reports server crashes or WASM load errors.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "diag_dir": {
                        "type": "string",
                        "description": "Path to diagnostics directory. Default: './diagnostics'"
                    },
                    "status": {
                        "type": "string",
                        "description": "Filter by status: 'pending' (default), 'published', 'resolved', 'all'"
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "publish_diagnostic".to_string(),
            description: "Publish a local server diagnostic to the Clean Language error server. Reads the diagnostic from ./diagnostics/pending/<sha>, converts it to a standard error report, and submits it. IMPORTANT: Always ask the user for permission before calling this tool. The response indicates if the bug was already fixed, already known, or newly accepted. On success, moves the diagnostic to published/.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "sha": {
                        "type": "string",
                        "description": "SHA-256 hash of the WASM module (prefix >= 4 chars accepted)"
                    },
                    "diag_dir": {
                        "type": "string",
                        "description": "Path to diagnostics directory. Default: './diagnostics'"
                    },
                    "user_contact": {
                        "type": "string",
                        "description": "Optional contact info if the user wants follow-up on this bug."
                    }
                }),
                required: vec!["sha".to_string()],
            },
        },
        Tool {
            name: "list_component_bugs".to_string(),
            description: "List all open bugs for a specific component from the error server. Unlike check_reported_fixes (which only shows bugs reported from this machine), this tool shows ALL bugs reported by ALL users and AI instances. Use this at session start when working on a component to discover bugs that need fixing. Requires network access to the error server.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "component": {
                        "type": "string",
                        "description": "Component to query bugs for.",
                        "enum": ["compiler", "server", "node-server", "framework", "extension", "manager", "website", "canvas", "ui", "mcp", "all"]
                    },
                    "status": {
                        "type": "string",
                        "description": "Filter by bug status. Default: 'reported' (open bugs).",
                        "enum": ["reported", "acknowledged", "in_progress", "resolved", "all"]
                    }
                }),
                required: vec!["component".to_string()],
            },
        },
        Tool {
            name: "show_server_diagnostic".to_string(),
            description: "Show the full RUNTIME_WASM_PARSE diagnostic report for a given SHA. Returns the complete JSON payload ready to pass to report_error. Accepts SHA prefix (minimum 4 chars).".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "sha": {
                        "type": "string",
                        "description": "SHA-256 hash of the broken WASM module (prefix >= 4 chars accepted)"
                    },
                    "diag_dir": {
                        "type": "string",
                        "description": "Path to diagnostics directory. Default: './diagnostics'"
                    }
                }),
                required: vec!["sha".to_string()],
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
        // ====================================================================
        // New Tools: format, validate, run, list_examples, get_changelog
        // ====================================================================
        Tool {
            name: "format".to_string(),
            description: "Format Clean Language source code. Normalizes indentation (tabs), whitespace, and block structure.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "The Clean Language source code to format"
                    }
                }),
                required: vec!["source".to_string()],
            },
        },
        Tool {
            name: "validate".to_string(),
            description: "Validate a plugin.toml or package.clean.toml file. Checks required fields, structure, and bridge function declarations.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "content": {
                        "type": "string",
                        "description": "The TOML file content to validate"
                    },
                    "file_type": {
                        "type": "string",
                        "description": "File type: 'plugin' (plugin.toml) or 'package' (package.clean.toml). Default: auto-detect."
                    }
                }),
                required: vec!["content".to_string()],
            },
        },
        Tool {
            name: "run".to_string(),
            description: "Compile and validate Clean Language source in one step. Returns compilation result with WASM size and any diagnostics. Does not execute (execution requires a runtime).".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "The Clean Language source code to compile and validate"
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Path to the source file (used for error reporting)"
                    }
                }),
                required: vec!["source".to_string()],
            },
        },
        Tool {
            name: "list_examples".to_string(),
            description: "List example .cln files from the compiler's examples directory. Returns file names and contents for learning Clean Language patterns.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "name": {
                        "type": "string",
                        "description": "Optional: specific example file name to read (e.g., 'hello.cln'). Returns all examples if omitted."
                    }
                }),
                required: vec![],
            },
        },
        Tool {
            name: "get_changelog".to_string(),
            description: "Get recent changes to the Clean Language compiler. Returns the last N version changes from git history.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "count": {
                        "type": "number",
                        "description": "Number of recent versions to return. Default: 10."
                    }
                }),
                required: vec![],
            },
        },
        // ====================================================================
        // Doc-Code Synchronization Tools
        // ====================================================================
        Tool {
            name: "validate_docs".to_string(),
            description: "Validate feature spec doc: references against compiler-visible symbols in a .cln file or directory. Detects broken references (symbol not found) and stale signatures (symbol changed since the spec was last updated). Optionally writes updated signatures back to spec files.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "Path to a .cln file or directory containing .cln files"
                    },
                    "docs_dir": {
                        "type": "string",
                        "description": "Path to docs/features directory. Defaults to docs/features relative to the source directory."
                    },
                    "update_signatures": {
                        "type": "boolean",
                        "description": "If true, write updated signatures back to spec files. Default: false."
                    }
                }),
                required: vec!["source".to_string()],
            },
        },
        Tool {
            name: "doc_coverage".to_string(),
            description: "Compute documentation coverage: which compiler-visible symbols (functions, classes, state vars) are referenced by at least one feature spec, and which are undocumented. Returns a percentage and lists of covered/uncovered symbols.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "source": {
                        "type": "string",
                        "description": "Path to a .cln file or directory containing .cln files"
                    },
                    "docs_dir": {
                        "type": "string",
                        "description": "Path to docs/features directory. Defaults to docs/features relative to the source directory."
                    }
                }),
                required: vec!["source".to_string()],
            },
        },
        Tool {
            name: "get_feature_spec".to_string(),
            description: "Retrieve the full content of a feature spec document. Look up by doc: ref string (e.g., 'functions/transfer') or by feature name substring. Returns the raw Markdown including frontmatter.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "ref": {
                        "type": "string",
                        "description": "A doc: ref string (e.g., 'functions/transfer') or a feature name substring to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Direct file path to a feature spec Markdown file"
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
    let response = match tool_name {
        "check" => tool_check(id.clone(), arguments),
        "compile" => tool_compile(id.clone(), arguments),
        "parse" => tool_parse(id.clone(), arguments),
        "diagnostics" => tool_diagnostics(id.clone(), arguments),
        "explain_error" => tool_explain_error(id.clone(), arguments),
        "list_functions" => tool_list_functions(id.clone(), arguments),
        "list_types" => tool_list_types(id.clone(), arguments),
        "list_plugins" => tool_list_plugins(id.clone(), arguments),
        "get_specification" => tool_get_specification(id.clone(), arguments),
        "list_builtins" => tool_list_builtins(id.clone(), arguments),
        "list_error_codes" => tool_list_error_codes(id.clone(), arguments),
        "get_quick_reference" => tool_get_quick_reference(id.clone()),
        "get_plugin_examples" => tool_get_plugin_examples(id.clone(), arguments),
        "list_ecosystem" => tool_list_ecosystem(id.clone(), arguments),
        "get_stack_recommendation" => tool_get_stack_recommendation(id.clone(), arguments),
        "report_error" => tool_report_error(id.clone(), arguments),
        "check_reported_fixes" => tool_check_reported_fixes(id.clone(), arguments),
        "list_server_diagnostics" => tool_list_server_diagnostics(id.clone(), arguments),
        "show_server_diagnostic" => tool_show_server_diagnostic(id.clone(), arguments),
        "publish_diagnostic" => tool_publish_diagnostic(id.clone(), arguments),
        "list_component_bugs" => tool_list_component_bugs(id.clone(), arguments),
        "get_architecture" => tool_get_architecture(id.clone(), arguments),
        "format" => tool_format(id.clone(), arguments),
        "validate" => tool_validate(id.clone(), arguments),
        "run" => tool_run(id.clone(), arguments),
        "list_examples" => tool_list_examples(id.clone(), arguments),
        "get_changelog" => tool_get_changelog(id.clone(), arguments),
        "validate_docs" => tool_validate_docs(id.clone(), arguments),
        "doc_coverage" => tool_doc_coverage(id.clone(), arguments),
        "get_feature_spec" => tool_get_feature_spec(id.clone(), arguments),
        _ => {
            return JsonRpcResponse::error(
                id,
                error_codes::METHOD_NOT_FOUND,
                format!("Unknown tool: {}", tool_name),
            )
        }
    };

    // Wrap tool result in MCP content format: {content: [{type: "text", text: "..."}]}
    // Per MCP spec, tools/call results must use this format
    if let Some(result) = response.result {
        let text = serde_json::to_string(&result).unwrap_or_default();
        let is_error = result
            .get("success")
            .and_then(|v| v.as_bool())
            .map(|s| !s)
            .unwrap_or(false);
        JsonRpcResponse::success(
            response.id,
            json!({
                "content": [{"type": "text", "text": text}],
                "isError": is_error
            }),
        )
    } else {
        response
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
            let diagnostics: Vec<serde_json::Value> =
                result.diagnostics.iter().map(error_to_json).collect();
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
            let diagnostics: Vec<serde_json::Value> = errors.iter().map(error_to_json).collect();
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
            let diagnostics: Vec<serde_json::Value> = errors.iter().map(error_to_json).collect();
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
            let diagnostics: Vec<serde_json::Value> = errors.iter().map(error_to_json).collect();
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
        .unwrap_or("input.cln");

    // Type-check and return only diagnostics with severity filtering
    let severity_filter = args.get("severity").and_then(|v| v.as_str());

    match type_check(source, file_path) {
        Ok(_result) => JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "diagnostics": [],
                "error_count": 0,
                "warning_count": 0
            }),
        ),
        Err(errors) => {
            let all_diagnostics: Vec<serde_json::Value> =
                errors.iter().map(error_to_json).collect();

            let diagnostics: Vec<serde_json::Value> = if let Some(filter) = severity_filter {
                all_diagnostics
                    .into_iter()
                    .filter(|d| {
                        d.get("severity")
                            .and_then(|s| s.as_str())
                            .map(|s| s.eq_ignore_ascii_case(filter))
                            .unwrap_or(false)
                    })
                    .collect()
            } else {
                all_diagnostics
            };

            let error_count = diagnostics
                .iter()
                .filter(|d| d.get("severity").and_then(|s| s.as_str()) == Some("error"))
                .count();
            let warning_count = diagnostics.len() - error_count;

            JsonRpcResponse::success(
                id,
                json!({
                    "success": error_count == 0,
                    "diagnostics": diagnostics,
                    "error_count": error_count,
                    "warning_count": warning_count
                }),
            )
        }
    }
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
            let diagnostics: Vec<serde_json::Value> = errors.iter().map(error_to_json).collect();
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
            let diagnostics: Vec<serde_json::Value> = errors.iter().map(error_to_json).collect();
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
                        || !ai.patterns.is_empty()
                    {
                        let patterns_index: Vec<serde_json::Value> = ai
                            .patterns
                            .iter()
                            .map(|p| {
                                json!({
                                    "name": p.name,
                                    "use_when": p.use_when
                                })
                            })
                            .collect();
                        plugin_json["ai"] = json!({
                            "description": ai.description,
                            "examples": ai.examples,
                            "constraints": ai.constraints,
                            "patterns": patterns_index
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
                "success": false,
                "plugins": [],
                "error": format!("Plugin discovery failed: {}", e)
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
    let pattern_name = args.get("pattern").and_then(|v| v.as_str());
    let project_dir = args.get("project_dir").and_then(|v| v.as_str());

    let mut discovery = PluginDiscovery::new();
    if let Some(dir) = project_dir {
        discovery = discovery.with_project_dir(dir);
    }

    match discovery.load_plugin(plugin_name) {
        Ok(manifest) => {
            let plugin_dir = find_plugin_dir(plugin_name, project_dir);

            // Pattern-specific request: return one named architectural recipe
            if let Some(pname) = pattern_name {
                let found = manifest.ai.patterns.iter().find(|p| p.name == pname);
                return match found {
                    Some(pattern) => {
                        let full_path = if let Some(ref pdir) = plugin_dir {
                            pdir.join(&pattern.example)
                        } else {
                            std::path::PathBuf::from(&pattern.example)
                        };
                        let content = std::fs::read_to_string(&full_path)
                            .unwrap_or_else(|e| format!("// Could not read example file: {}", e));
                        JsonRpcResponse::success(
                            id,
                            json!({
                                "success": true,
                                "plugin": plugin_name,
                                "pattern": {
                                    "name": pattern.name,
                                    "use_when": pattern.use_when,
                                    "anti_pattern": pattern.anti_pattern,
                                    "example": content
                                }
                            }),
                        )
                    }
                    None => {
                        let available: Vec<&str> = manifest
                            .ai
                            .patterns
                            .iter()
                            .map(|p| p.name.as_str())
                            .collect();
                        JsonRpcResponse::success(
                            id,
                            json!({
                                "success": false,
                                "error": format!("Pattern '{}' not found in plugin '{}'", pname, plugin_name),
                                "available_patterns": available
                            }),
                        )
                    }
                };
            }

            // Generic request: return all example files declared in [ai]
            let example_paths = &manifest.ai.examples;

            if example_paths.is_empty() && manifest.ai.patterns.is_empty() {
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

            let patterns_index: Vec<serde_json::Value> = manifest
                .ai
                .patterns
                .iter()
                .map(|p| json!({"name": p.name, "use_when": p.use_when}))
                .collect();

            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "plugin": plugin_name,
                    "examples": examples,
                    "patterns": patterns_index,
                    "tip": "Call get_plugin_examples with a 'pattern' name to get the full architectural recipe and anti-pattern guidance.",
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

    // Mark catalog entries as installed/not
    let catalog_names: Vec<String> = plugins
        .iter()
        .filter_map(|p| {
            p.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    for plugin in plugins.iter_mut() {
        let name = plugin["name"].as_str().unwrap_or("");
        plugin["installed"] = json!(installed_names.iter().any(|n| n == name));
    }

    // Auto-discover plugins installed but not in the hardcoded catalog
    for (name, manifest) in &installed {
        if !catalog_names.iter().any(|n| n == name) {
            let should_include = if let Some(cat) = category_filter {
                cat.eq_ignore_ascii_case("all")
            } else {
                true
            };
            if should_include {
                let bridge_count = manifest.bridge.functions.len();
                let blocks: Vec<&str> =
                    manifest.handles.blocks.iter().map(|s| s.as_str()).collect();
                plugins.push(json!({
                    "name": name,
                    "version": manifest.plugin.version,
                    "category": "discovered",
                    "description": manifest.plugin.description,
                    "blocks": blocks,
                    "key_features": [],
                    "bridge_function_count": bridge_count,
                    "install": format!("cleen plugin add {}", name),
                    "status": "installed",
                    "installed": true,
                    "auto_detect_paths": [],
                    "replaces": "",
                    "permissions": manifest.bridge.functions.iter()
                        .map(|f| f.name.as_str())
                        .collect::<Vec<_>>(),
                    "source": "auto-discovered"
                }));
            }
        }
    }

    let total = plugins.len();

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "ecosystem": plugins,
            "total": total,
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
            name: "frame.server",
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
            install: "cleen plugin add frame.server",
            status: "stable",
            auto_detect_paths: &["/api/", "/backend/api/", "/server/api/", "/endpoints/"],
            replaces: "Express.js, Fastify, Koa, or any Node.js HTTP framework. Use frame.server for all server-side HTTP handling.",
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
| HTTP Server      | frame.server | Routing, request/response, middleware, auth      | Express.js, Fastify, Koa          |
| Database / ORM   | frame.data       | Queries, CRUD, migrations, transactions          | Sequelize, Prisma, Knex           |
| Frontend UI      | frame.ui         | DOM manipulation, events, hydration, components  | jQuery, vanilla JS, React         |
| Graphics / Games | frame.canvas     | Canvas rendering, animation, sprites, input      | Phaser, PixiJS, JS Canvas API     |
| Authentication   | frame.auth       | Sessions, JWT, roles, CSRF, password hashing     | Passport.js, Auth0 SDK            |

### When to use JavaScript
- NEVER for DOM manipulation (use frame.ui)
- NEVER for event handling (use frame.ui directives: onclick, oninput, onsubmit)
- NEVER for form validation (use frame.ui)
- NEVER for canvas/graphics (use frame.canvas)
- NEVER for HTTP servers (use frame.server)
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

## AI Metadata (foundation/spec/intent)
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
- `check_reported_fixes` — Check if your reported bugs are fixed (also shows open bugs from all reporters)
- `publish_diagnostic` — Publish a local server diagnostic to the error server (ask user first!)
- `list_component_bugs` — List ALL open bugs for a component from all reporters (dev team tool)
- `list_server_diagnostics` — List local runtime errors captured by clean-server

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
13. Call `list_server_diagnostics` to check for runtime errors — if unpublished diagnostics exist, ask the user to publish them
14. If user agrees, call `publish_diagnostic` with the SHA to send the error to the Clean Language team

## Error Lifecycle (for Clean Language development team)
When fixing bugs from the error server:
1. Call `list_component_bugs` to see all open bugs for your component
2. Pick the highest-priority bug and write a failing test
3. Fix the root cause in the source code
4. Run `comita` to commit, tag, push, and wait for CI
5. After CI passes, run `/resolve-fix <ERROR_CODE> <VERSION> "<description>"` to close the bug

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
            "tools_available": get_available_tools().len(),
            "tip": "Use 'check' for fast iteration, 'compile' when ready for WASM. If spec-correct code fails, call 'report_error' immediately — never write workarounds. Call 'check_reported_fixes' at session start. Call 'list_server_diagnostics' to find local runtime errors — ask the user to publish them."
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
- HTTP routing → Layer 3 / frame.server plugin
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
            vec!["frame.server", "frame.data", "frame.ui", "frame.auth"],
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
                "Node.js/Express/Fastify — use frame.server",
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
            vec!["frame.server", "frame.data", "frame.auth"],
            json!({
                "app/server/main.cln": "Server entry point with API routes",
                "app/data/models.cln": "Data models and database schema",
                "app/auth/auth.cln": "Authentication configuration and guards",
                "app/server/middleware.cln": "Custom middleware"
            }),
            vec![
                "Node.js/Express/Fastify — use frame.server",
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

    // SEC001: injection detection — reject values that look like script injection
    {
        let injection_patterns = [
            "<script",
            "javascript:",
            "onerror=",
            "onload=",
            "data:text/html",
        ];
        let combined = format!("{error_code} {error_message}").to_lowercase();
        if injection_patterns.iter().any(|p| combined.contains(p)) {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Invalid input: error_code or error_message contains disallowed content"
                    .to_string(),
            );
        }
    }
    // SEC001: cap error_message at 2000 chars to prevent payload bloat
    let error_message = if error_message.len() > 2000 {
        format!("{}…", &error_message[..1997])
    } else {
        error_message
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
    let explicit_subsystem = args
        .get("subsystem")
        .and_then(|v| v.as_str())
        .map(String::from);
    let discovered_during = args
        .get("discovered_during")
        .and_then(|v| v.as_str())
        .map(String::from);

    // MCP001: extract new cross-component context fields
    let raw_affected_component = args
        .get("affected_component")
        .and_then(|v| v.as_str())
        .map(String::from);
    let affected_version = args
        .get("affected_version")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Determine error category from code prefix.
    // Covers both symbolic prefixes (SYN, SEM, COD, RUN, SYS) and the E-series
    // codes used by compiler constructors (E001–E013, MEM*, VAL*, MOD*, PLG*).
    let category = if error_code.starts_with("SYN") || error_code == "E001" {
        "syntax"
    } else if error_code.starts_with("SEM") || error_code == "E002" {
        "semantic"
    } else if error_code.starts_with("COD") || error_code.starts_with("COM") || error_code == "E007"
    {
        "codegen"
    } else if error_code.starts_with("RUN") || error_code == "E009" {
        "runtime"
    } else if error_code.starts_with("MEM") || error_code == "E006" {
        "memory"
    } else if error_code.starts_with("VAL") || error_code == "E010" {
        "validation"
    } else if error_code.starts_with("MOD") || error_code == "E013" {
        "module"
    } else if error_code.starts_with("PLG") {
        "plugin"
    } else if error_code.starts_with("LEX") || error_code == "LEX000" {
        "syntax"
    } else if error_code.starts_with("SYS") {
        "system"
    } else {
        "unknown"
    };

    // Auto-detect subsystem from error code prefix (if not explicitly provided)
    let subsystem = explicit_subsystem.or_else(|| {
        let auto = if error_code.starts_with("SYN") {
            "parser"
        } else if error_code.starts_with("SEM") {
            "semantic"
        } else if error_code.starts_with("COD") || error_code.starts_with("COM") {
            "codegen"
        } else if error_code.starts_with("RUN") {
            "runtime"
        } else if error_code.starts_with("SYS") {
            "system"
        } else {
            return None;
        };
        Some(auto.to_string())
    });

    // Normalize legacy component names to unified taxonomy, then validate.
    // The wildcard must never pass through arbitrary strings — that's how "validation"
    // ended up as a component name when an AI used the task name instead of the buggy component.
    const VALID_COMPONENTS: &[&str] = &[
        "compiler",
        "server",
        "node-server",
        "framework",
        "extension",
        "manager",
        "website",
        "canvas",
        "ui",
        "mcp",
        "unknown",
    ];
    let component = match component.as_str() {
        "parser" | "semantic" | "codegen" | "cli" | "syntax" | "system" => "compiler".to_string(),
        "runtime" => "server".to_string(),
        "plugin" => "framework".to_string(),
        c if c.starts_with("frame.") => "framework".to_string(),
        c if VALID_COMPONENTS.contains(&c) => c.to_string(),
        _ => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                format!(
                    "Invalid component '{}'. Must be one of: {}. \
                     Use the component that CONTAINS the buggy code — not the name \
                     of the task or tool you were running when you found the bug. \
                     If unsure, use 'unknown'.",
                    component,
                    VALID_COMPONENTS.join(", ")
                ),
            );
        }
    };

    // MCP001: normalize affected_component using the same taxonomy as component
    let affected_component = raw_affected_component.map(|c| match c.as_str() {
        "parser" | "semantic" | "codegen" | "cli" | "syntax" | "system" => "compiler".to_string(),
        "runtime" => "server".to_string(),
        "plugin" => "framework".to_string(),
        c if c.starts_with("frame.") => "framework".to_string(),
        c if VALID_COMPONENTS.contains(&c) => c.to_string(),
        _ => "unknown".to_string(),
    });

    // Generate report ID
    let report_id = generate_report_id();

    // Build the report
    let mut report = ErrorReport::new(
        report_id.clone(),
        ReportError {
            code: error_code,
            category: category.to_string(),
            component,
            subsystem,
            severity,
            message: error_message,
            file_context: None,
            affected_component,
            affected_version,
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

    // Add AI context at error_with_code consent and above.
    // ai_analysis and suggested_fix contain no user code — they are AI-generated
    // reasoning about the bug. Gating them on "full" silently discards the most
    // actionable part of a report for all default-consent calls.
    if consent_level != "error_only" {
        // Prepend discovered_during to analysis so the lineage is visible in the dashboard.
        let analysis = {
            let base = args
                .get("ai_analysis")
                .and_then(|v| v.as_str())
                .map(String::from);
            match (&discovered_during, &base) {
                (Some(ctx), Some(a)) => Some(format!("Discovered during: {ctx}\n\n{a}")),
                (Some(ctx), None) => Some(format!("Discovered during: {ctx}")),
                (None, b) => b.clone(),
            }
        };
        let suggested_fix = args
            .get("suggested_fix")
            .and_then(|v| v.as_str())
            .map(String::from);
        let suggested_component = args
            .get("suggested_component_file")
            .and_then(|v| v.as_str())
            .map(String::from);
        // Only attach the struct if there is at least one non-null field
        if analysis.is_some() || suggested_fix.is_some() || suggested_component.is_some() {
            report.ai_context = Some(ReportAiContext {
                analysis,
                suggested_component,
                suggested_fix,
                confidence: None,
            });
        }
    }

    // Dev-mode gate: when the MCP server runs inside the source tree of the
    // component being reported, route the report to the local dev queue
    // instead of uploading. This prevents the dashboard from filling with
    // in-progress work-in-flight from the platform authors themselves.
    let dev_ctx = crate::telemetry::detect_dev_context_for_component(&report.error.component);
    if dev_ctx.is_dev() {
        let entry = crate::telemetry::dev_queue::entry_from(
            &dev_ctx,
            &report.error.code,
            &report.error.component,
            &report.error.message,
            report.error.file_context.as_deref(),
            crate::VERSION,
        );
        let outcome = crate::telemetry::dev_queue::append(entry);

        let payload = serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "Dev-mode: recorded locally (not uploaded).\nReason: {}\nComponent: {}\nError: {} \u{00d7}{} (fingerprint {})",
                    dev_ctx.reason().unwrap_or("dev"),
                    report.error.component,
                    report.error.code,
                    outcome.occurrences,
                    outcome.fingerprint,
                ),
            }],
            "dev_mode": true,
            "dev_reason": dev_ctx.reason().unwrap_or("dev"),
            "fingerprint": outcome.fingerprint,
            "occurrences": outcome.occurrences,
        });
        return JsonRpcResponse::success(id, payload);
    }

    // Store locally for fix notification tracking
    let mut store = ReportStore::load();
    store.add_report(&report);
    let _ = store.save();

    // Attempt to submit to backend (falls back to queue)
    let result = crate::telemetry::submit_report(&report);

    // Persist the server-returned fingerprint so the local report is confirmed.
    if let Some(fp) = result.fingerprint() {
        store.update_fingerprint(&report.report_id, fp);
        let _ = store.save();
    }

    match result {
        crate::telemetry::SubmitResult::AlreadyFixed {
            report_id,
            fingerprint,
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
                    "fingerprint": fingerprint,
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
            fingerprint,
            occurrences,
            current_status,
            message,
        } => JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "known_issue": true,
                "report_id": report_id,
                "fingerprint": fingerprint,
                "occurrences": occurrences,
                "current_status": current_status,
                "message": message,
                "action": "This is a known issue being worked on. Write spec-correct code and note the limitation. Do NOT write workarounds."
            }),
        ),
        crate::telemetry::SubmitResult::Submitted {
            report_id,
            fingerprint,
            tracking_url,
        } => JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "report_id": report_id,
                "fingerprint": fingerprint,
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
        crate::telemetry::SubmitResult::RateLimited {
            report_id,
            local_path,
            retry_after_seconds,
        } => JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "queued": true,
                "rate_limited": true,
                "report_id": report_id,
                "local_path": local_path,
                "retry_after_seconds": retry_after_seconds,
                "message": format!(
                    "Report saved locally. Server rate-limited; retry will occur after {}s.",
                    retry_after_seconds
                )
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

    // Cross-reference resolved fixes against local server diagnostics
    let matching_local_diagnostics = cross_reference_diagnostics(&fixes);

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

    // Also fetch open bugs for this component from the error server
    // so AI instances discover bugs reported by OTHER users/instances
    let component = args
        .get("component")
        .and_then(|v| v.as_str())
        .unwrap_or("compiler");
    let component_bugs = crate::telemetry::submit::fetch_component_bugs(component, "reported");
    let open_bugs: Vec<serde_json::Value> = component_bugs
        .iter()
        .take(10) // limit to top 10 to avoid response bloat
        .map(|b| {
            json!({
                "fingerprint": b.fingerprint,
                "error_code": b.error_code,
                "component": b.component,
                "subsystem": b.subsystem,
                "severity": b.severity,
                "message": b.message,
                "minimal_repro": b.minimal_repro,
                "expected_behavior": b.expected_behavior,
                "actual_behavior": b.actual_behavior,
                "occurrences": b.occurrences,
                "priority_score": b.priority_score,
                "first_reported_version": b.first_reported_version,
                "ai_suggested_fix": b.ai_suggested_fix,
                "report_ids": b.report_ids,
            })
        })
        .collect();

    // Surface local dev-queue summary so AI agents see in-progress dev-time
    // errors at session start — these are bugs the dev needs to fix before
    // shipping, never uploaded to the public dashboard.
    let dev_queue_summary = build_dev_queue_summary();

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "fixes": fixes,
            "pending": pending,
            "current_version": crate::VERSION,
            "has_updates": has_updates,
            "matching_local_diagnostics": matching_local_diagnostics,
            "open_bugs_for_component": open_bugs,
            "open_bugs_total": component_bugs.len(),
            "dev_queue_summary": dev_queue_summary,
            "tip": if !open_bugs.is_empty() {
                "There are open bugs for this component. Use list_component_bugs to see full details and fix guidance."
            } else {
                ""
            }
        }),
    )
}

/// Build a compact summary of the local dev queue for the check_reported_fixes
/// response. Returns `{count, top_entries: [...]}` with the 5 most-recently-
/// seen entries so an AI agent can raise them at session start without
/// drowning in full payloads.
fn build_dev_queue_summary() -> serde_json::Value {
    let mut entries = crate::telemetry::dev_queue::load();
    if entries.is_empty() {
        return json!({ "count": 0, "top_entries": [] });
    }
    // Most recent first
    entries.sort_by(|a, b| b.last_seen_at.cmp(&a.last_seen_at));
    let total = entries.len();
    let top: Vec<serde_json::Value> = entries
        .iter()
        .take(5)
        .map(|e| {
            let mut msg = e.message.replace('\n', " ");
            if msg.len() > 120 {
                msg.truncate(117);
                msg.push_str("...");
            }
            json!({
                "fingerprint": e.fingerprint,
                "error_code": e.error_code,
                "component": e.component,
                "occurrences": e.occurrences,
                "first_seen_at": e.first_seen_at,
                "last_seen_at": e.last_seen_at,
                "message": msg,
                "file_context": e.file_context,
            })
        })
        .collect();
    json!({
        "count": total,
        "top_entries": top,
        "guidance": format!(
            "{} dev-mode error(s) pending local fix. These were captured from failed compiles against component source trees and never uploaded. Run `cln dev-queue list` for full details, `cln dev-queue show <prefix>` for one entry, `cln dev-queue clear <prefix>` after fixing. Resolve these before running `comita`.",
            total
        ),
    })
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

// ============================================================================
// New Tools: format, validate, run, list_examples, get_changelog
// ============================================================================

/// Tool: format - Format Clean Language source code
fn tool_format(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
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

    // Format Clean Language source:
    // - Normalize line endings to \n
    // - Convert spaces-based indentation to tabs
    // - Trim trailing whitespace from each line
    // - Ensure single newline at end of file
    // - Remove consecutive blank lines (max 1)
    let mut formatted_lines: Vec<String> = Vec::new();
    let mut prev_blank = false;

    for line in source.lines() {
        let trimmed_right = line.trim_end();

        if trimmed_right.is_empty() {
            if !prev_blank {
                formatted_lines.push(String::new());
            }
            prev_blank = true;
            continue;
        }
        prev_blank = false;

        // Count leading whitespace and convert to tabs
        let content = trimmed_right.trim_start();
        let leading = &trimmed_right[..trimmed_right.len() - content.len()];

        // Convert spaces to tabs (4 spaces = 1 tab, 2 spaces = 1 tab for partial)
        let tab_count = if leading.contains('\t') {
            leading.matches('\t').count()
        } else {
            // Count spaces and convert: 4 spaces = 1 tab level, round up partial
            let spaces = leading.len();
            if spaces > 0 {
                spaces.div_ceil(4) // Round up to nearest tab
            } else {
                0
            }
        };

        let formatted = format!("{}{}", "\t".repeat(tab_count), content);
        formatted_lines.push(formatted);
    }

    // Remove trailing blank lines
    while formatted_lines
        .last()
        .map(|l| l.is_empty())
        .unwrap_or(false)
    {
        formatted_lines.pop();
    }

    let formatted = formatted_lines.join("\n") + "\n";
    let changed = formatted != source;

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "formatted": formatted,
            "changed": changed,
            "line_count": formatted_lines.len()
        }),
    )
}

/// Tool: validate - Validate plugin.toml or package.clean.toml
fn tool_validate(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'content' parameter".to_string(),
            )
        }
    };

    let file_type = args.get("file_type").and_then(|v| v.as_str()).unwrap_or("");

    // Try to parse as TOML
    let parsed: toml::Value = match content.parse() {
        Ok(v) => v,
        Err(e) => {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "valid": false,
                    "errors": [format!("TOML parse error: {}", e)]
                }),
            )
        }
    };

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Auto-detect file type
    let detected_type = if file_type == "plugin" || parsed.get("plugin").is_some() {
        "plugin"
    } else if file_type == "package" || parsed.get("package").is_some() {
        "package"
    } else {
        errors.push("Cannot detect file type: no [plugin] or [package] section found".to_string());
        "unknown"
    };

    match detected_type {
        "plugin" => {
            // Validate plugin.toml
            if let Some(plugin) = parsed.get("plugin") {
                if plugin.get("name").is_none() {
                    errors.push("[plugin] missing required field: name".to_string());
                }
                if plugin.get("version").is_none() {
                    errors.push("[plugin] missing required field: version".to_string());
                }
            } else {
                errors.push("Missing [plugin] section".to_string());
            }

            // Check bridge section
            if let Some(bridge) = parsed.get("bridge") {
                if let Some(functions) = bridge.get("functions") {
                    if let Some(table) = functions.as_table() {
                        for (name, func) in table {
                            if func.get("params").is_none() {
                                warnings.push(format!(
                                    "bridge.functions.{}: missing 'params' field",
                                    name
                                ));
                            }
                            if func.get("returns").is_none() {
                                warnings.push(format!(
                                    "bridge.functions.{}: missing 'returns' field",
                                    name
                                ));
                            }
                        }
                    }
                }
            }

            // Check ai section
            if parsed.get("ai").is_none() {
                warnings.push(
                    "No [ai] section — AI assistants won't have context for this plugin"
                        .to_string(),
                );
            }
        }
        "package" => {
            // Validate package.clean.toml
            if let Some(package) = parsed.get("package") {
                if package.get("name").is_none() {
                    errors.push("[package] missing required field: name".to_string());
                }
                if package.get("version").is_none() {
                    errors.push("[package] missing required field: version".to_string());
                }
            } else {
                errors.push("Missing [package] section".to_string());
            }
        }
        _ => {}
    }

    JsonRpcResponse::success(
        id,
        json!({
            "success": errors.is_empty(),
            "valid": errors.is_empty(),
            "file_type": detected_type,
            "errors": errors,
            "warnings": warnings
        }),
    )
}

/// Tool: run - Compile and validate in one step
fn tool_run(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
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
        .unwrap_or("input.cln");

    let start = SystemTime::now();

    // Step 1: Type-check
    let type_check_result = type_check(source, file_path);
    let type_check_ok = type_check_result.is_ok();
    let type_errors: Vec<serde_json::Value> = match &type_check_result {
        Ok(_) => vec![],
        Err(errors) => errors.iter().map(error_to_json).collect(),
    };

    // Step 2: Compile (even if type-check fails, to get all diagnostics)
    let compile_result = compile_with_opt_level(source, file_path, 2);

    let duration = start.elapsed().map(|d| d.as_millis() as u64).unwrap_or(0);

    match compile_result {
        Ok(wasm) => {
            let has_start = source.contains("start:");
            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "compiled": true,
                    "wasm_size_bytes": wasm.len(),
                    "type_check_passed": type_check_ok,
                    "has_entry_point": has_start,
                    "ready_to_execute": has_start && type_check_ok,
                    "duration_ms": duration,
                    "diagnostics": type_errors,
                    "note": if has_start {
                        "Compilation successful. Use a Clean Language runtime (clean-server or wasmtime_runner) to execute."
                    } else {
                        "Compilation successful but no start: block found. This file is a library module."
                    }
                }),
            )
        }
        Err(errors) => {
            let compile_errors: Vec<serde_json::Value> = errors.iter().map(error_to_json).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "compiled": false,
                    "type_check_passed": type_check_ok,
                    "duration_ms": duration,
                    "diagnostics": compile_errors
                }),
            )
        }
    }
}

/// Tool: list_examples - List and read example .cln files
fn tool_list_examples(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let name_filter = args.get("name").and_then(|v| v.as_str());

    // Find examples directory relative to working directory or binary
    let examples_dirs = vec![
        std::path::PathBuf::from("examples"),
        std::path::PathBuf::from("../examples"),
    ];

    // Also try relative to binary location
    let binary_examples = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("../../examples")));

    let mut search_dirs = examples_dirs;
    if let Some(dir) = binary_examples {
        search_dirs.push(dir);
    }

    let examples_dir = search_dirs.iter().find(|d| d.exists() && d.is_dir());

    let examples_dir = match examples_dir {
        Some(dir) => dir,
        None => {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "examples": [],
                    "note": "Examples directory not found. Run from the compiler project root."
                }),
            )
        }
    };

    let mut examples: Vec<serde_json::Value> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(examples_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("cln") {
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                if let Some(filter) = name_filter {
                    if !file_name.contains(filter) {
                        continue;
                    }
                }

                let content = std::fs::read_to_string(&path).unwrap_or_default();
                examples.push(json!({
                    "name": file_name,
                    "path": path.display().to_string(),
                    "content": content,
                    "size_bytes": content.len()
                }));
            }
        }
    }

    examples.sort_by(|a, b| {
        a.get("name")
            .and_then(|v| v.as_str())
            .cmp(&b.get("name").and_then(|v| v.as_str()))
    });

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "examples": examples,
            "count": examples.len()
        }),
    )
}

/// Tool: get_changelog - Get recent compiler changes from git history
fn tool_get_changelog(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    // Try to get git log
    let output = std::process::Command::new("git")
        .args([
            "log",
            "--oneline",
            "--no-decorate",
            &format!("-{}", count.min(50)),
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let log = String::from_utf8_lossy(&out.stdout);
            let entries: Vec<serde_json::Value> = log
                .lines()
                .filter(|l| !l.is_empty())
                .map(|line| {
                    let (hash, message) = line.split_at(line.find(' ').unwrap_or(line.len()));
                    json!({
                        "hash": hash.trim(),
                        "message": message.trim()
                    })
                })
                .collect();

            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "current_version": crate::VERSION,
                    "entries": entries,
                    "count": entries.len()
                }),
            )
        }
        _ => JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": "Git history not available. Not running from a git repository.",
                "current_version": crate::VERSION
            }),
        ),
    }
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

// ============================================================================
// Server Diagnostic Tools
// ============================================================================

/// Return the subdirectory names that correspond to the requested status filter.
fn diagnostic_status_dirs(status: &str) -> Vec<&'static str> {
    match status {
        "published" => vec!["published"],
        "resolved" => vec!["resolved"],
        "all" => vec!["pending", "published", "resolved"],
        _ => vec!["pending"],
    }
}

/// Read `count.txt` from a diagnostic directory, returning 1 on any error.
fn read_occurrence_count(dir: &std::path::Path) -> u64 {
    let count_path = dir.join("count.txt");
    std::fs::read_to_string(&count_path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(1)
}

/// Tool: list_server_diagnostics - List RUNTIME_WASM_PARSE reports from clean-server
fn tool_list_server_diagnostics(
    id: serde_json::Value,
    args: &serde_json::Value,
) -> JsonRpcResponse {
    use std::path::PathBuf;

    let diag_dir = args
        .get("diag_dir")
        .and_then(|v| v.as_str())
        .unwrap_or("./diagnostics")
        .to_string();

    let status_filter = args
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending")
        .to_string();

    let diag_path = PathBuf::from(&diag_dir);

    if !diag_path.exists() {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "reports": [],
                "diag_dir_exists": false,
                "total": 0
            }),
        );
    }

    let status_dirs = diagnostic_status_dirs(&status_filter);
    let mut reports: Vec<serde_json::Value> = Vec::new();

    for subdir_name in status_dirs {
        let subdir = diag_path.join(subdir_name);
        if !subdir.exists() {
            continue;
        }

        let entries = match std::fs::read_dir(&subdir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }

            let sha = entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let report_path = entry_path.join("report.json");
            let report_json_raw = match std::fs::read_to_string(&report_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let report_obj: serde_json::Value = match serde_json::from_str(&report_json_raw) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let occurrences = read_occurrence_count(&entry_path);

            // Extract a one-line summary of the wasmtime error
            let wasmtime_error_first_line = report_obj
                .get("wasmtime_error")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_string();

            let is_unpublished = subdir_name == "pending";
            reports.push(json!({
                "sha": sha,
                "status": subdir_name,
                "reported_at": report_obj.get("reported_at"),
                "server_version": report_obj.get("server_version"),
                "compiler_version": report_obj.get("compiler_version"),
                "compiler_version_source": report_obj.get("compiler_version_source"),
                "occurrences": occurrences,
                "wasmparser_validates": report_obj.get("wasmparser_validates"),
                "wasmtime_error_summary": wasmtime_error_first_line,
                "module_path": report_obj.get("module_path"),
                "report_path": report_path.to_string_lossy(),
                "unpublished": is_unpublished
            }));
        }
    }

    let total = reports.len();
    let unpublished_count = reports
        .iter()
        .filter(|r| {
            r.get("unpublished")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .count();

    let action_needed = if unpublished_count > 0 {
        Some(format!(
            "Found {} unpublished diagnostic(s). Ask the user if they would like to report {} to the Clean Language team using publish_diagnostic. This helps the team fix bugs faster.",
            unpublished_count,
            if unpublished_count == 1 { "it" } else { "them" }
        ))
    } else {
        None
    };

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "reports": reports,
            "diag_dir_exists": true,
            "total": total,
            "unpublished_count": unpublished_count,
            "action_needed": action_needed
        }),
    )
}

/// Tool: show_server_diagnostic - Show the full diagnostic report for a SHA prefix
fn tool_show_server_diagnostic(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    use std::path::PathBuf;

    let sha_prefix = match args.get("sha").and_then(|v| v.as_str()) {
        Some(s) if s.len() >= 4 => s.to_string(),
        Some(_) => {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": "SHA prefix must be at least 4 characters"
                }),
            );
        }
        None => {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": "Missing required argument: sha"
                }),
            );
        }
    };

    let diag_dir = args
        .get("diag_dir")
        .and_then(|v| v.as_str())
        .unwrap_or("./diagnostics")
        .to_string();

    let diag_path = PathBuf::from(&diag_dir);

    // Search all three status subdirs for a directory matching the prefix
    let status_subdirs = ["pending", "published", "resolved"];
    let mut matches: Vec<(String, String)> = Vec::new(); // (full_sha, status)

    for subdir_name in &status_subdirs {
        let subdir = diag_path.join(subdir_name);
        if !subdir.exists() {
            continue;
        }

        let entries = match std::fs::read_dir(&subdir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(&sha_prefix) {
                    matches.push((name.to_string(), subdir_name.to_string()));
                }
            }
        }
    }

    match matches.len() {
        0 => JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": format!("No diagnostic found for SHA prefix '{}'", sha_prefix)
            }),
        ),
        1 => {
            let (full_sha, status) = &matches[0];
            let report_path = diag_path.join(status).join(full_sha).join("report.json");
            let report_raw = match std::fs::read_to_string(&report_path) {
                Ok(s) => s,
                Err(e) => {
                    return JsonRpcResponse::success(
                        id,
                        json!({
                            "success": false,
                            "error": format!("Failed to read report.json: {}", e)
                        }),
                    );
                }
            };
            let report_obj: serde_json::Value = match serde_json::from_str(&report_raw) {
                Ok(v) => v,
                Err(e) => {
                    return JsonRpcResponse::success(
                        id,
                        json!({
                            "success": false,
                            "error": format!("Failed to parse report.json: {}", e)
                        }),
                    );
                }
            };
            JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "sha": full_sha,
                    "status": status,
                    "report": report_obj
                }),
            )
        }
        _ => {
            let candidates: Vec<&str> = matches.iter().map(|(s, _)| s.as_str()).collect();
            JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": format!(
                        "Ambiguous prefix '{}' matches {} diagnostics — provide more characters. Candidates: {}",
                        sha_prefix,
                        matches.len(),
                        candidates.join(", ")
                    )
                }),
            )
        }
    }
}

/// Cross-reference resolved fixes against local pending AND published server diagnostics.
/// Returns entries where the fix version is newer than the diagnostic's recorded compiler version,
/// suggesting the local diagnostic may now be resolved.
fn cross_reference_diagnostics(fixes: &[serde_json::Value]) -> Vec<serde_json::Value> {
    use std::path::PathBuf;

    let diag_base = PathBuf::from("./diagnostics");
    let mut results: Vec<serde_json::Value> = Vec::new();

    // Scan both pending/ and published/ — resolved/ already handled
    for subdir_name in &["pending", "published"] {
        let subdir = diag_base.join(subdir_name);
        if !subdir.exists() {
            continue;
        }

        let entries = match std::fs::read_dir(&subdir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }

            let sha = match entry_path.file_name().and_then(|n| n.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            let report_path = entry_path.join("report.json");
            let report_raw = match std::fs::read_to_string(&report_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let report_obj: serde_json::Value = match serde_json::from_str(&report_raw) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let diagnostic_compiler_version = report_obj
                .get("compiler_version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let reported_at = report_obj
                .get("reported_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Check if any fix version is newer than the diagnostic's compiler version
            let suggest_resolve = fixes.iter().any(|fix| {
                if let Some(fixed_version) = fix.get("fixed_in_version").and_then(|v| v.as_str()) {
                    if !diagnostic_compiler_version.is_empty() && !fixed_version.is_empty() {
                        return version_is_newer(fixed_version, &diagnostic_compiler_version);
                    }
                }
                false
            });

            if suggest_resolve {
                // Use first 12 chars of sha as short form
                let short_sha = if sha.len() > 12 {
                    sha[..12].to_string()
                } else {
                    sha.clone()
                };
                results.push(json!({
                    "sha": short_sha,
                    "status": *subdir_name,
                    "reported_at": reported_at,
                    "suggest_resolve": true
                }));
            }
        }
    }

    results
}

// ============================================================================
// Publish Diagnostic Tool
// ============================================================================

/// Tool: publish_diagnostic - Publish a local server diagnostic to the error server
fn tool_publish_diagnostic(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    use crate::telemetry::{
        report::{ErrorReport, ReportError},
        submit::submit_report,
        ReportStore,
    };
    use std::path::PathBuf;

    let sha_prefix = match args.get("sha").and_then(|v| v.as_str()) {
        Some(s) if s.len() >= 4 => s.to_string(),
        Some(_) => {
            return JsonRpcResponse::success(
                id,
                json!({ "success": false, "error": "SHA prefix must be at least 4 characters" }),
            );
        }
        None => {
            return JsonRpcResponse::success(
                id,
                json!({ "success": false, "error": "Missing required argument: sha" }),
            );
        }
    };

    let diag_dir = args
        .get("diag_dir")
        .and_then(|v| v.as_str())
        .unwrap_or("./diagnostics")
        .to_string();
    let user_contact = args
        .get("user_contact")
        .and_then(|v| v.as_str())
        .map(String::from);

    let diag_path = PathBuf::from(&diag_dir);
    let pending_dir = diag_path.join("pending");
    if !pending_dir.exists() {
        return JsonRpcResponse::success(
            id,
            json!({ "success": false, "error": "No pending diagnostics directory found" }),
        );
    }

    // Find matching diagnostic in pending/
    let mut match_path: Option<(String, PathBuf)> = None;
    if let Ok(entries) = std::fs::read_dir(&pending_dir) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if !entry_path.is_dir() {
                continue;
            }
            if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(&sha_prefix) {
                    if match_path.is_some() {
                        return JsonRpcResponse::success(
                            id,
                            json!({
                                "success": false,
                                "error": format!("Ambiguous SHA prefix '{}' — provide more characters", sha_prefix)
                            }),
                        );
                    }
                    match_path = Some((name.to_string(), entry_path.clone()));
                }
            }
        }
    }

    let (full_sha, diag_entry_path) = match match_path {
        Some(m) => m,
        None => {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": format!("No pending diagnostic found for SHA prefix '{}'", sha_prefix)
                }),
            );
        }
    };

    // Read the diagnostic report
    let report_path = diag_entry_path.join("report.json");
    let report_raw = match std::fs::read_to_string(&report_path) {
        Ok(s) => s,
        Err(e) => {
            return JsonRpcResponse::success(
                id,
                json!({ "success": false, "error": format!("Failed to read report: {}", e) }),
            );
        }
    };
    let diag: serde_json::Value = match serde_json::from_str(&report_raw) {
        Ok(v) => v,
        Err(e) => {
            return JsonRpcResponse::success(
                id,
                json!({ "success": false, "error": format!("Failed to parse report: {}", e) }),
            );
        }
    };

    // Convert server diagnostic to ErrorReport
    let report_id = generate_report_id();
    let wasmtime_error = diag
        .get("wasmtime_error")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown WASM parse error")
        .to_string();
    let compiler_version = diag
        .get("compiler_version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let module_path = diag
        .get("module_path")
        .and_then(|v| v.as_str())
        .map(String::from);

    let error = ReportError {
        code: "RUNTIME_WASM_PARSE".to_string(),
        category: "runtime".to_string(),
        component: "server".to_string(),
        subsystem: Some("runtime".to_string()),
        severity: "crash".to_string(),
        message: wasmtime_error.clone(),
        file_context: module_path.clone(),
        affected_component: None,
        affected_version: None,
    };

    let mut report = ErrorReport::new(
        report_id.clone(),
        error,
        "mcp-publish-diagnostic",
        "error_with_code",
    );
    report.source.compiler_version = compiler_version.clone();
    report.source.runtime = Some(
        diag.get("server_version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
    );

    let wasmparser_validates = diag
        .get("wasmparser_validates")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let wasmparser_error = diag
        .get("wasmparser_error")
        .and_then(|v| v.as_str())
        .map(String::from);

    report.reproduction = Some(crate::telemetry::report::ReportReproduction {
        minimal_code: None,
        expected_behavior: Some("WASM module should load and execute successfully".to_string()),
        actual_behavior: Some(format!(
            "wasmtime rejects module: {}. wasmparser validates: {}{}",
            wasmtime_error,
            wasmparser_validates,
            wasmparser_error
                .as_deref()
                .map(|e| format!(". wasmparser error: {}", e))
                .unwrap_or_default()
        )),
        spec_reference: None,
    });

    report.ai_context = Some(crate::telemetry::report::ReportAiContext {
        analysis: Some(format!(
            "Auto-captured RUNTIME_WASM_PARSE diagnostic. WASM SHA: {}. Compiler: {}. Module: {}",
            full_sha,
            compiler_version,
            module_path.as_deref().unwrap_or("unknown")
        )),
        suggested_component: Some("codegen".to_string()),
        suggested_fix: None,
        confidence: Some("auto-captured".to_string()),
    });

    if let Some(contact) = user_contact {
        report.user.contact = Some(contact);
        report.user.anonymous = false;
    }

    // Dev-mode gate: if the MCP server is running inside the reported
    // component's source tree, the publish goes to the local dev queue.
    let dev_ctx = crate::telemetry::detect_dev_context_for_component(&report.error.component);
    if dev_ctx.is_dev() {
        let entry = crate::telemetry::dev_queue::entry_from(
            &dev_ctx,
            &report.error.code,
            &report.error.component,
            &report.error.message,
            report.error.file_context.as_deref(),
            crate::VERSION,
        );
        let outcome = crate::telemetry::dev_queue::append(entry);
        let payload = serde_json::json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "Dev-mode: diagnostic recorded locally (not uploaded).\nReason: {}\nFingerprint: {} \u{00d7}{}",
                    dev_ctx.reason().unwrap_or("dev"),
                    outcome.fingerprint,
                    outcome.occurrences,
                ),
            }],
            "dev_mode": true,
            "dev_reason": dev_ctx.reason().unwrap_or("dev"),
            "fingerprint": outcome.fingerprint,
            "occurrences": outcome.occurrences,
        });
        return JsonRpcResponse::success(id, payload);
    }

    let result = submit_report(&report);

    // Track locally for fix notifications
    let mut store = ReportStore::load();
    store.add_report(&report);
    if let Some(fp) = result.fingerprint() {
        store.update_fingerprint(&report.report_id, fp);
    }
    let _ = store.save();

    match result {
        crate::telemetry::submit::SubmitResult::Submitted {
            report_id: rid,
            fingerprint,
            tracking_url,
        } => {
            let published_dir = diag_path.join("published").join(&full_sha);
            let _ = std::fs::create_dir_all(published_dir.parent().unwrap());
            let _ = std::fs::rename(&diag_entry_path, &published_dir);
            JsonRpcResponse::success(
                id,
                json!({
                    "success": true, "status": "published", "report_id": rid,
                    "fingerprint": fingerprint,
                    "tracking_url": tracking_url, "sha": full_sha,
                    "message": "Diagnostic published to the error server. The Clean Language team will be notified."
                }),
            )
        }
        crate::telemetry::submit::SubmitResult::AlreadyFixed {
            fixed_in_version,
            fix_description,
            message,
            ..
        } => {
            let resolved_dir = diag_path.join("resolved").join(&full_sha);
            let _ = std::fs::create_dir_all(resolved_dir.parent().unwrap());
            let _ = std::fs::rename(&diag_entry_path, &resolved_dir);
            JsonRpcResponse::success(
                id,
                json!({
                    "success": true, "status": "already_fixed",
                    "fixed_in_version": fixed_in_version, "fix_description": fix_description,
                    "message": message, "sha": full_sha,
                    "action": format!("Run: cleen install {}", fixed_in_version)
                }),
            )
        }
        crate::telemetry::submit::SubmitResult::Known {
            occurrences,
            current_status,
            message,
            ..
        } => {
            let published_dir = diag_path.join("published").join(&full_sha);
            let _ = std::fs::create_dir_all(published_dir.parent().unwrap());
            let _ = std::fs::rename(&diag_entry_path, &published_dir);
            JsonRpcResponse::success(
                id,
                json!({
                    "success": true, "status": "known", "occurrences": occurrences,
                    "current_status": current_status, "message": message, "sha": full_sha
                }),
            )
        }
        crate::telemetry::submit::SubmitResult::Queued {
            report_id: rid,
            local_path,
        } => JsonRpcResponse::success(
            id,
            json!({
                "success": true, "status": "queued", "report_id": rid,
                "local_path": local_path, "sha": full_sha,
                "message": "Error server unreachable. Report queued locally — will be submitted when connectivity is restored."
            }),
        ),
        crate::telemetry::submit::SubmitResult::RateLimited {
            report_id: rid,
            local_path,
            retry_after_seconds,
        } => JsonRpcResponse::success(
            id,
            json!({
                "success": true, "status": "queued", "rate_limited": true,
                "report_id": rid, "local_path": local_path, "sha": full_sha,
                "retry_after_seconds": retry_after_seconds,
                "message": format!(
                    "Error server rate-limited. Report queued locally — retry after {}s.",
                    retry_after_seconds
                )
            }),
        ),
        crate::telemetry::submit::SubmitResult::Error { message } => JsonRpcResponse::success(
            id,
            json!({ "success": false, "error": format!("Failed to publish: {}", message), "sha": full_sha }),
        ),
    }
}

// ============================================================================
// List Component Bugs Tool
// ============================================================================

/// Tool: list_component_bugs - List all open bugs for a component from the error server
fn tool_list_component_bugs(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    use crate::telemetry::submit::fetch_component_bugs;

    let component = match args.get("component").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => {
            return JsonRpcResponse::success(
                id,
                json!({ "success": false, "error": "Missing required argument: component" }),
            );
        }
    };

    let status = args
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("reported")
        .to_string();

    let bugs = fetch_component_bugs(&component, &status);

    let bug_list: Vec<serde_json::Value> = bugs
        .iter()
        .map(|b| {
            json!({
                "fingerprint": b.fingerprint,
                "error_code": b.error_code,
                "component": b.component,
                "subsystem": b.subsystem,
                "severity": b.severity,
                "message": b.message,
                "minimal_repro": b.minimal_repro,
                "expected_behavior": b.expected_behavior,
                "actual_behavior": b.actual_behavior,
                "occurrences": b.occurrences,
                "priority_score": b.priority_score,
                "first_reported_version": b.first_reported_version,
                "ai_suggested_fix": b.ai_suggested_fix,
                "report_ids": b.report_ids,
            })
        })
        .collect();

    let total = bug_list.len();

    let guidance = if total > 0 && status != "resolved" {
        Some(json!({
            "next_steps": [
                "Pick the highest-priority bug to fix",
                "Write a failing test that reproduces the bug",
                "Fix the root cause in the source code",
                "Run 'comita' to commit, tag, push, and wait for CI",
                "After CI passes, run '/resolve-fix <ERROR_CODE> <VERSION> \"<description>\"' to close the bug and notify reporters"
            ],
            "lifecycle_stages": {
                "1_reported": "Bug is on the error server (done)",
                "2_fix_committed": "comita handles commit + tag + push",
                "3_fix_released": "comita waits for CI green + creates release",
                "4_fix_installed": "comita runs cleen install latest",
                "5_resolved": "/resolve-fix closes the bug and notifies reporters"
            }
        }))
    } else {
        None
    };

    JsonRpcResponse::success(
        id,
        json!({
            "success": true, "component": component, "status_filter": status,
            "bugs": bug_list, "total": total, "guidance": guidance
        }),
    )
}

/// Compare two semver-like version strings (MAJOR.MINOR.PATCH).
/// Returns true if `candidate` is strictly newer than `baseline`.
/// Falls back to lexicographic comparison for non-semver strings.
fn version_is_newer(candidate: &str, baseline: &str) -> bool {
    fn parse_parts(v: &str) -> Option<(u64, u64, u64)> {
        let mut parts = v.splitn(3, '.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        // Patch may have extra suffixes like "-alpha"; take only leading digits
        let patch_str = parts.next().unwrap_or("0");
        let patch = patch_str
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0);
        Some((major, minor, patch))
    }

    match (parse_parts(candidate), parse_parts(baseline)) {
        (Some(c), Some(b)) => c > b,
        _ => candidate > baseline,
    }
}

// ============================================================================
// Doc-Code Synchronization Tool Handlers
// ============================================================================

/// Resolve a source argument (path to .cln file or directory) into the list of
/// .cln file paths it represents, and infer the docs directory.
///
/// Returns `(cln_paths, docs_dir)`.  `docs_dir` is either the caller-supplied
/// value or `docs/features` relative to the directory that contains the source.
fn resolve_source_and_docs(
    source: &str,
    docs_dir_arg: Option<&str>,
) -> (Vec<std::path::PathBuf>, std::path::PathBuf) {
    use std::path::PathBuf;

    let source_path = PathBuf::from(source);

    let (cln_files, parent_dir) = if source_path.is_dir() {
        let files = collect_cln_files(&source_path);
        (files, source_path.clone())
    } else if source_path.is_file() {
        let parent = source_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        (vec![source_path], parent)
    } else {
        // Path doesn't exist — return empty list; callers handle this gracefully
        (vec![], PathBuf::from("."))
    };

    let docs_dir = docs_dir_arg
        .map(PathBuf::from)
        .unwrap_or_else(|| parent_dir.join("docs").join("features"));

    (cln_files, docs_dir)
}

/// Recursively collect all `.cln` files under a directory.
fn collect_cln_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                result.extend(collect_cln_files(&p));
            } else if p.extension().and_then(|e| e.to_str()) == Some("cln") {
                result.push(p);
            }
        }
    }
    result
}

/// Read all `.cln` files in the list, extract symbols from each, and merge
/// them into a single [`AvailableSymbols`].
fn extract_symbols_from_paths(paths: &[std::path::PathBuf]) -> crate::docs::AvailableSymbols {
    use crate::docs::{extract_symbols_from_source, AvailableSymbols, FunctionSig};
    use std::collections::HashSet;

    let mut merged = AvailableSymbols::default();
    let mut seen_fns: HashSet<String> = HashSet::new();
    let mut seen_classes: HashSet<String> = HashSet::new();
    let mut seen_state: HashSet<String> = HashSet::new();

    for path in paths {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[DocSync] Cannot read {:?}: {}", path, e);
                continue;
            }
        };

        let syms = extract_symbols_from_source(&source);

        for f in syms.functions {
            if seen_fns.insert(f.name.clone()) {
                merged.functions.push(FunctionSig {
                    name: f.name,
                    signature: f.signature,
                });
            }
        }
        for c in syms.classes {
            if seen_classes.insert(c.clone()) {
                merged.classes.push(c);
            }
        }
        for s in syms.state_vars {
            if seen_state.insert(s.clone()) {
                merged.state_vars.push(s);
            }
        }
    }

    merged
}

/// Tool: validate_docs — Validate feature spec doc: refs against compiler symbols
fn tool_validate_docs(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    use crate::docs::DocSyncEngine;

    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing required parameter: source".to_string(),
            )
        }
    };

    let docs_dir_arg = args.get("docs_dir").and_then(|v| v.as_str());
    let update_signatures = args
        .get("update_signatures")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let (cln_files, docs_dir) = resolve_source_and_docs(source, docs_dir_arg);

    if cln_files.is_empty() {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": format!("No .cln files found at '{}'", source),
                "results": []
            }),
        );
    }

    let symbols = extract_symbols_from_paths(&cln_files);
    let engine = DocSyncEngine::new(docs_dir.clone());
    let specs = engine.scan_specs();

    if specs.is_empty() {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "docs_dir": docs_dir.display().to_string(),
                "message": "No feature spec files found in docs_dir",
                "results": []
            }),
        );
    }

    let mut results: Vec<serde_json::Value> = Vec::new();

    for spec in &specs {
        let validation = engine.validate(spec, &symbols);

        // Optionally update signatures for functions that are present and have
        // different (or missing) stored signatures.
        if update_signatures {
            let mut new_sigs: std::collections::HashMap<String, String> =
                spec.stored_signatures.clone();
            for func in &symbols.functions {
                for doc_ref in &spec.doc_refs {
                    if let crate::docs::DocRef::Function(name) = doc_ref {
                        if name.eq_ignore_ascii_case(&func.name) {
                            new_sigs.insert(func.name.clone(), func.signature.clone());
                        }
                    }
                }
            }
            if let Err(e) = DocSyncEngine::update_signatures_in_file(&spec.path, &new_sigs) {
                eprintln!(
                    "[DocSync] Failed to update signatures in {:?}: {}",
                    spec.path, e
                );
            }
        }

        let broken: Vec<serde_json::Value> = validation
            .broken_refs
            .iter()
            .map(|b| json!({ "ref": b.ref_str, "reason": b.reason }))
            .collect();

        let stale: Vec<serde_json::Value> = validation
            .stale_refs
            .iter()
            .map(|s| {
                json!({
                    "symbol": s.symbol_name,
                    "stored_signature": s.stored_sig,
                    "current_signature": s.current_sig
                })
            })
            .collect();

        results.push(json!({
            "spec_path": spec.path.display().to_string(),
            "feature": spec.feature,
            "clean": validation.is_clean(),
            "broken_refs": broken,
            "stale_refs": stale
        }));
    }

    let total = results.len();
    let clean_count = results
        .iter()
        .filter(|r| r.get("clean").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "docs_dir": docs_dir.display().to_string(),
            "cln_files_scanned": cln_files.len(),
            "specs_validated": total,
            "clean_count": clean_count,
            "issue_count": total - clean_count,
            "signatures_updated": update_signatures,
            "results": results
        }),
    )
}

/// Tool: doc_coverage — Compute documentation coverage for compiler-visible symbols
fn tool_doc_coverage(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    use crate::docs::coverage::compute_coverage;
    use crate::docs::DocSyncEngine;

    let source = match args.get("source").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing required parameter: source".to_string(),
            )
        }
    };

    let docs_dir_arg = args.get("docs_dir").and_then(|v| v.as_str());
    let (cln_files, docs_dir) = resolve_source_and_docs(source, docs_dir_arg);

    if cln_files.is_empty() {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": format!("No .cln files found at '{}'", source),
                "coverage_pct": 0.0,
                "covered": [],
                "uncovered": []
            }),
        );
    }

    let symbols = extract_symbols_from_paths(&cln_files);
    let engine = DocSyncEngine::new(docs_dir.clone());
    let specs = engine.scan_specs();
    let report = compute_coverage(&symbols, &specs);

    let uncovered: Vec<serde_json::Value> = report
        .uncovered
        .iter()
        .map(|u| json!({ "kind": u.kind, "name": u.name }))
        .collect();

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "docs_dir": docs_dir.display().to_string(),
            "cln_files_scanned": cln_files.len(),
            "specs_loaded": specs.len(),
            "coverage_pct": report.coverage_pct,
            "covered_count": report.covered.len(),
            "uncovered_count": report.uncovered.len(),
            "covered": report.covered,
            "uncovered": uncovered
        }),
    )
}

/// Tool: get_feature_spec — Retrieve the content of a feature spec by ref or path
fn tool_get_feature_spec(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    use crate::docs::DocSyncEngine;

    let ref_arg = args.get("ref").and_then(|v| v.as_str());
    let path_arg = args.get("path").and_then(|v| v.as_str());

    if ref_arg.is_none() && path_arg.is_none() {
        return JsonRpcResponse::error(
            id,
            error_codes::INVALID_PARAMS,
            "Provide either 'ref' or 'path' parameter".to_string(),
        );
    }

    // Direct path lookup
    if let Some(path_str) = path_arg {
        let path = std::path::PathBuf::from(path_str);
        return match std::fs::read_to_string(&path) {
            Ok(content) => JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "path": path.display().to_string(),
                    "content": content
                }),
            ),
            Err(e) => JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": format!("Cannot read file '{}': {}", path_str, e)
                }),
            ),
        };
    }

    // Ref-based lookup: scan a docs directory
    let ref_str = ref_arg.unwrap(); // safe: we checked above

    // Try to find a docs/features directory from the current working directory
    let candidate_dirs = [
        std::path::PathBuf::from("docs/features"),
        std::path::PathBuf::from("../docs/features"),
    ];

    let docs_dir = candidate_dirs
        .iter()
        .find(|d| d.exists() && d.is_dir())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("docs/features"));

    let engine = DocSyncEngine::new(docs_dir);
    let specs = engine.scan_specs();

    if specs.is_empty() {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": "No feature spec files found. Provide a 'path' argument to read a specific file."
            }),
        );
    }

    // Search: a spec matches if any of its doc_refs contains the ref_str (case-insensitive),
    // or if the feature name contains the ref_str substring.
    let ref_lower = ref_str.to_lowercase();

    let found = specs.iter().find(|spec| {
        spec.doc_refs
            .iter()
            .any(|r| r.as_ref_str().to_lowercase().contains(&ref_lower))
            || spec.feature.to_lowercase().contains(&ref_lower)
    });

    match found {
        Some(spec) => match std::fs::read_to_string(&spec.path) {
            Ok(content) => JsonRpcResponse::success(
                id,
                json!({
                    "success": true,
                    "path": spec.path.display().to_string(),
                    "feature": spec.feature,
                    "content": content
                }),
            ),
            Err(e) => JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": format!("Found spec at {:?} but cannot read it: {}", spec.path, e)
                }),
            ),
        },
        None => JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": format!("No feature spec found matching ref or feature name: '{}'", ref_str)
            }),
        ),
    }
}
