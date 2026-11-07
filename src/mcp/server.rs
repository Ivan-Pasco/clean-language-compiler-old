/*!
 * MCP Server Implementation
 *
 * Implements the Model Context Protocol server for AI agent communication.
 * Reads JSON-RPC requests from stdin and writes responses to stdout.
 */

use super::protocol::{error_codes, JsonRpcRequest, JsonRpcResponse, Tool, ToolInputSchema};
use crate::plugins::PluginDiscovery;
use crate::resolver::symbol_table::{GlobalSymbolTable, SymbolKind};
use crate::{
    compile_source_with_detected_plugins, parse_to_ast, type_check_with_external_plugins, VERSION,
};
use serde_json::json;
use std::time::SystemTime;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

// ============================================================================
// Specification
// ============================================================================

/// The specification chapters (`foundation/docs/specification/NN-*.md`),
/// embedded at build time from the vendored copies in src/mcp/specification/
/// so `get_specification` works wherever `cln` is installed.
const SPEC_CHAPTERS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/spec_chapters.rs"));

#[derive(Debug)]
struct SpecSection {
    /// Chapter title, e.g. `06. Expressions`.
    title: String,
    /// Source file of the chapter.
    file: &'static str,
    content: &'static str,
}

/// One section per chapter, titled by the chapter's top-level heading.
fn spec_sections() -> &'static [SpecSection] {
    static SECTIONS: std::sync::OnceLock<Vec<SpecSection>> = std::sync::OnceLock::new();
    SECTIONS.get_or_init(|| {
        SPEC_CHAPTERS
            .iter()
            .map(|(file, content)| SpecSection {
                title: content
                    .lines()
                    .find_map(|l| l.strip_prefix("# "))
                    .map(|t| t.trim().to_string())
                    .unwrap_or_else(|| file.trim_end_matches(".md").to_string()),
                file,
                content,
            })
            .collect()
    })
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

        // MCP stdio messages are newline-delimited JSON. Content-Length
        // framed messages (LSP style) are accepted too, and each response
        // uses the framing of the request it answers.
        let framed = trimmed.starts_with("Content-Length:");
        let json_body = if framed {
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
                write_response(&mut stdout, &error_response, framed).await?;
                continue;
            }
        };

        if request.jsonrpc != "2.0" {
            let error_response = JsonRpcResponse::error(
                request.id.unwrap_or(json!(null)),
                error_codes::INVALID_REQUEST,
                format!(
                    "Invalid request: jsonrpc must be \"2.0\", got \"{}\"",
                    request.jsonrpc
                ),
            );
            write_response(&mut stdout, &error_response, framed).await?;
            continue;
        }

        // Check if this is a notification (no id field) — per JSON-RPC 2.0, no response
        let is_notification = request.id.is_none();

        if is_notification {
            eprintln!("[MCP] Notification: {}", request.method);
            handle_notification(&request.method);
            continue;
        }

        // Handle the request (has id, expects a response)
        let response = handle_request(request).await;
        write_response(&mut stdout, &response, framed).await?;
    }

    Ok(())
}

/// Write a JSON-RPC response to stdout: one JSON line (MCP stdio transport),
/// or with a `Content-Length` header when the request was framed that way.
async fn write_response(
    stdout: &mut tokio::io::Stdout,
    response: &JsonRpcResponse,
    framed: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(response)?;
    eprintln!("[MCP] -> {}", json);
    // Header + body go out as a single buffer to avoid partial flush delays.
    // Content-Length counts only the JSON body; the trailing \n separates
    // messages.
    let message = if framed {
        format!("Content-Length: {}\r\n\r\n{}\n", json.len(), json)
    } else {
        format!("{}\n", json)
    };
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
        "ping" => JsonRpcResponse::success(id, json!({})),
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
        "instructions": "You are working with the Clean Language compiler MCP server. Clean Language is a type-safe language that compiles to WebAssembly.\n\n## GETTING STARTED (call these tools in order)\n1. `get_quick_reference` — Learn syntax, types, patterns (CALL FIRST)\n2. `get_stack_recommendation` — Get recommended plugins for your project type\n3. `list_plugins` — See installed plugins with full DSL syntax\n4. `get_plugin_examples` — Read real example files from plugins\n\n## BEST PRACTICES FOR AI ASSISTANTS\n- ALWAYS call `get_quick_reference` at the start of a session before writing any Clean Language code\n- ALWAYS call `list_plugins` before using framework features (data:, endpoints:, component:, etc.)\n- Use `check` for fast type-checking during development (no WASM generation)\n- Use `get_specification` to look up detailed language rules\n- Use `get_architecture` to understand the execution model and host bridge\n- **JAVASCRIPT IS NEVER ACCEPTABLE** in Clean Language projects. No `<script>` tags, no `.js` files, no inline DOM API calls. This applies even when you are unsure if a plugin covers the interaction. Decision protocol: (1) call `list_plugins` and search for the `_ui_*` bridge function you need; (2) wire it through a `component:` block with exported handler functions; (3) if no bridge function exists for the interaction, call `report_error` with component `frame.ui` and stop — do NOT write JavaScript. Key equivalences: `addEventListener` → `_ui_on_event`, `classList.toggle` → `_ui_toggle_class`, `querySelector` → `_ui_query_selector`, `innerHTML` → `_ui_set_inner_html`. \"I don't know how to express this in Clean\" is a signal to call `list_plugins` harder, then `report_error` if truly absent.\n- Use `report_error` when you encounter what appears to be a compiler bug. It drafts a GitHub issue and returns the `gh issue create` command; nothing is sent anywhere, so show the command to the user\n- If the user reports a server runtime failure or WASM load error, call `list_server_diagnostics` BEFORE asking for reproduction details — the clean-server auto-captures structured diagnostics\n- Use `list_server_diagnostics` to surface pending compiler bugs the user may not realize are on disk\n\n## REPORT_ERROR — CRITICAL RULES\n- Bugs are filed as GitHub issues (label `compiler-old`) in the repository that owns the fix: Ivan-Pasco/clean-language-compiler-old (compiler, mcp), Ivan-Pasco/clean-server-old (server), Ivan-Pasco/cleen-framework-old (framework, frame.* plugins), Ivan-Pasco/clean-language-manager-old (manager). `report_error` picks the repository from `component`.\n- **SYNTHESIZE BEFORE REPORTING.** Before calling `report_error`, write a minimal reproduction and root-cause analysis — the team cannot fix a bug without a repro.\n- `minimal_repro` (REQUIRED): write the smallest possible `start:` block (or function) that triggers the same compiler failure. Write it from scratch — do NOT copy from the user's file.\n- `ai_analysis`: explain which pipeline stage fails and why.\n- `suggested_fix`: if you can identify the file and line, include it (e.g. `src/codegen/mod.rs line 847: condition is inverted`).\n- The `component` field MUST identify WHERE the bug lives (the component containing the buggy code), NOT the name of the task you were running (e.g. never use 'validation', 'testing', 'docs'). Invalid component names are rejected with an error.\n- Valid components: compiler, server, node-server, framework, manager, canvas, ui, mcp, unknown. If unsure, use 'unknown'.\n- Use `discovered_during` (optional) to record the context in which you found the bug (e.g. 'solving E0042', 'doc_coverage run') — this does not affect routing.\n- Before calling `report_error` during doc/spec validation or example checking: verify the error also reproduces when compiling the file normally with the correct plugin loaded. If the error disappears in that context, it is a false positive from missing plugin context — do NOT report it.\n- Errors discovered while solving another bug are independent reports. Report them under their correct component. Do NOT attach them to the original bug.\n\n## KEY RULES\n- File extension: .cln\n- Indentation: tabs only (not spaces)\n- Entry point: `start:` block\n- Types: integer, number, string, boolean, list<T>, matrix<T>, pairs\n- Functions declared in `functions:` block with return type first\n- No semicolons, no curly braces\n- `return value` (no parentheses)\n- One way to do things — follow the spec exactly\n\n## REPORTING RUNTIME_WASM_PARSE BUGS\n1. `list_server_diagnostics` → identify pending reports\n2. `show_server_diagnostic(sha)` → load the full payload\n3. `publish_diagnostic(sha)` → drafts the GitHub issue and returns the `gh issue create` command for the user to run\n4. After the user files it, tell them to run `clean-server errors publish <sha>` to mark it as forwarded"
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
            description: "List the error codes of the error-code registry (foundation/spec/error-codes.md) with their rule details.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "category": {
                        "type": "string",
                        "description": "Filter by category (Syntax, Semantic, Scope, Function, Class, Index Access, State, Import, Plugin, Compilation, Build, Runtime)"
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
            name: "get_app_structure".to_string(),
            description: "MANDATORY before creating any .cln file in an application project. Returns the canonical Clean Language application folder structure, the two architecture laws, a 10-step decision tree for placing new code, naming rules, forbidden patterns, and the growth path.".to_string(),
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
            description: "List the plugins installed for this toolchain, read from their plugin.toml manifests: name, version, description, blocks and bridge functions.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "category": {
                        "type": "string",
                        "description": "Filter by category, the last part of the plugin name ('server' for frame.server, 'data', 'auth', 'ui', 'canvas', ...), or 'all' (default)"
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
        // Bug Reporting Tools
        // ====================================================================
        Tool {
            name: "report_error".to_string(),
            description: "Draft a GitHub issue for a bug in the Clean Language toolchain. Nothing is sent anywhere: the result contains the issue title, body and the exact `gh issue create` command for the repository that owns the fix (label `compiler-old`). Show the command to the user and let them run it. When the MCP server runs inside the owning component's source tree, the bug is also recorded in the local dev queue (`cln dev-queue list`). BEFORE calling this tool, synthesize a minimal reproduction and root-cause analysis. minimal_repro must be AI-generated code that isolates the bug, not copied from the user's file.".to_string(),
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
                        "description": "The component that contains the buggy code (decides the GitHub repository).",
                        "enum": REPORT_COMPONENTS
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
                        "description": "REQUIRED: minimal reproduction case as a compilable Clean Language code fragment or complete .cln file that surfaces the bug. Write the smallest possible start: block (or function) that triggers the same failure. Do NOT copy from the user's file."
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
                        "description": "Root-cause analysis: which pipeline stage fails, why, and what the likely fix is."
                    },
                    "friendly_summary": {
                        "type": "string",
                        "description": "Optional: one plain-English sentence (max 300 chars) describing what the user tried to do and what went wrong."
                    },
                    "suggested_fix": {
                        "type": "string",
                        "description": "Specific fix location and change (e.g. 'src/codegen/mod.rs line 847: string comparison is inverted'). Include when you can identify the exact line."
                    },
                    "suggested_component_file": {
                        "type": "string",
                        "description": "Source file most likely to contain the fix (e.g. 'src/parser/token_parser.rs')."
                    },
                    "discovered_during": {
                        "type": "string",
                        "description": "Optional: the task or context in which this error was found (e.g., 'doc_coverage run'). Does not affect routing."
                    },
                    "affected_component": {
                        "type": "string",
                        "description": "Optional: the specific component where the bug was observed, when different from `component`.",
                        "enum": REPORT_COMPONENTS
                    },
                    "affected_version": {
                        "type": "string",
                        "description": "Optional: version of the affected component where the bug was observed (e.g., '2.6.1' for frame.data 2.6.1). The compiler version is added automatically."
                    }
                }),
                required: vec![
                    "error_code".to_string(),
                    "error_message".to_string(),
                    "component".to_string(),
                    "severity".to_string(),
                    "minimal_repro".to_string(),
                ],
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
            description: "Draft a GitHub issue from a local server diagnostic (./diagnostics/pending/<sha>). Nothing is sent anywhere: the result contains the issue title, body and the exact `gh issue create` command; show it to the user. The diagnostic stays in pending/ until the user files the issue and marks it with `clean-server errors publish <sha>`.".to_string(),
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
                    }
                }),
                required: vec!["sha".to_string()],
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
            description: "Get recent changes to the Clean Language compiler: its last N commits, embedded when this compiler was built.".to_string(),
            input_schema: ToolInputSchema {
                type_: "object".to_string(),
                properties: json!({
                    "count": {
                        "type": "number",
                        "description": "Number of recent commits to return (max 50). Default: 10."
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
                        "description": "Path to a feature spec .md file inside docs/features"
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

    // `arguments` is optional in MCP: a tool without required parameters
    // may be called without it.
    let no_arguments = json!({});
    let arguments = match params.get("arguments") {
        None | Some(serde_json::Value::Null) => &no_arguments,
        Some(args) if args.is_object() => args,
        Some(_) => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Tool arguments must be an object".to_string(),
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
        "get_app_structure" => tool_get_app_structure(id.clone()),
        "get_plugin_examples" => tool_get_plugin_examples(id.clone(), arguments),
        "list_ecosystem" => tool_list_ecosystem(id.clone(), arguments),
        "get_stack_recommendation" => tool_get_stack_recommendation(id.clone(), arguments),
        "report_error" => tool_report_error(id.clone(), arguments),
        "list_server_diagnostics" => tool_list_server_diagnostics(id.clone(), arguments),
        "show_server_diagnostic" => tool_show_server_diagnostic(id.clone(), arguments),
        "publish_diagnostic" => tool_publish_diagnostic(id.clone(), arguments),
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
    // Per MCP spec, tools/call results must use this format.
    // If the result already has a proper MCP content array (type+text items), pass it through
    // without re-serialising — re-wrapping embeds the JSON as a string in `text`, which causes
    // AI clients to do naive string extraction and pick up trailing JSON characters like `"}`.
    if let Some(result) = response.result {
        let already_formatted = result
            .get("content")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("type"))
            .is_some();
        if already_formatted {
            return JsonRpcResponse::success(response.id, result);
        }
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

    let lint_warnings = lint_endpoint_organization(source, file_path);

    match type_check_with_external_plugins(source, file_path) {
        Ok(result) => {
            let mut diagnostics: Vec<serde_json::Value> = result
                .diagnostics
                .iter()
                .map(|w| diagnostic_to_json(w, "warning"))
                .collect();
            diagnostics.extend(lint_warnings);
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
            let mut diagnostics: Vec<serde_json::Value> =
                errors.iter().map(error_to_json).collect();
            diagnostics.extend(lint_warnings);
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

    match compile_source_with_detected_plugins(source, file_path, opt_level) {
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

    let lint_warnings = lint_endpoint_organization(source, file_path);

    let (success, mut all_diagnostics): (bool, Vec<serde_json::Value>) =
        match type_check_with_external_plugins(source, file_path) {
            Ok(result) => (
                true,
                result
                    .diagnostics
                    .iter()
                    .map(|w| diagnostic_to_json(w, "warning"))
                    .collect(),
            ),
            Err(errors) => (false, errors.iter().map(error_to_json).collect()),
        };
    all_diagnostics.extend(lint_warnings);

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
            "success": success && error_count == 0,
            "diagnostics": diagnostics,
            "error_count": error_count,
            "warning_count": warning_count
        }),
    )
}

/// JSON form of one error-catalog entry.
fn error_code_json(entry: &crate::error::catalog::ErrorCodeInfo) -> serde_json::Value {
    json!({
        "code": entry.code,
        "name": entry.name,
        "category": entry.category,
        "severity": entry.severity,
        "summary": entry.summary,
        "title": entry.title,
        "details": entry.details,
    })
}

/// Tool: explain_error - Explain an error code from the error-code registry
fn tool_explain_error(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let code = match args.get("code").and_then(|v| v.as_str()) {
        Some(c) => c.trim().to_uppercase(),
        None => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing 'code' parameter".to_string(),
            )
        }
    };

    let Some(entry) = crate::error::catalog::lookup(&code) else {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "code": code,
                "error": "This error code is not in the error-code registry. Use `list_error_codes` to see all codes.",
            }),
        );
    };

    let mut result = error_code_json(entry);
    result["success"] = json!(true);
    result["spec_guidance"] = json!("If your code follows the specification exactly and still produces this error, it may be a compiler bug. Call `report_error` to draft a GitHub issue for it. Do NOT work around the error by using alternative syntax, underscore bridge names, or non-spec patterns.");
    JsonRpcResponse::success(id, result)
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

