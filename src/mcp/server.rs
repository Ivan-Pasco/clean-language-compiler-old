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
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
    let default_path = "documentation/Clean_Language_Specification.md";
    if std::path::Path::new(default_path).exists() {
        return Some(default_path.to_string());
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
            // EOF reached
            eprintln!("[MCP] EOF detected, shutting down");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        eprintln!("[MCP] <- {}", trimmed);

        // Parse JSON-RPC request
        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
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

        // Handle the request
        let response = handle_request(request).await;
        write_response(&mut stdout, &response).await?;
    }

    Ok(())
}

/// Write a JSON-RPC response to stdout
async fn write_response(
    stdout: &mut tokio::io::Stdout,
    response: &JsonRpcResponse,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(response)?;
    eprintln!("[MCP] -> {}", json);
    stdout.write_all(json.as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}

/// Handle a JSON-RPC request and return the appropriate response
async fn handle_request(request: JsonRpcRequest) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(request.id),
        "initialized" => {
            // Notification - no response needed
            eprintln!("[MCP] Client initialized");
            return JsonRpcResponse::success(request.id, json!(null));
        }
        "tools/list" => handle_tools_list(request.id),
        "tools/call" => handle_tools_call(request.id, request.params),
        _ => JsonRpcResponse::error(
            request.id,
            error_codes::METHOD_NOT_FOUND,
            format!("Method not found: {}", request.method),
        ),
    }
}

/// Handle the 'initialize' method
fn handle_initialize(id: serde_json::Value) -> JsonRpcResponse {
    let result = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "cln",
            "version": VERSION
        },
        "instructions": "Clean Language compiler MCP server. Call 'get_quick_reference' first to learn the language syntax, types, and patterns. Call 'list_plugins' to discover framework/plugin DSL syntax, then 'get_plugin_examples' to see usage patterns. Use 'check' for fast type-checking during development, 'compile' for WebAssembly output. All 14 tools are available — get_quick_reference is the starting point."
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
                        "description": "Optional project directory to search for project-specific plugins (in addition to global ~/.clean/plugins/)"
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
            "fix": explanation.fix
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
            let path = home.join(".clean").join("plugins").join(dir_name);
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
            version: "2.0.0",
            category: "server",
            description: "HTTP server plugin — routing, request context, response helpers, and authentication guards. Define REST APIs with endpoints: blocks.",
            blocks: &["server", "endpoints"],
            key_features: &[
                "HTTP route registration (GET, POST, PUT, PATCH, DELETE)",
                "Protected routes with role requirements",
                "Request context (params, query, headers, body, cookies)",
                "Response helpers (JSON, HTML, redirect, error)",
                "Response headers and caching control",
                "Authentication guard integration",
            ],
            bridge_function_count: 47,
            install: "cleen plugin add frame.httpserver",
            status: "stable",
            auto_detect_paths: &["/api/", "/backend/api/", "/server/api/", "/endpoints/"],
        },
        EcosystemPlugin {
            name: "frame.data",
            version: "2.0.0",
            category: "data",
            description: "ORM and database plugin — data models, CRUD operations, query builder, transactions, and raw SQL. Define models with data: blocks.",
            blocks: &["data"],
            key_features: &[
                "Model definition with typed fields",
                "Query builder (find, first, count, insert, update, delete)",
                "Filtering, sorting, and pagination",
                "Transaction support (Data.tx:)",
                "Raw SQL queries (db.query:, db.queryAs)",
                "Database migrations",
            ],
            bridge_function_count: 2,
            install: "cleen plugin add frame.data",
            status: "stable",
            auto_detect_paths: &["/data/", "/models/", "/server/models/"],
        },
        EcosystemPlugin {
            name: "frame.auth",
            version: "2.0.0",
            category: "auth",
            description: "Authentication and authorization plugin — sessions, JWT tokens, password hashing, roles/permissions, CSRF protection. Configure with auth: blocks.",
            blocks: &["auth", "protected", "login", "roles"],
            key_features: &[
                "Session management (store, get, delete)",
                "JWT token operations (sign, verify, decode)",
                "Password hashing (Argon2id)",
                "CSRF token management",
                "Role-based access control (RBAC)",
                "Permission checking",
                "Cookie management",
                "Environment variable access for secrets",
            ],
            bridge_function_count: 20,
            install: "cleen plugin add frame.auth",
            status: "stable",
            auto_detect_paths: &["/auth/", "/config/auth/"],
        },
        EcosystemPlugin {
            name: "frame.ui",
            version: "2.1.0",
            category: "ui",
            description: "HTML-first UI framework — components, screens, event handling, two-way binding, and hydration. Define components with component: blocks and HTML templates.",
            blocks: &["component", "screen", "page", "styles"],
            key_features: &[
                "Component definitions with properties and slots",
                "HTML directives (if, else, each, bind, show, validate)",
                "Event handling (onclick, oninput, onsubmit, etc.)",
                "Event modifiers (prevent, stop, once, enter, escape)",
                "Interpolation ({{escaped}} and {{{raw}}})",
                "Hydration modes (off, on, visible, idle, only)",
                "Two-way data binding",
                "State management for components",
            ],
            bridge_function_count: 10,
            install: "cleen plugin add frame.ui",
            status: "stable",
            auto_detect_paths: &["/ui/", "/components/", "/screens/"],
        },
        EcosystemPlugin {
            name: "frame.canvas",
            version: "2.0.0",
            category: "canvas",
            description: "Canvas rendering and game development plugin — drawing, animation, audio, sprites, input, collision detection, and easing functions. Define scenes with canvasScene: blocks.",
            blocks: &["canvasScene", "draw", "onFrame"],
            key_features: &[
                "Drawing primitives (circles, rectangles, lines, polygons)",
                "Text and image rendering",
                "Transform operations (translate, rotate, scale)",
                "Animation frame management",
                "Audio (sound effects and music with volume/pan control)",
                "Sprite sheet support",
                "Input handling (mouse, keyboard, touch, gamepad)",
                "Collision detection (7 types including raycasting)",
                "Camera and viewport control",
                "20 easing functions",
                "Scene management",
            ],
            bridge_function_count: 127,
            install: "cleen plugin add frame.canvas",
            status: "stable",
            auto_detect_paths: &["/canvas/"],
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

## Program Structure
Every program needs a `start:` block as entry point. Use tabs for indentation.

```
start:
	print("Hello, World!")
```

## Types
- `integer` — 64-bit signed integer
- `number` — 64-bit float
- `string` — UTF-8 text
- `boolean` — true/false
- `Array<T>` — typed arrays (e.g., Array<integer>)

## Variables
```
integer age = 25
number pi = 3.14159
string name = "Clean"
boolean active = true
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
// If/else — parentheses around condition
if (x > 0)
	print("positive")
else if (x == 0)
	print("zero")
else
	print("negative")

// While loop
while (i < 10)
	print(i.toString())
	i = i + 1

// Iterate (for-each over range)
iterate (i = 0 to 10)
	print(i.toString())

// Iterate over array
iterate (item in myArray)
	print(item.toString())
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
Array<string> parts = s.split(" ")
string trimmed = s.trim()
integer idx = s.indexOf("world")
```

## Math Operations
```
number result = Math.sqrt(16.0)
number sine = Math.sin(3.14)
number cosine = Math.cos(0.0)
number power = Math.pow(2.0, 10.0)
number absolute = Math.abs(-5.0)
number rounded = Math.round(3.7)
number floored = Math.floor(3.9)
number ceiled = Math.ceil(3.1)
number minimum = Math.min(5.0, 3.0)
number maximum = Math.max(5.0, 3.0)
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
print("text")              // print with newline
print("value: " + x.toString())  // concatenation
```

## Classes
```
class Person:
	properties:
		string name
		integer age

	constructor(string name, integer age)
		this.name = name
		this.age = age

	string greet()
		return "I'm " + this.name

start:
	Person p = Person("Alice", 30)
	print(p.greet())
	print(p.name)
```

## Inheritance
```
class Animal:
	properties:
		string name
	constructor(string name)
		this.name = name
	string speak()
		return this.name + " speaks"

class Dog extends Animal:
	properties:
		string breed
	constructor(string name, string breed)
		base(name)
		this.breed = breed
	string speak()
		return this.name + " barks"
```

## Arrays
```
Array<integer> nums = [1, 2, 3, 4, 5]
integer first = nums[0]
integer len = nums.length()
nums.push(6)
```

## Error Handling (require/rules)
```
functions:
	integer divide(integer a, integer b)
		require b != 0
		return a / b

	integer clamp(integer val, integer min, integer max)
		rules
			val >= min
			val <= max
		return val
```

## Computed Properties
```
class Circle:
	properties:
		number radius
	constructor(number radius)
		this.radius = radius
	computed:
		number area = 3.14159 * this.radius * this.radius
		number circumference = 2.0 * 3.14159 * this.radius
```

## JSON Operations
```
string jsonStr = json.stringify(data)
```

## Description (documentation)
```
functions:
	integer factorial(integer n)
		description "Calculates the factorial of n"
		require n >= 0
		if (n <= 1)
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

## Workflow
1. Call `get_quick_reference` (this tool) to learn base syntax
2. Call `list_ecosystem` to see ALL available plugins in the ecosystem
3. Call `list_plugins` to see installed plugins with full DSL details
4. Call `get_plugin_examples` to see plugin usage patterns
5. Write .cln code following the patterns above
6. Call `check` to type-check (fast feedback loop)
7. Call `compile` when ready for WASM output
8. If errors occur, call `explain_error` with the code
9. Use `get_specification` for detailed docs on specific features

## Plugin Syntax Discovery
Plugins (like Frame) add custom blocks, keywords, and types.
Call `list_plugins` to see what each plugin provides:
- blocks: DSL block names (endpoints:, data:, component:)
- keywords: Custom keywords (find, where, route)
- types: Custom types (Model, Endpoint)
- functions: Plugin functions (Data.tx, Http.route)
- ai.examples: Example files you can read with `get_plugin_examples`
"#;

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "quick_reference": quick_ref,
            "version": crate::VERSION,
            "tools_available": 14,
            "tip": "Use 'check' for fast iteration, 'compile' when ready for WASM."
        }),
    )
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