/// Tool: list_ecosystem - List the plugins installed for this toolchain
///
/// Built at call time from the installed plugin manifests
/// (`~/.cleen/plugins/*/plugin.toml`, read by `PluginDiscovery`), so names,
/// versions, blocks and bridge functions always match what the compiler
/// loads.
fn tool_list_ecosystem(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let category_filter = args
        .get("category")
        .and_then(|v| v.as_str())
        .filter(|c| !c.eq_ignore_ascii_case("all"));

    let discovery = PluginDiscovery::new();
    let installed = match discovery.discover_all() {
        Ok(found) => found,
        Err(e) => {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": format!("Failed to read the installed plugin manifests: {}", e),
                }),
            )
        }
    };

    if installed.is_empty() {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": true,
                "ecosystem": [],
                "total": 0,
                "note": "No plugins are installed. Install the Frame plugins with `cleen frame install`, or one plugin with `cleen plugin install <name>` (e.g. frame.server), then call list_ecosystem again."
            }),
        );
    }

    let mut names: Vec<&String> = installed.keys().collect();
    names.sort();
    let plugins: Vec<serde_json::Value> = names
        .into_iter()
        .filter_map(|name| {
            let manifest = &installed[name];
            // `frame.server` -> category `server`
            let category = name.rsplit('.').next().unwrap_or(name);
            if let Some(cat) = category_filter {
                if !category.eq_ignore_ascii_case(cat) {
                    return None;
                }
            }
            let bridge_functions: Vec<&str> = manifest
                .bridge
                .functions
                .iter()
                .map(|f| f.name.as_str())
                .collect();
            Some(json!({
                "name": name,
                "version": manifest.plugin.version,
                "category": category,
                "description": manifest
                    .ai
                    .description
                    .as_deref()
                    .unwrap_or(&manifest.plugin.description),
                "blocks": manifest.handles.blocks,
                "bridge_function_count": bridge_functions.len(),
                "bridge_functions": bridge_functions,
                "owned_paths": manifest.paths.owns,
                "installed": true,
            }))
        })
        .collect();

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "total": plugins.len(),
            "ecosystem": plugins,
            "note": "Use 'list_plugins' with project_dir to see installed plugins with full DSL details. Use 'get_plugin_examples' to read example code."
        }),
    )
}

/// Tool: get_specification - Query the Clean Language Specification
fn tool_get_specification(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let section_query = args.get("section").and_then(|v| v.as_str());
    let search_query = args.get("query").and_then(|v| v.as_str());
    let sections = spec_sections();

    // If no parameters, return table of contents
    if section_query.is_none() && search_query.is_none() {
        let toc: Vec<serde_json::Value> = sections
            .iter()
            .map(|s| json!({ "title": s.title, "file": s.file }))
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

    let mut results: Vec<&SpecSection> = sections.iter().collect();

    if let Some(section) = section_query {
        let section_lower = section.to_lowercase();
        results.retain(|s| s.title.to_lowercase().contains(&section_lower));
    }

    if let Some(query) = search_query {
        let query_lower = query.to_lowercase();
        results.retain(|s| s.content.to_lowercase().contains(&query_lower));
    }

    let result_json: Vec<serde_json::Value> = results
        .iter()
        .map(|s| json!({ "title": s.title, "file": s.file, "content": s.content }))
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

/// Tool: list_builtins - List all built-in functions, classes, and namespaces.
///
/// Source of truth: a freshly-constructed `SymbolTable`. The compiler's
/// resolver uses the same table when type-checking calls, so MCP introspection
/// stays in lockstep with what actually compiles. Replaced the parallel
/// `BuiltinRegistry` hardcoded list in 0.30.289 (BUILTIN-NAMESPACE-OVERREACH
/// sub-finding D-MCP-1).
///
/// Accepted regressions vs. the old `BuiltinRegistry`:
///   * No `BuiltinCategory` filter (Math/String/List/…). The `category` field
///     in arguments is now ignored. The IDE rarely filters on this.
///   * No `is_static` distinction on class methods. SymbolTable's
///     `SymbolKind::Method` does not track static-vs-instance; the field is
///     omitted from the response.
fn tool_list_builtins(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let namespace_filter = args.get("namespace").and_then(|v| v.as_str());

    let table = GlobalSymbolTable::new();
    let all_symbols = table.all_symbols();

    // Resolve a SymbolId to (name, params_json, return_type_json) when the
    // symbol is a Function or Method.
    let function_json =
        |sym_id: &crate::resolver::SymbolId| -> Option<(String, Vec<String>, String)> {
            let sym = all_symbols.get(sym_id)?;
            match &sym.kind {
                SymbolKind::Function {
                    parameters,
                    return_type,
                } => {
                    let params = parameters
                        .iter()
                        .map(|p| format!("{p:?}"))
                        .collect::<Vec<_>>();
                    let ret = return_type
                        .as_ref()
                        .map(|t| format!("{t:?}"))
                        .unwrap_or_else(|| "Void".to_string());
                    Some((sym.name.clone(), params, ret))
                }
                SymbolKind::Method {
                    parameters,
                    return_type,
                    ..
                } => {
                    let params = parameters
                        .iter()
                        .map(|p| format!("{p:?}"))
                        .collect::<Vec<_>>();
                    let ret = format!("{return_type:?}");
                    Some((sym.name.clone(), params, ret))
                }
                _ => None,
            }
        };

    let mut global_functions: Vec<serde_json::Value> = Vec::new();
    let mut classes_json: Vec<serde_json::Value> = Vec::new();
    let mut namespaces_json: Vec<serde_json::Value> = Vec::new();

    for (sid, symbol) in all_symbols.iter() {
        if !table.is_builtin(*sid) {
            continue;
        }
        match &symbol.kind {
            SymbolKind::Function {
                parameters,
                return_type,
            } => {
                // Filter out namespace-qualified names (e.g. "math.sin") —
                // those are surfaced under the namespace section below to
                // avoid double-listing.
                if symbol.name.contains('.') {
                    continue;
                }
                let params: Vec<String> = parameters.iter().map(|p| format!("{p:?}")).collect();
                let ret = return_type
                    .as_ref()
                    .map(|t| format!("{t:?}"))
                    .unwrap_or_else(|| "Void".to_string());
                global_functions.push(json!({
                    "name": symbol.name,
                    "parameters": params,
                    "return_type": ret,
                }));
            }
            SymbolKind::Class { methods, .. } => {
                let mut methods_arr: Vec<serde_json::Value> = Vec::new();
                for m_id in methods {
                    if let Some((m_name, m_params, m_ret)) = function_json(m_id) {
                        methods_arr.push(json!({
                            "name": m_name,
                            "parameters": m_params,
                            "return_type": m_ret,
                        }));
                    }
                }
                classes_json.push(json!({
                    "name": symbol.name,
                    "methods": methods_arr,
                }));
            }
            SymbolKind::Namespace { functions } => {
                if let Some(filter) = namespace_filter {
                    if !symbol.name.eq_ignore_ascii_case(filter) {
                        continue;
                    }
                }
                let mut funcs_arr: Vec<serde_json::Value> = Vec::new();
                for f_id in functions {
                    if let Some((f_name, f_params, f_ret)) = function_json(f_id) {
                        funcs_arr.push(json!({
                            "name": f_name,
                            "parameters": f_params,
                            "return_type": f_ret,
                        }));
                    }
                }
                namespaces_json.push(json!({
                    "name": symbol.name,
                    "functions": funcs_arr,
                }));
            }
            _ => {}
        }
    }

    let total_methods: usize = classes_json
        .iter()
        .filter_map(|c| c.get("methods").and_then(|m| m.as_array()).map(Vec::len))
        .sum();
    let summary = json!({
        "global_functions": global_functions.len(),
        "classes": classes_json.len(),
        "namespaces": namespaces_json.len(),
        "total_methods": total_methods,
    });

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "summary": summary,
            "global_functions": global_functions,
            "namespaces": namespaces_json,
            "classes": classes_json,
        }),
    )
}

/// Tool: list_error_codes - List all compiler error codes
fn tool_list_error_codes(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let category_filter = args.get("category").and_then(|v| v.as_str());
    let catalog = crate::error::catalog::all();

    let filtered_codes: Vec<serde_json::Value> = catalog
        .iter()
        .filter(|entry| category_filter.is_none_or(|cat| entry.category.eq_ignore_ascii_case(cat)))
        .map(error_code_json)
        .collect();

    // Count by category
    let mut category_counts: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for entry in catalog {
        *category_counts.entry(entry.category.as_str()).or_insert(0) += 1;
    }

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "summary": {
                "total_codes": catalog.len(),
                "by_category": category_counts
            },
            "count": filtered_codes.len(),
            "error_codes": filtered_codes,
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

## String Interpolation
Regular double-quoted string literals support `{expr}` interpolation — the expression is evaluated, coerced to string via `.toString()`, and substituted in place. Use `\{` and `\}` to include literal braces.
```
string name = "world"
string greeting = "hello {name}"          // "hello world"
integer n = 42
string msg = "answer: {n}"                // "answer: 42"
string method = "len: {name.length()}"    // "len: 5" — any logical expression works
string literal = "\{not a var\}"          // "{not a var}" — escaped braces
string jsonLike = "{\"a\":1}"             // "{\"a\":1}" — `{"` doesn't start an ident, so no interpolation
```
If you write `"{ident}"` where `ident` is not a variable in scope, you get error `SEM007: Undefined variable 'ident'` — that's interpolation at work, not a bug. Use `\{ident\}` for a literal.

The same `{var}` and `{!var}` (raw HTML) syntax is used inside `html:` blocks — see §"HTML Generation".

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
print("text")              // print with newline (default)
printl("text")             // alias for print(x) — also prints with newline
print("value: " + x.toString())  // string concatenation
print("no newline here") + // trailing '+' means CONTINUATION: stays on same line (rare)
```
The trailing `+` continuation form is the opposite of what older code assumed: `print(x)` adds the newline; `print(x) +` suppresses it. Semantics flipped 2026-05-13 (compiler commit 6700d4b9). Prefer plain `print(x)` for line-per-value output.

## Lists
```
list<integer> nums = [1, 2, 3, 4, 5]
integer first = nums[0]
integer len = nums.length()
nums.add(6)                // add to end
nums.remove(0)             // remove at index
boolean has = nums.contains(3)
list<integer> sorted = nums.sort()

// List behaviors — declared as a TYPE SUFFIX on the list type
list<string>.line queue = []      // FIFO queue: add to back, remove from front
list<string>.pile stack = []      // LIFO stack: add/remove from top
list<string>.unique names = []    // Set: no duplicates allowed

// Combined behaviors (canonical order: .line then .unique then .pile)
list<integer>.line.unique uniqueQueue = []
```

## Classes
Class fields and methods are **private by default**. To expose members to outside callers, group them inside a single `public:` sub-section. There is no per-member visibility modifier — the absence of `public:` is what makes a member private.
```
class Person
	integer age              // private — internal only

	public:
		string name          // public — readable as p.name

	constructor(string nameParam, integer ageParam)
		name = nameParam
		age = ageParam

	functions:
		public:
			string greet()
				return "I'm " + name

start:
	Person p = Person("Alice", 30)
	print(p.greet())     // OK — greet is in public: methods block
	print(p.name)        // OK — name is in public: fields block
	// print(p.age)      // ERROR (SEM005) — age is private
```

## Visibility model
- **Classes:** Fields and methods are private by default. A `public:` field block exposes fields; a `public:` method block inside `functions:` exposes methods. At most one of each per class.
- **Modules:** Top-level functions are private to their file by default. A `public:` sub-section inside `functions:` exposes them to importing modules.
- **State:** `state:` declarations are private to their file by default. A `public:` sub-section inside `state:` exposes them to importing modules.
- **Inheritance:** Subclasses inherit only public members of their parent. Overrides of a parent's public method must themselves live inside the child's `public:` method block.
- **Error code:** SEM005 — Access Violation.

## Inheritance
```
class Animal
	public:
		string name

	constructor(string nameParam)
		name = nameParam

	functions:
		public:
			string speak()
				return name + " speaks"

class Dog is Animal
	public:
		string breed

	constructor(string nameParam, string breedParam)
		base(nameParam)
		breed = breedParam

	functions:
		public:
			string speak()
				return name + " barks"
```

## Capabilities (can)
Capabilities are named method contracts that unrelated classes can claim. Use `can` when the relationship is "can-do" rather than "is-a". A class must explicitly declare `can Name` — nominal, not structural.

**Naming convention: bare verbs.** `can Draw`, `can Print`, `can Close`, `can Serialize`, `can Compare`. Adjective forms like `Drawable` read awkwardly against the `can` keyword. Every capability in the standard library and every example in this reference follows the verb convention.

```
can Draw:
	draw()

can Describe:
	string describe()
	string tag()
		return "[" + this.describe() + "]"   // default body — classes that don't override inherit this

class Circle is Shape can Draw, Describe
	public:
		number radius

	functions:
		public:
			draw()
				print("circle")
			string describe()
				return "circle"
			// tag() not written — Circle inherits Describe's default

class Square can Draw
	functions:
		public:
			draw()
				print("square")
```

Capability values are usable as parameter/return/variable types. Runtime dispatch is dynamic — the actual class of the value at runtime determines which method runs.

```
functions:
	render(Draw shape)
		shape.draw()                          // dispatches via runtime class-id

start:
	render(Circle())                          // prints "circle"
	render(Square())                          // prints "square"
```

Header order when both are present: `class Name [is Parent] [can C1, C2, ...]`.

**Compile-time enforcement:**
- `SEM011` — class claims a capability but doesn't implement one of its required (non-default) methods.
- `SEM012` — class claims a capability that was never declared.
- `SEM013` — class implements a capability method with the wrong signature.
- `SEM014` — a capability default body calls a method that isn't part of the same capability.

## Event Handlers (events: class section)
Classes can declare an `events:` block sibling to `functions:` for UI event handlers (frame.ui components, etc.). Event-handler methods use a distinct signature: bare `name(params):` form — NO return type, trailing colon required. Parameters use `name [: type]` form.

The compiler tags each `events:` method with `FunctionModifier::EventHandler`. After expansion, the compiler emits a bare-named top-level dispatch function per event method (when the class has a matching `state:` block instance), so the browser loader's `instance.exports[handlerName]()` lookup succeeds.

```
class FormatToolbar
	events:
		onMount():
			printl("mounted")
		fmt_bold():
			printl("bold clicked")
		fmt_italic():
			printl("italic clicked")

state:
	FormatToolbar instance = FormatToolbar()

start:
	// Direct method call works as normal:
	instance.onMount()
	// Bare-name shim works too (used by the browser loader):
	fmt_bold()
	fmt_italic()
```

## Environment Variables
Use `env.get("VAR_NAME")` — the single canonical form for reading environment variables. Do NOT use `$VAR` or `env("VAR")` shortcuts; they were removed in 2026-06-15.
```
string dbHost = env.get("DB_HOST")
string apiKey = env.get("API_KEY") default ""
```
This form is also accepted inside plugin config blocks (`frame.auth` JWT secret, `frame.server` mail values, etc.).

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

### Expression tests (unit / pure-function)
```
tests:
	"adds numbers": add(2, 3) = 5
	"squares a number": square(4) = 16
	"hi".toUpperCase() = "HI"
	math.abs(-42) = 42
```

### Endpoint tests (HTTP — requires live server via `cln serve <file.cln>`)
Each `test "name"` block declares one HTTP test case.
First line: METHOD "path" with optional `json(...)` body and `header(...)`.
Subsequent lines: assertions against the response.

```
tests:
	test "health check"
		GET "/api/health"
		status = 200

	test "creates user"
		POST "/api/users" json(name: "Alice", email: "alice@test.com")
		status = 201
		json.name = "Alice"
		json.id != null

	test "rejects missing email"
		POST "/api/users" json(name: "Alice")
		status = 400
		json.error != null

	test "with auth header"
		GET "/api/profile" header("Authorization": "Bearer abc123")
		status = 200
		json.email != null

	test "updates user"
		PUT "/api/users/1" json(name: "Bob")
		status = 200
		json.name = "Bob"

	test "deletes user"
		DELETE "/api/users/1"
		status = 204

	test "query params"
		GET "/api/users?active=true"
		status = 200
		json.length > 0
```

**HTTP methods**: `GET` `POST` `PUT` `DELETE` `PATCH` (contextual — not global reserved words)
**Assertion operators**: `=` `!=` `<` `>` `<=` `>=`
**Special assertion**: `json.field != null` — field must be present and non-null
**Body syntax**: `json(key: value, key2: value2)` — inline JSON object
**Header syntax**: `header("Key": "Value")` — single header per request

## Running Tests

### Expression tests (no server needed)
```
cln test file.cln              # run all tests: blocks in one file
cln test src/                  # run all .cln files in a directory
```
Output:
```
PASS: addition works
PASS: string length
FAIL: divide by zero
Tests: 2 passed, 1 failed
```

### Endpoint tests (requires live server)
```
cln serve app/api/main.cln     # start the dev server in one terminal
cln test file.cln              # run endpoint tests against it
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
- `list_ecosystem` — Installed plugins with their blocks and bridge functions
- `get_plugin_examples` — Read plugin example files
- `get_stack_recommendation` — Recommended plugin stack for a project type
- `report_error` — Draft a GitHub issue (and its `gh issue create` command) for a likely compiler bug
- `publish_diagnostic` — Draft a GitHub issue from a local server diagnostic
- `list_server_diagnostics` — List local runtime errors captured by clean-server

## Workflow
1. Call `get_quick_reference` (this tool) to learn base syntax
2. Call `get_stack_recommendation` with your project type (web-app, api, game, cli) — includes best practices for web apps
3. Call `list_ecosystem` to see the installed plugins
4. Call `list_plugins` to see installed plugins with full DSL details
5. Call `get_plugin_examples` to see plugin usage patterns
6. Write .cln code following the patterns above — spec-correct code ONLY
7. Call `check` to type-check (fast feedback loop)
8. Call `compile` when ready for WASM output
9. If errors occur, call `explain_error` with the code
10. If the code follows the spec but doesn't compile, call `report_error` immediately — do NOT write workaround code — and give the user the `gh issue create` command it returns
11. Use `get_specification` for detailed docs on specific features
12. Call `list_server_diagnostics` to check for runtime errors — if unpublished diagnostics exist, ask the user whether to report them
13. If user agrees, call `publish_diagnostic` with the SHA and give the user the returned `gh issue create` command

## Bug Lifecycle (for Clean Language development team)
Bugs are GitHub issues labeled `compiler-old` in the repository that owns the fix:
1. List open bugs: `gh issue list --repo Ivan-Pasco/clean-language-compiler-old --label compiler-old`
2. Pick the highest-priority bug and write a failing test
3. Fix the root cause in the source code
4. Commit with `fixes #N` in the message; GitHub closes the issue when the commit reaches the default branch
5. Run `comita` to tag, push and wait for CI

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

### Rule A — Entity + Database service first, raw SQL only when necessary (frame.data v2)

Under frame.data v2 there are exactly three places to touch persisted data:

1. **`Entity.data.<method>(args)`** — for reads. `<method>` is declared in the paired data block's `queries:` sub-block and returns `Entity`, `Entity?`, or `list<Entity>`.
2. **`Database.save(entity)` / `Database.delete(entity)` / `Database.deleteOrFail(entity)` / `Database.saveAll(list)` / `Database.deleteAll(list)`** — for writes.
3. **`db.query(sql, params)` / `db.queryAs(Type): sql: ... params: ...`** — raw SQL escape hatch, used inside report classes in `app/data/reports/` when the DSL can't express the query.

**Removed under v2 (compile-time error — E FRAME-DATA-E021):** `Model.insert:`, `Model.insert_id:`, `Model.update:`, `Model.upsert:`, `Model.delete:` block forms. `db.update:`, `db.insert:`, `db.delete:` block forms are also removed.

**Migration recipes (v1 → v2):**

```
// WRONG (v1) — Model.insert: block form
User.insert:
    email = "a@b.com"
    status = "pending"

// CORRECT (v2) — construct entity, call Database.save
User u = User(email: "a@b.com", status: "pending")
Database.save(u)
// After save, u.id is mutated in place to the DB-generated primary key.

// WRONG (v1) — Model.update: block form
User.update:
    set status = "active"
    where id == userId

// CORRECT (v2) — load, mutate, save
User u = User.data.findOrFailById(userId)
u.status = "active"
Database.save(u)

// WRONG (v1) — Model.delete: block form
User.delete:
    where id == userId

// CORRECT (v2) — load, delete
User u = User.data.findOrFailById(userId)
Database.delete(u)       // lenient — no error if already gone
// or Database.deleteOrFail(u) if the row MUST have existed (raises NOT_FOUND)

// CORRECT (v2) — raw SQL still the escape hatch for aggregates / joins that
// the DSL can't express. Lives in a report class under app/data/reports/.
class MonthlySignups
    functions:
        public:
            list<map<string, any>> byMonth()
                return db.query:
                    sql: "SELECT DATE_FORMAT(created_at, '%Y-%m') as month, COUNT(*) AS cnt FROM users GROUP BY month"
                    params: []
```

**Reads use the `.data` accessor** — `Entity.data.<method>` resolves at compile time to a `queries:` method on the paired data block. It cannot be captured (`var q = User.data` is a compile-time error per DAT-A001).

See specs `foundation/spec/plugins/frame-data-semantics.md` DAT-S001..S010 (`Database`), DAT-A001..A005 (`.data`), DAT-Q014/DAT-Q015 (retired mutation blocks).

### Rule B — Compose functions, never duplicate queries

Never repeat a SQL query that another function in the same file already performs. If you need decorated or filtered data from an existing query, call the existing function and transform the result. A second DB round-trip for data you already fetched is a bug.

```
// WRONG — duplicates the query from get_user()
string get_user_display_name(integer id)
    string sql = "SELECT first_name, last_name FROM users WHERE id = ?"
    string row = db.query(sql, id.toString())
    return json.get(row, "first_name") + " " + json.get(row, "last_name")

// CORRECT — reuse the existing function, transform the result
string get_user_display_name(integer id)
    string row = get_user(id)
    return json.get(row, "first_name") + " " + json.get(row, "last_name")
```

### Rule C — One resource per endpoints file (frame.server)

Do NOT create a single mega `api.cln` containing every endpoint in the app. Split `endpoints:` blocks by resource/domain so each file owns one noun.

**Thresholds — split a file when ANY is true:**
- More than **6 endpoint declarations** (`GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `FEED`, `LIVE`) in one `endpoints:` block
- File exceeds **150 lines** (excluding blank lines and comments)
- The file mixes **two or more unrelated resources** (e.g. `/users/*` and `/orders/*` in the same `endpoints:` block)

**Grouping rule:** group by resource **noun**, not by HTTP verb. All `/users/*` routes (GET, POST, PUT, DELETE) belong in the same file regardless of method. Never split by verb (no `users_get.cln`, `users_post.cln`).

**Directory layout:**

    app/server/api/
        users.cln              — GET/POST/PUT/DELETE /users, /users/:id
        orders.cln             — all /orders/* routes
        auth.cln               — login, logout, register
        _middleware.cln        — shared auth guards, request logging (underscore prefix = not a route file)
        _types.cln             — shared request/response shapes
        admin/
            users.cln          — admin-only /admin/users/* routes
            metrics.cln        — /admin/metrics/* routes

**Nested resources** get a subfolder when the parent resource itself has its own endpoints AND has child resources with their own endpoints. Otherwise keep child routes in the parent file.

```
// WRONG — one mega file mixing unrelated resources
// app/server/api.cln
endpoints:
    GET  "/users"              -> list_users()
    POST "/users"              -> create_user()
    GET  "/orders"             -> list_orders()
    POST "/orders"             -> create_order()
    POST "/auth/login"         -> login()
    POST "/auth/logout"        -> logout()
    GET  "/admin/metrics"      -> metrics()

// CORRECT — one file per resource
// app/server/api/users.cln
endpoints:
    GET  "/users"              -> list_users()
    POST "/users"              -> create_user()
    GET  "/users/:id"          -> get_user()

// app/server/api/orders.cln
endpoints:
    GET  "/orders"             -> list_orders()
    POST "/orders"             -> create_order()

// app/server/api/auth.cln
endpoints:
    POST "/auth/login"         -> login()
    POST "/auth/logout"        -> logout()
```

**Cross-cutting concerns** (auth middleware, shared error handlers, shared request/response shapes) live in their own files prefixed with `_` so they sort to the top and signal "not a route file": `_middleware.cln`, `_types.cln`, `_errors.cln`.

**Why split?** Long endpoint files (1) make code review and diff noise unmanageable, (2) force unrelated teams to merge into the same file, (3) hide which resources the API actually exposes, and (4) prevent route-level loading optimisations once the framework supports them.

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
    integer count = json.get(db.query(count_sql, params), "cnt").toInteger()

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

Pages live in app/web/pages/ — HTML templates with optional .cln companion adapters:

    app/web/pages/
        home.cln         — companion adapter (load() returns context data, optional guard())
        about.html       — HTML template ({var} escaped, {!var} raw HTML)
        about.cln        — companion adapter for about.html
        blog/
            [slug].html  — dynamic route (/blog/:slug)
            [slug].cln   — companion adapter for dynamic page

Companion .cln example (delegates data-fetching, never queries DB directly):
    // app/web/pages/blog/[slug].cln
    functions:
        any load(Request request)
            string slug = request.params.slug
            Post post = posts.getBySlug(slug)
            return { post: post }

HTML template example:
    <!-- app/web/pages/blog/[slug].html -->
    <page layout="main"></page>
    <article>
        <h1>{post.title}</h1>
        <p>{post.content}</p>
    </article>

Note: app/server/api/ endpoints that return full HTML pages still use html: blocks
(endpoint-level SSR). Page companions use return { key: value } — the template handles HTML.

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
            "tip": "Use 'check' for fast iteration, 'compile' when ready for WASM. If spec-correct code fails, call 'report_error' immediately — never write workarounds — and give the user the GitHub issue command it returns. Call 'list_server_diagnostics' to find local runtime errors."
        }),
    )
}

/// Tool: get_app_structure - Canonical Clean Language application architecture
fn tool_get_app_structure(id: serde_json::Value) -> JsonRpcResponse {
    let architecture = r##"# Clean Language Application Architecture (Canonical)

**MANDATORY for all application projects.** Apply BEFORE creating any .cln file.
Does NOT apply to plugins, library packages, or single-file scripts under 200 LOC.

## 1. Folder Layout

```
my-app/
├── main.cln                    REQUIRED — declares shared: and target: blocks
│                               (also holds the frame.data: connection config
│                                under v2 — no more app/data/config.cln)
├── public/{css,images}/        Static assets
└── app/
    ├── auth/                   frame.auth configuration and guards
    ├── types/                  Validated value types (Email, Money, Slug)
    ├── entity/                 Entity classes — the DOMAIN shape
    │                           (fields + invariants + methods; NO persistence)
    ├── data/
    │   ├── models/             data <T>: blocks — the PERSISTENCE view of an
    │   │                         entity T. Sub-block form (table, fields:,
    │   │                         indexes:, relations:, queries:). One per file.
    │   ├── reports/            Report classes — cross-entity or aggregate
    │   │                         queries using db.query / db.queryAs
    │   └── migrations/         Versioned schema migration files
    ├── logic/                  Business logic — ONE class per capability
    ├── server/
    │   ├── api/                HTTP endpoints (thin, call ONE logic method)
    │   └── middleware/         Request filters
    └── web/
        ├── layouts/            Page layout wrappers
        ├── pages/              Page templates + companion .cln adapters
        └── components/         Reusable UI components
```

No other folders inside `app/`. FORBIDDEN filenames anywhere in `app/`:
`utils.cln`, `helpers.cln`, `common.cln`, `misc.cln`, `lib.cln`, `shared.cln`,
`tools.cln`, `core.cln`. These always degrade into junk drawers.

**Entity ↔ data pairing (v2, DAT-P001..P005).** An entity class `User` lives in
`app/entity/user.cln`. Its persistence view — table name, indexes, relations,
queries — lives in `app/data/models/user.cln` as a `data User:` block. The two
files must share a basename. An entity may exist without a data block (session
shapes, request DTOs), but every data block requires a paired entity.

## 2. The Two Laws (non-negotiable)

### Law 1 — Dependency direction is one-way

```
api, pages, components, middleware  →  logic  →  data/models, data/reports
                                          ↓         ↓
                                        entity     entity
                                          ↓
                                        types
```

- `types/` imports nothing from the app
- `entity/` imports only from `types/` (entities are persistence-ignorant, DAT-E003)
- `data/models/` and `data/reports/` import from `entity/` and `types/`
- `logic/` imports from `entity/`, `data/models/`, `data/reports/`, and `types/`
- `server/api/`, `server/middleware/`, `web/pages/`, `web/components/` import
  from `logic/`, `entity/`, and `types/`
- A reverse import is a structural error

### Law 2 — One class per logic file, called statically

- Every `app/logic/<name>.cln` defines EXACTLY ONE `class <Name>`
- Methods are invoked as `ClassName.method(args)` — NO instantiation
  (e.g., `Auth.register(email, password)`, NOT `Auth a = Auth(); a.register(...)`)
- The class name MUST be a capability, NEVER an entity
- Logic classes have NO fields and NO constructor — they are stateless namespaces

## 3. Decision Tree — Where Does New Code Go?

Apply in order. First match wins.

1. Validates or represents a primitive value (Email, Money, Slug)?
   → `app/types/<name>.cln`
2. Represents a domain object — fields, invariants, business methods
   (User, Order, Product; whether persistent or not)?
   → `class T` in `app/entity/<basename>.cln`
3. Persistence view of an entity — table, indexes, relations, entity-scoped
   queries returning `T` / `T?` / `list<T>`?
   → `data T:` block in `app/data/models/<basename>.cln` (basename matches
     the entity file per DAT-M014)
4. Cross-entity, aggregate, or raw-SQL query (returns a report shape, not
   an entity type)?
   → Report class in `app/data/reports/<capability>.cln` — uses `db.query`
     or `db.queryAs(SomeType)` (see DAT-R001..R003)
5. ANY behavior that orchestrates entities + persistence + I/O — workflow,
   validation, computation across multiple entities?
   → method on a class in `app/logic/<capability>.cln` — calls
     `Entity.data.<method>` for reads and `Database.save` / `Database.delete`
     for writes
6. HTTP route?
   → `app/server/api/<area>.cln` — body calls EXACTLY ONE logic method
7. Request filter (auth check, logging, CORS)?
   → `app/server/middleware/<name>.cln`
8. Page template or companion adapter?
   → `app/web/pages/<route>.{html,cln}`
9. Reusable UI component?
   → `app/web/components/<name>.cln`
10. Page layout wrapper?
    → `app/web/layouts/<name>.html`
11. Auth configuration?
    → `app/auth/`
12. Schema migration?
    → `app/data/migrations/`
13. Database connection configuration (engine, host, credentials, pool)?
    → `frame.data:` block in `main.cln` (v2: NOT `app/data/config.cln`;
      per DAT-I004)

If nothing matches, the code does not belong in the app.

## 4. Naming Rules

| Element | Rule | OK | Forbidden |
|---|---|---|---|
| Value type | Singular PascalCase | `Email`, `Money`, `Slug` | `UserEmail`, `PriceValue` |
| Model | Singular PascalCase | `User`, `Order`, `Product` | `Users`, `UserData`, `UserRecord` |
| Logic class | Capability — never entity | `Auth`, `Checkout`, `Mailer`, `Billing`, `Search`, `Profile`, `Onboarding` | `User`, `UserService`, `OrderManager`, `ProductHelper`, `UserUtils`, `UserRepository` |
| Filename | snake_case, matches primary export | `user.cln` → `User`; `auth.cln` → `Auth`; `email.cln` → `Email` | `User.cln`, `auth_service.cln`, `Email.cln` |

## 5. Reserved Lowercase Identifiers (do NOT use as variable names)

These names are taken by frame plugin bridge namespaces:
`auth`, `db`, `http`, `req`, `res`, `request`, `response`, `string`, `math`,
`list`, `json`, `ui`, `data`, `state`.

The static-call pattern (`Auth.register(...)`) avoids the issue entirely.
If you ever need an instance, pick a name the plugins don't own:
`authentication`, `database`, `httpClient`, `httpRequest`, `appState`.

## 6. Forbidden Patterns (will become structural errors)

| Code | Violation |
|---|---|
| `E-STRUCT-001` | Junk-drawer filename (`utils.cln`, `helpers.cln`, etc.) |
| `E-STRUCT-002` | Entity-named logic class or file (`UserService`, `OrderManager`) |
| `E-STRUCT-003` | `functions:` block inside a `data:` block (v2: business methods go on the entity, not the data block; queries go in `queries:` sub-block) |
| `E-STRUCT-004` | Database call from `app/server/api/`, `app/web/components/`, or page companion |
| `E-STRUCT-005` | More than one class or `data:` block in a single file |
| `E-STRUCT-006` | Logic class with fields or constructor (must be stateless namespace) |
| `E-STRUCT-007` | File outside the canonical folder set |
| `E-STRUCT-008` | Reverse import (lower layer importing from higher layer) |
| `E-STRUCT-009` | Logic file importing another logic file |
| `W-STRUCT-010` | Variable shadows reserved plugin namespace |
| `E-STRUCT-012` | `data T:` block without a paired `class T` in `app/entity/` (DAT-P001) |
| `E-STRUCT-013` | Bare-field `data T:` declaration (v1) — use the sub-block form under v2 |
| `E-STRUCT-014` | Field named in `fields:` not declared on the paired entity (DAT-P003) |
| `E-STRUCT-015` | Non-optional entity field missing from `fields:` (DAT-P004) |
| `FRAME-DATA-E021` | Removed v1 block form (`Model.insert:`, `Model.update:`, `Model.delete:`, `Model.upsert:`, `Model.insert_id:`) — use `Database.save` / `Database.delete` |

(Error codes are reserved but not yet enforced. They will be promoted to
warnings, then to hard errors, over the v0.31–v0.33 release window.)

## 7. Growth Path — Don't Pre-Create Folders

A new app starts with ONLY:

```
main.cln
app/
├── entity/
├── data/models/
├── server/api/
└── web/pages/
```

Add the rest ONLY when the first real need arises:

| Trigger | Folder to add |
|---|---|
| Endpoint or page companion grows beyond a single concern, or touches more than one entity | `app/logic/` |
| Same validation rule appears twice | `app/types/` |
| Cross-entity or aggregate query needed (return type is not `T` / `T?` / `list<T>`) | `app/data/reports/` |
| UI pattern used in more than one page | `app/web/components/` |
| Pages share header/footer/layout | `app/web/layouts/` |
| Schema needs to evolve in production | `app/data/migrations/` |
| Auth becomes non-trivial | `app/auth/` |
| Request filters needed | `app/server/middleware/` |

Empty folders are FORBIDDEN.

## 8. Known Limitations and v2 Migration Notes (today)

- **Under v2, the entity class IS the type.** `class User` in `app/entity/user.cln`
  is the type used throughout the app: `User u = User.data.findOrFailById(id)`,
  `void notify(User user)`, `list<User> subscribers`. The `data User:` block in
  `app/data/models/user.cln` is the persistence view, not the type declaration.
- **`.data` is a compile-time namespace, not a value** (DAT-A001). `var q = User.data`
  is a compile-time error. Call methods directly: `User.data.findByEmail(email)`.
- **`Database` is a compile-time namespace, not a value** (DAT-S007). `var db = Database`
  is a compile-time error.
- **Bare-field `data: T` (v1) and `Model.insert:`/`update:`/`delete:`/`upsert:`
  block forms are removed.** They emit `FRAME-DATA-E021` at compile time with a
  message pointing at the v2 replacement. Do not attempt to use them in new code.
- **Business methods go on the entity, not the data block.** The `data T:` block
  is schema and queries only. Behavior lives in the entity's `functions:` block
  or in `app/logic/` capabilities.

## 9. Example — Auth Capability End-to-End (v2)

`app/entity/user.cln`
```clean
class User
    integer? id
    string email
    private string passwordHash
    string status
    datetime createdAt

    always:
        status in ["active", "pending", "suspended"]

    functions:
        public:
            boolean canPost()
                return status == "active"

            boolean verifiesPassword(string input)
                return crypto.verify(input, passwordHash)
```

`app/data/models/user.cln`
```clean
data User:
    table "users"

    fields:
        id primary generated
        email required unique
        passwordHash required as "password_hash"
        status required default: "pending"
        createdAt required as "created_at"

    indexes:
        email unique
        (status, createdAt)

    queries:
        User? byEmail(string email)
            return User.first: where: email == email

        User findOrFailById(integer id)
            return User.findOrFail: where: id == id

        list<User> activeUsers()
            return User.find: where: status == "active"
```

`app/types/email.cln`
```clean
class Email
    string value

    constructor(string raw)
        require raw.contains("@")
        value = raw.toLowerCase().trim()
```

`app/logic/auth.cln`
```clean
class Auth
    functions:
        public:
            integer register(string email, string password)
                User u = User(email: email, passwordHash: hash(password), status: "pending", createdAt: time.now())
                Database.save(u)               // mutates u.id in place, enforces always: invariants
                Mailer.sendWelcome(u.email)
                return u.id!

            string? login(string email, string password)
                User? candidate = User.data.byEmail(email)
                if candidate == null
                    return null
                User u = candidate!
                if not u.verifiesPassword(password)
                    return null
                return Sessions.issue(u.id!)

            void logout(string sessionToken)
                Sessions.revoke(sessionToken)
```

`app/server/api/auth.cln`
```clean
endpoints:
    POST /api/register
        integer userId = Auth.register(req.body.email, req.body.password)
        return json({ id: userId })

    POST /api/login
        string? token = Auth.login(req.body.email, req.body.password)
        if token == null
            return res.status(401).json({ error: "invalid credentials" })
        return json({ token: token! })
```

Note the shape: entity in `app/entity/` (domain + invariants + methods), data
block in `app/data/models/` (schema + queries), logic in `app/logic/` (orchestrates
entities + Database + other capabilities), endpoint in `app/server/api/` (thin
call to logic method). No `functions:` in the data block, no `Model.insert:`
anywhere, no `db.query` in this flow (it's an escape hatch, not a default).
"##;

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "architecture": architecture,
            "version": crate::VERSION,
            "tip": "Apply the decision tree (§3) BEFORE creating any .cln file. Under v2 the domain shape (fields + invariants + methods) lives in app/entity/<name>.cln; the persistence view (table, indexes, queries) lives in app/data/models/<name>.cln paired by basename. Reads go through <Entity>.data.<method>; writes go through Database.save / Database.delete. Logic classes are stateless capabilities called as Auth.register(...) without instantiating. If a file or class name doesn't fit a capability, it almost certainly belongs in app/entity/, app/data/models/, or app/types/ instead."
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

### JSON (stdlib-reference §8 — the compiler embeds no JSON parser)
- `_json_decode_v2(ptr, len) -> any` — Parse JSON text to a boxed-Any tree (0 on a parse failure)
- `_json_encode_v2(any) -> string` — Compact JSON text of a boxed-Any value
- `_json_encode_pretty_v2(any) -> string` — 2-space indented JSON text
- `_json_get(any, path) -> any` — Value at a dot-separated path (null-tag on a miss)

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
                "main.cln": "Package manifest — declares shared: folders and target: blocks with plugins (REQUIRED)",
                "app/server/api/": "HTTP endpoint handlers (endpoints: blocks) — owned by frame.server",
                "app/server/middleware/": "Request filters — owned by frame.server",
                "app/data/models/": "Data model definitions — one .cln file per model — owned by frame.data",
                "app/data/migrations/": "Schema migration files — owned by frame.data",
                "app/logic/": "Shared business logic — no plugin, compiled for every target",
                "app/web/pages/": "HTML templates (.html) with .cln companion adapters — auto-routed by filename — owned by frame.ui",
                "app/web/components/": "Reusable HTML components (component: blocks) — owned by frame.ui",
                "app/web/layouts/": "Page layout wrappers referenced via <page layout=\"name\"> — owned by frame.ui",
                "app/auth/": "Authentication configuration and guards — owned by frame.auth",
                "public/css/": "Stylesheets served directly (no compilation)",
                "public/images/": "Static image assets"
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
                "ALWAYS create main.cln first — it declares shared: folders and target: blocks. Without it, frame.ui/frame.server silently ignore all your files",
                "Pages go in app/web/pages/ as .html templates with .cln companion adapters (load() returns { key: value } context)",
                "Companion load() delegates to app/logic/ — never queries the database directly",
                "Use {var} for HTML-escaped interpolation, {!var} for raw HTML in templates",
                "app/server/api/ endpoints that return full HTML still use html: blocks (endpoint-level SSR)",
                "SQL queries must return DATA only — NEVER generate HTML inside data queries",
                "Reusable HTML components go in app/web/components/ as component: blocks — referenced as custom HTML tags",
                "Double quotes for HTML attributes inside html: blocks (single quotes cause lexer errors)",
                "For JSON API responses use return json(body); for full HTML pages use html: block as implicit return",
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
        .map(|p| format!("cleen plugin install {}", p))
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

/// Returns true if `text` contains patterns associated with HTML/script injection
/// or LLM prompt injection. Applied to all free-text fields in error reports
/// before storage and display (SEC002).
fn contains_injection(text: &str) -> bool {
    let lower = text.to_lowercase();
    let patterns: &[&str] = &[
        // Web / HTML injection (original SEC001 patterns)
        "<script",
        "javascript:",
        "onerror=",
        "onload=",
        "data:text/html",
        // LLM prompt injection — attacker embeds instructions targeting AI readers
        "ignore previous instructions",
        "ignore all prior",
        "ignore all previous",
        "disregard your instructions",
        "disregard previous",
        "forget your instructions",
        "forget everything",
        "your new instructions",
        "you are now a ",
        "you are now an ",
        "new persona:",
        "[system]",
        "<|im_start|>",
        "<|im_end|>",
        "act as if you",
        "pretend you are",
        "you must now",
        "from now on you",
        "override your instructions",
        "override all instructions",
    ];
    patterns.iter().any(|p| lower.contains(p))
}

/// Read a string field from `args` and cap it at `max_bytes` to prevent payload bloat.
/// Finds a valid UTF-8 char boundary before truncating.
fn read_capped(args: &serde_json::Value, key: &str, max_bytes: usize) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(|s| {
        if s.len() <= max_bytes {
            return s.to_string();
        }
        let mut boundary = max_bytes.saturating_sub(3);
        while boundary > 0 && !s.is_char_boundary(boundary) {
            boundary -= 1;
        }
        format!("{}…", &s[..boundary])
    })
}

/// Component names `report_error` accepts; each maps to the GitHub
/// repository that owns the fix (see `telemetry::normalize_component`).
const REPORT_COMPONENTS: &[&str] = &[
    "compiler",
    "server",
    "node-server",
    "framework",
    "manager",
    "canvas",
    "ui",
    "mcp",
    "unknown",
];

/// MCP result for a drafted bug report: the issue text, the `gh` command
/// that files it and, for dev work, the local dev-queue record.
fn issue_draft_result(
    draft: &crate::telemetry::IssueDraft,
    recorded: Option<(
        crate::telemetry::DevContext,
        crate::telemetry::dev_queue::AppendOutcome,
    )>,
) -> serde_json::Value {
    let mut text = String::new();
    if let Some((ctx, outcome)) = &recorded {
        text.push_str(&format!(
            "Recorded in the local dev queue: fingerprint {} \u{00d7}{} ({}). View with `cln dev-queue list`.\n\n",
            outcome.fingerprint,
            outcome.occurrences,
            ctx.reason().unwrap_or("dev")
        ));
    }
    text.push_str(&format!(
        "Nothing was sent anywhere. File the bug in {} by running:\n\n{}",
        draft.repo(),
        draft.gh_command()
    ));
    json!({
        "content": [{ "type": "text", "text": text }],
        "success": true,
        "repo": draft.repo(),
        "title": draft.title,
        "body": draft.body,
        "gh_command": draft.gh_command(),
        "recorded_in_dev_queue": recorded.is_some(),
        "fingerprint": recorded.as_ref().map(|(_, o)| o.fingerprint.clone()),
        "occurrences": recorded.as_ref().map(|(_, o)| o.occurrences),
    })
}

/// Tool: report_error - Draft a GitHub issue for a toolchain bug
fn tool_report_error(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
    let required = |key: &str| -> Result<String, JsonRpcResponse> {
        args.get(key)
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| {
                JsonRpcResponse::error(
                    id.clone(),
                    error_codes::INVALID_PARAMS,
                    format!("Missing '{}' parameter", key),
                )
            })
    };
    let error_code = match required("error_code") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let error_message = match required("error_message") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let component = match required("component") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let severity = match required("severity") {
        Ok(v) => v,
        Err(e) => return e,
    };

    // SEC002: injection detection — reject all free-text fields containing web XSS
    // or LLM prompt injection patterns. Covers mandatory and all optional fields.
    if contains_injection(&error_code) || contains_injection(&error_message) {
        return JsonRpcResponse::error(
            id,
            error_codes::INVALID_PARAMS,
            "Invalid input: error_code or error_message contains disallowed content".to_string(),
        );
    }
    let optional_text_fields = [
        "minimal_repro",
        "expected_behavior",
        "actual_behavior",
        "ai_analysis",
        "suggested_fix",
        "discovered_during",
        "friendly_summary",
    ];
    for field in &optional_text_fields {
        if let Some(val) = args.get(field).and_then(|v| v.as_str()) {
            if contains_injection(val) {
                return JsonRpcResponse::error(
                    id,
                    error_codes::INVALID_PARAMS,
                    format!(
                        "Invalid input: field '{}' contains disallowed content",
                        field
                    ),
                );
            }
        }
    }
    // SEC002: cap error_message at 2000 characters to prevent payload bloat
    let error_message = crate::error::truncate_chars(&error_message, 2000);

    // A bug cannot be fixed without a reproduction.
    let minimal_repro = match read_capped(args, "minimal_repro", 4000) {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            return JsonRpcResponse::error(
                id,
                error_codes::INVALID_PARAMS,
                "Missing or empty 'minimal_repro' parameter. Synthesize a minimal Clean Language reproduction (smallest start: block that triggers the same failure) and include it as `minimal_repro`.".to_string(),
            )
        }
    };

    // The component decides the repository. It must name WHERE the bug
    // lives, not the task that was running when it was found.
    let Some(canonical_component) = crate::telemetry::normalize_component(&component) else {
        return JsonRpcResponse::error(
            id,
            error_codes::INVALID_PARAMS,
            format!(
                "Invalid component '{}'. Must be one of: {}. Use the component that CONTAINS the buggy code — not the name of the task or tool you were running when you found the bug. If unsure, use 'unknown'.",
                component,
                REPORT_COMPONENTS.join(", ")
            ),
        );
    };

    let mut body = format!(
        "{}\nComponent: {}{}\nSeverity: {}\n\nError {}: {}\n",
        crate::telemetry::environment_section(),
        canonical_component,
        args.get("subsystem")
            .and_then(|v| v.as_str())
            .map(|s| format!(" ({s})"))
            .unwrap_or_default(),
        severity,
        error_code,
        error_message
    );
    if let Some(affected) = args.get("affected_component").and_then(|v| v.as_str()) {
        let version = args
            .get("affected_version")
            .and_then(|v| v.as_str())
            .map(|v| format!(" {v}"))
            .unwrap_or_default();
        body.push_str(&format!("Affected component: {affected}{version}\n"));
    }
    if let Some(summary) = read_capped(args, "friendly_summary", 300) {
        body.push_str(&format!("\n{summary}\n"));
    }
    body.push_str(&format!(
        "\n### Minimal reproduction\n\n```clean\n{}\n```\n",
        minimal_repro.trim_end()
    ));
    let sections = [
        ("Expected behavior", "expected_behavior", 1000),
        ("Actual behavior", "actual_behavior", 1000),
        ("Spec reference", "spec_reference", 500),
        ("Analysis", "ai_analysis", 4000),
        ("Suggested fix", "suggested_fix", 2000),
        ("Likely file", "suggested_component_file", 300),
        ("Discovered during", "discovered_during", 300),
    ];
    for (heading, key, cap) in sections {
        if let Some(text) = read_capped(args, key, cap) {
            body.push_str(&format!("\n### {heading}\n\n{}\n", text.trim_end()));
        }
    }

    let draft = crate::telemetry::IssueDraft::new(
        canonical_component,
        format!(
            "{}: {}",
            error_code,
            crate::error::truncate_chars(&error_message.replace('\n', " "), 80)
        ),
        body,
    );
    // Inside the owning component's source tree the bug is also kept in the
    // local dev queue.
    let recorded = crate::telemetry::record_in_dev_queue(
        canonical_component,
        &error_code,
        &error_message,
        None,
    );
    JsonRpcResponse::success(id, issue_draft_result(&draft, recorded))
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

    let formatted = format_clean_source(source);
    let changed = formatted != source;
    let line_count = formatted.lines().count();

    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "formatted": formatted,
            "changed": changed,
            "line_count": line_count
        }),
    )
}

/// Format Clean Language source:
/// - Normalize line endings to \n
/// - Convert space indentation to tabs, one tab per indentation step. The
///   step is the file's own unit (the greatest common divisor of its
///   space-indent widths), so 2-space and 4-space files both keep their
///   nesting.
/// - Trim trailing whitespace from each line
/// - Ensure single newline at end of file
/// - Remove consecutive blank lines (max 1)
fn format_clean_source(source: &str) -> String {
    fn gcd(a: usize, b: usize) -> usize {
        if b == 0 {
            a
        } else {
            gcd(b, a % b)
        }
    }
    let leading_spaces = |line: &str| {
        line.chars()
            .skip_while(|c| *c == '\t')
            .take_while(|c| *c == ' ')
            .count()
    };
    let indent_unit = source
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(leading_spaces)
        .filter(|n| *n > 0)
        .fold(0, gcd)
        .max(1);

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

        let content = trimmed_right.trim_start();
        let tabs = trimmed_right.chars().take_while(|c| *c == '\t').count();
        let spaces = leading_spaces(trimmed_right);
        let tab_count = tabs + spaces.div_ceil(indent_unit);

        formatted_lines.push(format!("{}{}", "\t".repeat(tab_count), content));
    }

    // Remove trailing blank lines
    while formatted_lines.last().is_some_and(|l| l.is_empty()) {
        formatted_lines.pop();
    }

    formatted_lines.join("\n") + "\n"
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
            // Validate plugin.toml against the manifest schema the plugin
            // loader uses (`[bridge] functions = [ { name, params, returns } ]`).
            match toml::from_str::<crate::plugins::plugin_abi::PluginManifest>(content) {
                Ok(manifest) => {
                    for func in &manifest.bridge.functions {
                        if func.hosts.is_none() {
                            warnings.push(format!(
                                "bridge function {}: no 'hosts' field (treated as [\"all\"])",
                                func.name
                            ));
                        }
                    }
                }
                Err(e) => errors.push(format!(
                    "plugin.toml does not match the plugin manifest schema: {}",
                    e
                )),
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
    let type_check_result = type_check_with_external_plugins(source, file_path);
    let type_check_ok = type_check_result.is_ok();
    let type_errors: Vec<serde_json::Value> = match &type_check_result {
        Ok(_) => vec![],
        Err(errors) => errors.iter().map(error_to_json).collect(),
    };

    // Step 2: Compile (even if type-check fails, to get all diagnostics)
    let compile_result = compile_source_with_detected_plugins(source, file_path, 2);

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
    // The compiler's own commits, embedded at build time (see build.rs).
    const HISTORY: &str = include_str!(concat!(env!("OUT_DIR"), "/changelog.txt"));
    let count = args.get("count").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let entries: Vec<serde_json::Value> = HISTORY
        .lines()
        .filter(|l| !l.is_empty())
        .take(count.min(50))
        .map(|line| {
            let (hash, message) = line.split_once(' ').unwrap_or((line, ""));
            json!({ "hash": hash, "message": message })
        })
        .collect();

    if entries.is_empty() {
        return JsonRpcResponse::success(
            id,
            json!({
                "success": false,
                "error": "This compiler was built without its git history, so no changes are available.",
                "current_version": crate::VERSION
            }),
        );
    }
    JsonRpcResponse::success(
        id,
        json!({
            "success": true,
            "current_version": crate::VERSION,
            "count": entries.len(),
            "entries": entries
        }),
    )
}

/// Convert a CompilerError to JSON for MCP responses
/// Lint: enforce "one resource per endpoints file" (Rule C in get_quick_reference).
///
/// Emits soft warnings — never errors. Always enabled. Codes:
///   W-API-001 — endpoints: block has more than 6 route declarations
///   W-API-002 — file containing endpoints: exceeds 150 non-blank/non-comment lines
///   W-API-003 — endpoints: block mixes 2+ unrelated top-level resources
fn lint_endpoint_organization(source: &str, file_path: &str) -> Vec<serde_json::Value> {
    const MAX_ROUTES_PER_BLOCK: usize = 6;
    const MAX_FILE_LINES: usize = 150;
    const HTTP_VERBS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "FEED", "LIVE"];

    let mut warnings: Vec<serde_json::Value> = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    // Track whether the file contains any endpoints: block (gates W-API-002).
    let mut has_endpoints_block = false;
    let mut significant_line_count = 0usize;

    for raw in &lines {
        let trimmed = raw.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        significant_line_count += 1;
    }

    let indent_width =
        |s: &str| -> usize { s.chars().take_while(|c| *c == '\t' || *c == ' ').count() };

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("endpoints:") {
            has_endpoints_block = true;
            let base_indent = indent_width(line);
            let block_start_line = i + 1; // 1-indexed
            let mut routes: Vec<String> = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let inner = lines[j];
                let inner_trim = inner.trim_start();
                if inner_trim.is_empty() || inner_trim.starts_with("//") {
                    j += 1;
                    continue;
                }
                if indent_width(inner) <= base_indent {
                    break;
                }
                if let Some(rest) = HTTP_VERBS.iter().find_map(|verb| {
                    let needle = format!("{} ", verb);
                    inner_trim.strip_prefix(&needle)
                }) {
                    if let Some(path) = extract_quoted_path(rest) {
                        routes.push(path);
                    }
                }
                j += 1;
            }

            if routes.len() > MAX_ROUTES_PER_BLOCK {
                warnings.push(json!({
                    "severity": "warning",
                    "code": "W-API-001",
                    "message": format!(
                        "endpoints: block declares {} routes (limit {}). Split by resource into separate files under app/server/api/<resource>.cln. See Rule C in get_quick_reference.",
                        routes.len(),
                        MAX_ROUTES_PER_BLOCK
                    ),
                    "file": file_path,
                    "line": block_start_line,
                    "column": 1,
                }));
            }

            let mut distinct_segments: Vec<String> = Vec::new();
            for path in &routes {
                let seg = top_level_segment(path);
                if !distinct_segments.contains(&seg) {
                    distinct_segments.push(seg);
                }
            }
            if distinct_segments.len() >= 2 {
                warnings.push(json!({
                    "severity": "warning",
                    "code": "W-API-003",
                    "message": format!(
                        "endpoints: block mixes {} unrelated resources ({}). Split each resource into its own file (one resource per endpoints file). See Rule C in get_quick_reference.",
                        distinct_segments.len(),
                        distinct_segments
                            .iter()
                            .map(|s| format!("/{}", s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    "file": file_path,
                    "line": block_start_line,
                    "column": 1,
                }));
            }

            i = j;
            continue;
        }
        i += 1;
    }

    if has_endpoints_block && significant_line_count > MAX_FILE_LINES {
        warnings.push(json!({
            "severity": "warning",
            "code": "W-API-002",
            "message": format!(
                "File contains {} non-blank/non-comment lines (limit {} for files with endpoints:). Split by resource into separate files under app/server/api/<resource>.cln. See Rule C in get_quick_reference.",
                significant_line_count,
                MAX_FILE_LINES
            ),
            "file": file_path,
            "line": 1,
            "column": 1,
        }));
    }

    warnings
}

/// Extract the first quoted string from `rest`. Returns the path without quotes.
fn extract_quoted_path(rest: &str) -> Option<String> {
    let bytes = rest.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx] != b'"' {
        idx += 1;
    }
    if idx >= bytes.len() {
        return None;
    }
    idx += 1;
    let start = idx;
    while idx < bytes.len() && bytes[idx] != b'"' {
        idx += 1;
    }
    if idx > start {
        Some(rest[start..idx].to_string())
    } else {
        None
    }
}

/// Return the top-level resource segment of a route path.
/// `/users` -> "users", `/users/:id` -> "users", `/` -> "root", `/api/v1/users` -> "api".
fn top_level_segment(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return "root".to_string();
    }
    let seg = trimmed.split('/').next().unwrap_or("");
    if seg.starts_with(':') || seg.is_empty() {
        "root".to_string()
    } else {
        seg.to_string()
    }
}

fn error_to_json(error: &crate::error::CompilerError) -> serde_json::Value {
    diagnostic_to_json(error, "error")
}

/// JSON form of a diagnostic with the given severity (`error` or `warning`).
fn diagnostic_to_json(error: &crate::error::CompilerError, severity: &str) -> serde_json::Value {
    match error.context() {
        Some(context) => json!({
            "severity": severity,
            "code": context.error_code,
            "message": context.message,
            "file": context.location.as_ref().map(|l| l.file.as_str()).unwrap_or(""),
            "line": context.location.as_ref().map(|l| l.line).unwrap_or(0),
            "column": context.location.as_ref().map(|l| l.column).unwrap_or(0),
        }),
        None => json!({
            "severity": severity,
            "message": error.to_string(),
        }),
    }
}

/// Simple base64 encoding
fn base64_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose, Engine as _};
    general_purpose::STANDARD.encode(bytes)
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
            "Found {} unpublished diagnostic(s). Ask the user if they would like to report {}: publish_diagnostic drafts the GitHub issue and returns the command to file it.",
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

// ============================================================================
// Publish Diagnostic Tool
// ============================================================================

/// Tool: publish_diagnostic - Draft a GitHub issue from a local server diagnostic
fn tool_publish_diagnostic(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
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

    let field = |key: &str| diag.get(key).and_then(|v| v.as_str());
    let wasmtime_error = field("wasmtime_error").unwrap_or("Unknown WASM parse error");
    let compiler_version = field("compiler_version").unwrap_or("unknown");
    let server_version = field("server_version").unwrap_or("unknown");
    let module_path = field("module_path").unwrap_or("unknown");
    let wasmparser_validates = diag
        .get("wasmparser_validates")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let wasmparser_error = field("wasmparser_error")
        .map(|e| format!("\nwasmparser error: {e}"))
        .unwrap_or_default();

    // The module was produced by the compiler and rejected by wasmtime, so
    // the fix lives in the compiler's code generation.
    let body = format!(
        "Auto-captured RUNTIME_WASM_PARSE diagnostic from clean-server.\n\n\
         Compiler that built the module: {compiler_version}\n\
         clean-server: {server_version}\n\
         Module: {module_path}\n\
         WASM SHA-256: {full_sha}\n\n\
         wasmtime rejects the module: {wasmtime_error}\n\
         wasmparser validates the module: {wasmparser_validates}{wasmparser_error}\n\n\
         Expected: the module loads and runs.\n\n\
         The full diagnostic is in {}/pending/{full_sha}/ on the reporting machine.\n",
        diag_dir
    );
    let draft = crate::telemetry::IssueDraft::new(
        "compiler",
        format!(
            "RUNTIME_WASM_PARSE: {}",
            crate::error::truncate_chars(wasmtime_error, 80)
        ),
        body,
    );
    let recorded = crate::telemetry::record_in_dev_queue(
        "compiler",
        "RUNTIME_WASM_PARSE",
        wasmtime_error,
        Some(module_path),
    );
    let mut result = issue_draft_result(&draft, recorded);
    result["sha"] = json!(full_sha);
    result["next_step"] = json!(format!(
        "After filing the issue, mark the diagnostic as forwarded with: clean-server errors publish {}",
        full_sha
    ));
    JsonRpcResponse::success(id, result)
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

    // Feature specs live in the project's docs/features directory.
    let candidate_dirs = [
        std::path::PathBuf::from("docs/features"),
        std::path::PathBuf::from("../docs/features"),
    ];

    let docs_dir = candidate_dirs
        .iter()
        .find(|d| d.exists() && d.is_dir())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("docs/features"));

    // Direct path lookup: only Markdown files inside the feature spec
    // directory can be read (no `..`, absolute or symlinked escapes).
    if let Some(path_str) = path_arg {
        let path = std::path::PathBuf::from(path_str);
        let inside_docs = match (path.canonicalize(), docs_dir.canonicalize()) {
            (Ok(file), Ok(root)) => {
                file.starts_with(&root) && file.extension().and_then(|e| e.to_str()) == Some("md")
            }
            _ => false,
        };
        if !inside_docs {
            return JsonRpcResponse::success(
                id,
                json!({
                    "success": false,
                    "error": format!(
                        "'{}' is not a feature spec: only .md files under {} can be read",
                        path_str,
                        docs_dir.display()
                    )
                }),
            );
        }
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

    // Ref-based lookup: scan the docs directory
    let ref_str = ref_arg.unwrap(); // safe: we checked above

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

#[cfg(test)]
mod endpoint_lint_tests {
    use super::*;

    fn codes(warnings: &[serde_json::Value]) -> Vec<&str> {
        warnings
            .iter()
            .filter_map(|w| w.get("code").and_then(|c| c.as_str()))
            .collect()
    }

    #[test]
    fn no_warning_below_thresholds() {
        let src = "endpoints:\n\tGET \"/users\" -> list()\n\tPOST \"/users\" -> create()\n";
        let warnings = lint_endpoint_organization(src, "api/users.cln");
        assert!(
            warnings.is_empty(),
            "expected no warnings, got {:?}",
            warnings
        );
    }

    #[test]
    fn w_api_001_fires_at_7_routes_same_resource() {
        let mut src = String::from("endpoints:\n");
        for i in 0..7 {
            src.push_str(&format!("\tGET \"/users/{}\" -> h{}()\n", i, i));
        }
        let warnings = lint_endpoint_organization(&src, "api/users.cln");
        let codes = codes(&warnings);
        assert!(
            codes.contains(&"W-API-001"),
            "expected W-API-001 in {:?}",
            codes
        );
        assert!(
            !codes.contains(&"W-API-003"),
            "unexpected W-API-003 in {:?}",
            codes
        );
    }

    #[test]
    fn w_api_003_fires_when_resources_mixed() {
        let src = "endpoints:\n\tGET \"/users\" -> a()\n\tGET \"/orders\" -> b()\n";
        let warnings = lint_endpoint_organization(src, "api.cln");
        let codes = codes(&warnings);
        assert!(
            codes.contains(&"W-API-003"),
            "expected W-API-003 in {:?}",
            codes
        );
    }

    #[test]
    fn w_api_003_does_not_fire_for_subpaths_of_same_resource() {
        let src = "endpoints:\n\tGET \"/users\" -> a()\n\tGET \"/users/:id\" -> b()\n\tGET \"/users/:id/profile\" -> c()\n";
        let warnings = lint_endpoint_organization(src, "api/users.cln");
        let codes = codes(&warnings);
        assert!(
            !codes.contains(&"W-API-003"),
            "unexpected W-API-003 in {:?}",
            codes
        );
    }

    #[test]
    fn w_api_002_fires_only_when_endpoints_present_and_over_limit() {
        // 200 significant lines but no endpoints block -> no warning
        let mut src = String::new();
        for i in 0..200 {
            src.push_str(&format!("integer x{} = {}\n", i, i));
        }
        let warnings = lint_endpoint_organization(&src, "stuff.cln");
        assert!(codes(&warnings).is_empty());

        // Now add an endpoints block -> warning
        src.push_str("endpoints:\n\tGET \"/health\" -> ok()\n");
        let warnings = lint_endpoint_organization(&src, "api.cln");
        let codes = codes(&warnings);
        assert!(
            codes.contains(&"W-API-002"),
            "expected W-API-002 in {:?}",
            codes
        );
    }

    #[test]
    fn comments_and_blank_lines_do_not_count_toward_line_limit() {
        let mut src = String::new();
        for _ in 0..400 {
            src.push_str("// just a comment\n\n");
        }
        src.push_str("endpoints:\n\tGET \"/x\" -> h()\n");
        let warnings = lint_endpoint_organization(&src, "api.cln");
        let codes = codes(&warnings);
        assert!(
            !codes.contains(&"W-API-002"),
            "comments should not trigger W-API-002, got {:?}",
            codes
        );
    }

    #[test]
    fn all_seven_http_verbs_count_as_routes() {
        let src = "endpoints:\n\tGET \"/u\" -> a()\n\tPOST \"/u\" -> b()\n\tPUT \"/u\" -> c()\n\tDELETE \"/u\" -> d()\n\tPATCH \"/u\" -> e()\n\tFEED \"/u\" -> f()\n\tLIVE \"/u\" -> g()\n";
        let warnings = lint_endpoint_organization(src, "api/u.cln");
        let codes = codes(&warnings);
        assert!(
            codes.contains(&"W-API-001"),
            "7 routes should trip W-API-001, got {:?}",
            codes
        );
    }
}

#[cfg(test)]
mod app_structure_tests {
    use super::*;

    #[test]
    fn tool_is_registered() {
        let tools = get_available_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(
            names.contains(&"get_app_structure"),
            "get_app_structure not in registered tools: {:?}",
            names
        );
    }

    #[test]
    fn handler_returns_success_with_architecture_content() {
        let response = tool_get_app_structure(json!(1));
        let result = response.result.expect("expected success result");

        assert_eq!(result.get("success"), Some(&json!(true)));

        let architecture = result
            .get("architecture")
            .and_then(|v| v.as_str())
            .expect("architecture field must be a string");

        assert!(
            !architecture.is_empty(),
            "architecture content must be non-empty"
        );

        // Spot-check the section markers that drive AI behavior. Each is
        // load-bearing: removing one would silently weaken the rule.
        for marker in [
            "Folder Layout",
            "Two Laws",
            "Decision Tree",
            "Naming Rules",
            "Forbidden Patterns",
            "Growth Path",
            "Known Limitations",
        ] {
            assert!(
                architecture.contains(marker),
                "architecture missing section marker '{}'",
                marker
            );
        }

        // Spot-check the canonical folder names — these are the load-bearing
        // structural decisions of the rule.
        for folder in [
            "app/data/models/",
            "app/logic/",
            "app/types/",
            "app/server/api/",
        ] {
            assert!(
                architecture.contains(folder),
                "architecture missing canonical folder path '{}'",
                folder
            );
        }
    }

    #[test]
    fn handler_response_includes_version_and_tip() {
        let response = tool_get_app_structure(json!(1));
        let result = response.result.expect("expected success result");

        assert!(
            result.get("version").is_some(),
            "version field must be present"
        );

        let tip = result
            .get("tip")
            .and_then(|v| v.as_str())
            .expect("tip field must be a string");
        assert!(
            tip.contains("decision tree"),
            "tip should reference the decision tree, got: {}",
            tip
        );
    }
}

#[cfg(test)]
mod report_draft_tests {
    use super::*;

    fn report_args(component: &str) -> serde_json::Value {
        json!({
            "error_code": "COM001",
            "error_message": "invalid WASM: type mismatch",
            "component": component,
            "severity": "bug",
            "minimal_repro": "start:\n\tprint(1)",
            "ai_analysis": "codegen emits i64 where i32 is expected",
        })
    }

    #[test]
    fn report_error_drafts_a_github_issue_instead_of_sending() {
        let _env = crate::telemetry::dev_context::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CLEEN_TELEMETRY_FORCE", "publish");
        let resp = tool_report_error(json!(1), &report_args("compiler"));
        std::env::remove_var("CLEEN_TELEMETRY_FORCE");
        let result = resp.result.expect("success result");
        let command = result["gh_command"].as_str().unwrap();
        assert!(command.starts_with(
            "gh issue create --repo Ivan-Pasco/clean-language-compiler-old --label bug --label compiler-old"
        ));
        assert_eq!(result["recorded_in_dev_queue"], json!(false));
        let body = result["body"].as_str().unwrap();
        assert!(body.contains("print(1)") && body.contains("codegen emits i64"));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Nothing was sent anywhere"));
        assert!(!text.contains("submitted") && !text.contains("will be sent"));
    }

    #[test]
    fn report_error_routes_components_to_their_repositories() {
        let _env = crate::telemetry::dev_context::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("CLEEN_TELEMETRY_FORCE", "publish");
        for (component, repo) in [
            ("node-server", "Ivan-Pasco/clean-server-old"),
            ("ui", "Ivan-Pasco/cleen-framework-old"),
            ("frame.data", "Ivan-Pasco/cleen-framework-old"),
            ("manager", "Ivan-Pasco/clean-language-manager-old"),
            ("mcp", "Ivan-Pasco/clean-language-compiler-old"),
        ] {
            let resp = tool_report_error(json!(1), &report_args(component));
            let result = resp.result.expect("success result");
            assert_eq!(result["repo"], json!(repo), "component {component}");
        }
        std::env::remove_var("CLEEN_TELEMETRY_FORCE");
        let resp = tool_report_error(json!(1), &report_args("validation"));
        assert!(resp.error.is_some(), "task names are not components");
    }

    #[test]
    fn dashboard_tools_are_gone() {
        let names: Vec<String> = get_available_tools().into_iter().map(|t| t.name).collect();
        assert!(!names.iter().any(|n| n == "check_reported_fixes"));
        assert!(!names.iter().any(|n| n == "list_component_bugs"));
    }
}

#[cfg(test)]
mod report_error_schema_tests {
    use super::{get_available_tools, tool_report_error, Tool};
    use serde_json::json;

    fn report_error_tool() -> Tool {
        get_available_tools()
            .into_iter()
            .find(|t| t.name == "report_error")
            .expect("report_error tool must be present")
    }

    #[test]
    fn schema_requires_minimal_repro() {
        let tool = report_error_tool();
        assert!(
            tool.input_schema
                .required
                .iter()
                .any(|f| f == "minimal_repro"),
            "minimal_repro must be listed in the required[] array so schema validators reject reports that omit it"
        );
    }

    #[test]
    fn schema_still_requires_baseline_fields() {
        let tool = report_error_tool();
        for field in ["error_code", "error_message", "component", "severity"] {
            assert!(
                tool.input_schema.required.iter().any(|f| f == field),
                "{} must remain in required[]",
                field
            );
        }
    }

    fn extract_error_message(resp: &super::JsonRpcResponse) -> String {
        let v = serde_json::to_value(resp).expect("serialize response");
        v.get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(String::from)
            .unwrap_or_default()
    }

    #[test]
    fn handler_rejects_missing_minimal_repro() {
        let args = json!({
            "error_code": "SYN001",
            "error_message": "unexpected token",
            "component": "compiler",
            "severity": "bug"
        });
        let resp = tool_report_error(json!(1), &args);
        let msg = extract_error_message(&resp);
        assert!(
            msg.contains("minimal_repro"),
            "handler must reject missing minimal_repro; got: {msg}"
        );
    }

    #[test]
    fn handler_rejects_empty_minimal_repro() {
        let args = json!({
            "error_code": "SYN001",
            "error_message": "unexpected token",
            "component": "compiler",
            "severity": "bug",
            "minimal_repro": "   "
        });
        let resp = tool_report_error(json!(1), &args);
        let msg = extract_error_message(&resp);
        assert!(
            msg.contains("minimal_repro"),
            "handler must reject whitespace-only minimal_repro; got: {msg}"
        );
    }
}

#[cfg(test)]
mod tool_fix_tests {
    use super::*;

    #[test]
    fn format_keeps_nesting_of_two_and_four_space_files() {
        let two = "start:\n  if x\n    print(1)\n  print(2)\n";
        let four = "start:\n    if x\n        print(1)\n    print(2)\n";
        let expected = "start:\n\tif x\n\t\tprint(1)\n\tprint(2)\n";
        assert_eq!(format_clean_source(two), expected);
        assert_eq!(format_clean_source(four), expected);
        // Tabs are kept; blank-line runs collapse.
        assert_eq!(
            format_clean_source("start:\n\tprint(1)\n\n\n\tprint(2)"),
            "start:\n\tprint(1)\n\n\tprint(2)\n"
        );
    }

    #[test]
    fn validate_accepts_bridge_function_arrays() {
        let toml = r#"
[plugin]
name = "frame.demo"
version = "1.0.0"

[handles]
blocks = []

[bridge]
functions = [
  { name = "_demo_call", params = ["string"], returns = "string", hosts = ["server"] },
  { name = "_demo_ping", params = [], returns = "i32" },
]

[ai]
description = "demo"
"#;
        let resp = tool_validate(json!(1), &json!({ "content": toml }));
        let result = resp.result.unwrap();
        assert_eq!(result["valid"], json!(true), "{result}");
        let warnings = result["warnings"].to_string();
        assert!(warnings.contains("_demo_ping"), "{warnings}");
        assert!(!warnings.contains("_demo_call"), "{warnings}");

        let broken = toml.replace("returns = \"i32\"", "");
        let resp = tool_validate(json!(1), &json!({ "content": broken }));
        assert_eq!(resp.result.unwrap()["valid"], json!(false));
    }

    #[test]
    fn feature_spec_path_is_restricted_to_the_spec_directory() {
        for path in ["/etc/hosts", "../../Cargo.toml", "Cargo.toml"] {
            let resp = tool_get_feature_spec(json!(1), &json!({ "path": path }));
            let result = resp.result.unwrap();
            assert_eq!(result["success"], json!(false), "{path}: {result}");
            assert!(result.get("content").is_none(), "{path} was read");
        }
    }

    #[test]
    fn specification_is_embedded() {
        assert!(!spec_sections().is_empty());
        assert!(spec_sections()
            .iter()
            .any(|s| s.title.contains("Expressions")));
    }

    #[test]
    fn changelog_is_this_compilers_history() {
        let resp = tool_get_changelog(json!(1), &json!({ "count": 3 }));
        let result = resp.result.unwrap();
        // Built from a git checkout: the compiler's own commits.
        if result["success"] == json!(true) {
            assert!(result["entries"].as_array().unwrap().len() <= 3);
        } else {
            assert!(result["error"].as_str().unwrap().contains("git history"));
        }
    }
}
