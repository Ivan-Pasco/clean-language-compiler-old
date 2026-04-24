# Error Handling and Debugging — Implementation Guide

> Error code definitions and semantic rules live in [`spec/semantic-rules.md`](../../spec/semantic-rules.md) and [`spec/error-codes.md`](../../spec/error-codes.md). This guide covers the compiler's implementation of error reporting and recovery.

> 🔗 **Related Documentation**: [Compilation Pipeline](./compilation-pipeline.md) • [Development Guide](./development-guide.md) • [Parser](./parser.md)

---

## Error Type Hierarchy (`src/error/mod.rs`)

The compiler uses a single top-level enum that carries phase-specific context. Each variant corresponds to a compilation phase; the severity and recoverability differ by variant.

```rust
/// Main error type for the Clean Language compiler
#[derive(Debug, thiserror::Error)]
pub enum CompilerError {
    #[error("Parse error: {message}")]
    ParseError {
        message: String,
        span: Span,
        suggestions: Vec<Suggestion>,
        recoverable: bool,
    },

    #[error("Semantic error: {message}")]
    SemanticError {
        message: String,
        span: Span,
        related_spans: Vec<(Span, String)>,
        suggestions: Vec<Suggestion>,
    },

    #[error("Type error: {message}")]
    TypeError {
        message: String,
        span: Span,
        expected_type: Option<Type>,
        actual_type: Option<Type>,
        suggestions: Vec<Suggestion>,
    },

    #[error("Code generation error: {message}")]
    CodeGenError {
        message: String,
        span: Option<Span>,
        context: CodeGenContext,
    },

    #[error("Runtime error: {message}")]
    RuntimeError {
        message: String,
        span: Option<Span>,
        stack_trace: Vec<StackFrame>,
    },

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Internal compiler error: {message}")]
    InternalError {
        message: String,
        location: &'static std::panic::Location<'static>,
        context: Option<String>,
    },
}

impl CompilerError {
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            CompilerError::ParseError { .. } => ErrorSeverity::Error,
            CompilerError::SemanticError { .. } => ErrorSeverity::Error,
            CompilerError::TypeError { .. } => ErrorSeverity::Error,
            CompilerError::CodeGenError { .. } => ErrorSeverity::Error,
            CompilerError::RuntimeError { .. } => ErrorSeverity::Fatal,
            CompilerError::IoError(_) => ErrorSeverity::Fatal,
            CompilerError::InternalError { .. } => ErrorSeverity::Fatal,
        }
    }

    pub fn is_recoverable(&self) -> bool {
        match self {
            CompilerError::ParseError { recoverable, .. } => *recoverable,
            CompilerError::SemanticError { .. } => true,
            CompilerError::TypeError { .. } => true,
            CompilerError::CodeGenError { .. } => false,
            _ => false,
        }
    }

    pub fn primary_span(&self) -> Option<&Span> {
        match self {
            CompilerError::ParseError { span, .. } => Some(span),
            CompilerError::SemanticError { span, .. } => Some(span),
            CompilerError::TypeError { span, .. } => Some(span),
            CompilerError::CodeGenError { span, .. } => span.as_ref(),
            CompilerError::RuntimeError { span, .. } => span.as_ref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorSeverity {
    Note,
    Warning,
    Error,
    Fatal,
}

#[derive(Debug, Clone)]
pub struct Suggestion {
    pub message: String,
    pub span: Span,
    pub replacement: Option<String>,
    pub kind: SuggestionKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SuggestionKind {
    Replace,   // Replace text at span
    Insert,    // Insert text at span start
    Remove,    // Remove text at span
    Note,      // Just a note, no code change
}

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub span: Option<Span>,
    pub local_variables: HashMap<String, String>, // name -> debug representation
}

#[derive(Debug, Clone)]
pub struct CodeGenContext {
    pub function: Option<String>,
    pub basic_block: Option<String>,
    pub instruction: Option<String>,
}
```

### Warning System

Warnings are non-fatal. They can be promoted to errors via `--warnings-as-errors`.

```rust
#[derive(Debug, thiserror::Error)]
pub enum CompilerWarning {
    #[error("Unused variable: {name}")]
    UnusedVariable {
        name: String,
        span: Span,
        suggestion: Option<Suggestion>,
    },

    #[error("Unreachable code")]
    UnreachableCode {
        span: Span,
        reason: String,
    },

    #[error("Deprecated feature: {feature}")]
    DeprecatedFeature {
        feature: String,
        span: Span,
        replacement: Option<String>,
    },

    #[error("Performance warning: {message}")]
    Performance {
        message: String,
        span: Span,
        optimization_hint: String,
    },

    #[error("Style warning: {message}")]
    Style {
        message: String,
        span: Span,
        suggestion: Suggestion,
    },
}

impl CompilerWarning {
    pub fn lint_name(&self) -> &'static str {
        match self {
            CompilerWarning::UnusedVariable { .. } => "unused-variable",
            CompilerWarning::UnreachableCode { .. } => "unreachable-code",
            CompilerWarning::DeprecatedFeature { .. } => "deprecated",
            CompilerWarning::Performance { .. } => "performance",
            CompilerWarning::Style { .. } => "style",
        }
    }

    pub fn is_enabled_by_default(&self) -> bool {
        match self {
            CompilerWarning::UnusedVariable { .. } => true,
            CompilerWarning::UnreachableCode { .. } => true,
            CompilerWarning::DeprecatedFeature { .. } => true,
            CompilerWarning::Performance { .. } => false,
            CompilerWarning::Style { .. } => false,
        }
    }
}
```

---

## Diagnostic Engine (`src/error/diagnostic.rs`)

`DiagnosticEngine` is the central accumulator passed through the compilation pipeline. Each phase calls `emit_error` or `emit_warning`; the engine collects them all and renders them at the end.

```rust
pub struct DiagnosticEngine {
    errors: Vec<CompilerError>,
    warnings: Vec<CompilerWarning>,
    source_manager: SourceManager,
    error_limit: usize,
    warning_limit: usize,
    settings: DiagnosticSettings,
}

#[derive(Debug, Clone)]
pub struct DiagnosticSettings {
    pub warnings_as_errors: bool,
    pub disabled_warnings: HashSet<String>,
    pub enabled_warnings: HashSet<String>,
    pub error_format: ErrorFormat,
    pub show_suggestions: bool,
    pub color_output: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorFormat {
    Human,    // Human-readable (default)
    Json,     // JSON for tooling/IDE integration
    Short,    // Compact single-line format
    Detailed, // Verbose with extra context
}

impl DiagnosticEngine {
    pub fn new(source_manager: SourceManager) -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            source_manager,
            error_limit: 100,
            warning_limit: 1000,
            settings: DiagnosticSettings::default(),
        }
    }

    pub fn emit_error(&mut self, error: CompilerError) {
        if self.errors.len() >= self.error_limit {
            return;
        }
        self.errors.push(error);
    }

    pub fn emit_warning(&mut self, warning: CompilerWarning) {
        if self.warnings.len() >= self.warning_limit {
            return;
        }
        let lint_name = warning.lint_name();
        if self.settings.disabled_warnings.contains(lint_name) {
            return;
        }
        if self.settings.warnings_as_errors {
            let error = CompilerError::SemanticError {
                message: warning.to_string(),
                span: warning.primary_span().cloned().unwrap_or_default(),
                related_spans: vec![],
                suggestions: vec![],
            };
            self.emit_error(error);
        } else {
            self.warnings.push(warning);
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn emit_diagnostics(&self, output: &mut dyn std::io::Write) -> std::io::Result<()> {
        match self.settings.error_format {
            ErrorFormat::Human => self.emit_human_format(output),
            ErrorFormat::Json => self.emit_json_format(output),
            ErrorFormat::Short => self.emit_short_format(output),
            ErrorFormat::Detailed => self.emit_detailed_format(output),
        }
    }
}
```

### Human-Readable Output Format

The human format renders a source excerpt with a caret underline, matching the Rust/Elm diagnostic style. The output looks like:

```
error[SEM001]: type mismatch: expected 'integer', got 'string'
  --> src/main.cln:12:5
   |
10 |     integer x = "hello"
   |                 ^^^^^^^
   |                 expected 'integer', found 'string'
   |
help: use an integer literal or convert the value (replace with: `42`)
```

The rendering logic lives in `DiagnosticEngine::emit_human_error`. It:

1. Resolves the span to a `SourceInfo` (file path, line count, line content) via `SourceManager`.
2. Prints 2 lines of context above and below the error line.
3. Prints a caret underline (`^`) spanning the exact column range of the span.
4. Iterates `related_spans` for secondary context (e.g., where a symbol was first declared).
5. Prints `Suggestion` entries in cyan if `show_suggestions` is enabled.

The implementation uses ANSI escape codes gated on `settings.color_output`. Pass `--no-color` or detect a non-TTY to disable them.

---

## Error Recovery

### Parser Error Recovery (`src/parser/recovery.rs`)

The parser uses a synchronization-based recovery strategy. When a parse error is encountered, `ErrorRecovery::attempt_recovery` is called. It selects a recovery point matching the current parse context and performs one of four actions:

| Action | Meaning |
|--------|---------|
| `Skip` | Advance past tokens until the recovery token is found |
| `Insert(t)` | Synthesize a missing token and continue |
| `Replace(t)` | Swap the current token and continue |
| `Synchronize` | Seek forward to the nearest statement or declaration boundary |

Recovery is bounded by `max_recovery_attempts` (default: 10) to prevent infinite loops on severely malformed input.

```rust
pub struct ErrorRecovery {
    recovery_points: Vec<RecoveryPoint>,
    max_recovery_attempts: usize,
    current_attempts: usize,
}

#[derive(Debug, Clone)]
pub struct RecoveryPoint {
    pub token_kind: TokenKind,
    pub context: ParseContext,
    pub action: RecoveryAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseContext {
    Statement,
    Expression,
    Declaration,
    Type,
    Parameters,
    Arguments,
}

#[derive(Debug, Clone)]
pub enum RecoveryAction {
    Skip,
    Insert(TokenKind),
    Replace(TokenKind),
    Synchronize,
}

#[derive(Debug, PartialEq)]
pub enum RecoveryResult {
    Recovered,
    GiveUp,
}
```

The fallback is `synchronize_to_statement`, which scans forward until it finds a semicolon or a keyword that starts a new declaration (`function`, `class`, `data`, `if`, `while`, `for`, `return`). This keeps the parser alive long enough to report subsequent independent errors in the same file.

### Semantic Error Recovery (`src/typechecker/recovery.rs`)

During type checking, errors do not halt analysis. Instead, `SemanticErrorRecovery` assigns an *error type* to the failing node so that downstream analysis can continue without cascading type errors:

```rust
pub struct SemanticErrorRecovery {
    type_assumptions: HashMap<NodeId, Type>,
    error_types: HashMap<NodeId, Type>,
}

impl SemanticErrorRecovery {
    /// Returns a stand-in type for a node that failed type checking.
    /// Downstream uses of the node receive this type instead of crashing.
    pub fn recover_from_type_error(&mut self, node_id: NodeId, error: &TypeError) -> Type {
        let error_type = match error {
            TypeError::UndefinedVariable { expected_type, .. } => {
                expected_type.clone().unwrap_or(Type::Unknown)
            }
            TypeError::TypeMismatch { expected_type, .. } => expected_type.clone(),
            TypeError::MethodNotFound { receiver_type, .. } => receiver_type.clone(),
            _ => Type::Error,
        };
        self.error_types.insert(node_id, error_type.clone());
        error_type
    }
}
```

`Type::Error` and `Type::Unknown` are both *error-compatible*: any operation on them succeeds without emitting a second diagnostic. This prevents a single undefined variable from generating dozens of cascading type errors.

```rust
impl Type {
    pub fn is_error_type(&self) -> bool {
        matches!(self, Type::Error | Type::Unknown)
    }

    /// Two types are error-compatible if either is an error type.
    /// Used to suppress cascading errors after a recovery.
    pub fn error_compatible_with(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Error, _) | (_, Type::Error) => true,
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            _ => self == other,
        }
    }
}
```

---

## Debug Information Generation (`src/codegen/debug_info.rs`)

`DebugInfoGenerator` attaches source-level information to the generated WASM so that tools can map execution addresses back to `.cln` source lines.

```rust
pub struct DebugInfo {
    pub compilation_unit: CompilationUnit,
    pub functions: Vec<FunctionDebugInfo>,
    pub types: Vec<TypeDebugInfo>,
    pub variables: Vec<VariableDebugInfo>,
    pub source_map: SourceMap,
}

pub struct FunctionDebugInfo {
    pub name: String,
    pub mangled_name: Option<String>,
    pub return_type: TypeRef,
    pub parameters: Vec<ParameterDebugInfo>,
    pub local_variables: Vec<LocalVariableDebugInfo>,
    pub line_table: LineTable,
    pub scope_tree: ScopeTree,
}

pub struct LocalVariableDebugInfo {
    pub name: String,
    pub type_ref: TypeRef,
    pub scope: ScopeId,
    pub location: VariableLocation,
    pub live_ranges: Vec<LiveRange>,
}

pub enum VariableLocation {
    Register(u32),       // WASM local index
    Memory(i32),         // Offset from frame pointer
    Constant(ConstValue),
    Optimized,           // Variable eliminated by the optimizer
}
```

The `LineTable` maps WASM byte offsets to `(file, line, column)` triples. It is built during codegen by recording each statement's span as instructions are emitted. After all blocks are generated, entries are sorted by address for binary-search lookup.

Live range analysis (`LiveVariableAnalysis::analyze`) determines where each local variable is actually live in the control flow graph. This allows debuggers to accurately report "variable optimized away" rather than showing stale values.

---

## Runtime Debugging Support (`src/runtime/debugger.rs`)

`RuntimeDebugger` supports interactive debugging of running WASM programs. It is not active during normal compilation; it is engaged when the CLI runs a program with `--debug`.

```rust
pub struct RuntimeDebugger {
    breakpoints: HashMap<u32, Breakpoint>,
    call_stack: Vec<StackFrame>,
    variable_inspector: VariableInspector,
    memory_inspector: MemoryInspector,
}

pub struct Breakpoint {
    pub id: BreakpointId,
    pub address: u32,
    pub condition: Option<String>, // Conditional breakpoint expression
    pub hit_count: u32,
    pub enabled: bool,
}

pub enum DebugValue {
    Integer(i64),
    Number(f64),
    Boolean(bool),
    String(String),
    List(Vec<DebugValue>),
    Object {
        type_name: String,
        fields: HashMap<String, DebugValue>,
    },
    Null,
    Unavailable(String), // Reason why value is not available
}
```

Conditional breakpoints (`condition: Option<String>`) support simple equality expressions of the form `variable == value`. The condition evaluator uses `VariableInspector` to read the current value and `parse_debug_value` to parse the expected value from the condition string. Condition evaluation errors cause the breakpoint to fire (fail-open), and the error is printed to stderr.

---

## Performance Profiling (`src/runtime/profiler.rs`)

The `Profiler` is an optional sampling profiler attached to the WASM runtime. It is enabled with `cln run --profile`.

```rust
pub struct Profiler {
    enabled: bool,
    samples: Vec<ProfileSample>,
    function_stats: HashMap<String, FunctionStats>,
    memory_stats: MemoryStats,
    start_time: std::time::Instant,
}

pub struct FunctionStats {
    pub name: String,
    pub call_count: u64,
    pub total_time: std::time::Duration,
    pub exclusive_time: std::time::Duration,
    pub memory_allocated: u64,
    pub memory_freed: u64,
}
```

`Profiler::generate_report` sorts functions by `call_count` descending, truncates to the top 20, and builds a `CallGraph` from the sample stream. `ProfileReport::print_report` writes a human-readable table to any `Write` output.

---

## Testing Error Handling

Error test cases are written as source strings paired with expected error descriptions. The framework compiles the source, then verifies error type, message pattern, and source location.

```rust
pub struct ErrorTestCase {
    pub name: String,
    pub source_code: String,
    pub expected_errors: Vec<ExpectedError>,
    pub expected_warnings: Vec<ExpectedWarning>,
    pub recovery_expected: bool,
}

pub struct ExpectedError {
    pub error_type: String,      // "ParseError", "SemanticError", "TypeError", "CodeGenError"
    pub message_pattern: String, // Regex pattern matched against the formatted message
    pub line: u32,
    pub column_range: Option<(u32, u32)>,
    pub suggestions: Vec<String>,
}
```

`matches_expected_error` uses the `regex` crate to match `message_pattern` against the rendered error string, then checks that `span.start.line` equals `expected.line`. Column range checking is optional; omit it when the exact column is not load-bearing for the test.

When an error test case passes compilation unexpectedly, the framework reports `"Expected errors but compilation succeeded"` — never silently passes.

---

## CLI Error Handling Features

### Verbosity and Output Format

```bash
# Default: warnings and errors only
cln compile file.cln -o output.wasm

# Verbose: info-level messages
cln -v compile file.cln -o output.wasm

# Debug: internal debug messages
cln -vv compile file.cln -o output.wasm

# Trace: all messages including internal traces
cln -vvv compile file.cln -o output.wasm

# Quiet: suppress everything except errors
cln -q compile file.cln -o output.wasm

# Machine-readable JSON diagnostics (for IDE integration)
cln --json compile file.cln -o output.wasm

# Disable ANSI color (for CI or piped output)
cln --no-color compile file.cln -o output.wasm
```

### Error Code Explanations

```bash
# Explain a specific error code (case-insensitive)
cln explain SEM001
cln explain sem001

# JSON output for IDE plugins
cln --json explain SEM002

# List all available error codes
cln explain --list
```

For the full list of error codes and their definitions, see [`spec/error-codes.md`](../../spec/error-codes.md).

### Diagnostic Output Format

The compiler emits diagnostics in this format (see also `spec/error-codes.md` §"Diagnostic Format"):

```
error[SEM002]: undefined symbol 'undefinedVariable'
  --> /path/to/file.cln:3:5
   |
 1 | start:
 2 |     // This will fail
 3 |     undefinedVariable = 42
   |     ^^^^^^^^^^^^^^^^^
   |
Summary: 1 error found
```

### Additional CLI Commands

```bash
# Debug: show AST and internal representations
cln debug file.cln --show-ast
cln debug file.cln --analyze-errors

# Lint: code style validation
cln lint file.cln
cln lint file.cln --fix

# Parse: show parse tree
cln parse file.cln --show-tree
cln parse file.cln --recover-errors

# Run: compile and execute
cln run program.cln
cln run program.cln --debug
cln run program.cln --profile
```

### Environment Variables

```bash
# Route internal compiler logs via RUST_LOG
RUST_LOG=debug cln compile file.cln -o output.wasm

# Narrow to a specific module
RUST_LOG=clean_language_compiler::codegen=trace cln compile file.cln -o output.wasm
```

---

## Adding a New Error to the Compiler

Follow this sequence when adding a new diagnostic:

1. **Check `spec/error-codes.md`** to see if a code already covers the condition. If not, propose a new code (requires developer approval per Principle 25).

2. **Add the error code to `spec/semantic-rules.md`** and `spec/error-codes.md` (after approval), with:
   - The triggering condition
   - An example of code that triggers it
   - The exact message format

3. **Construct a `CompilerError`** in the appropriate phase (parser, HIR builder, type checker, or codegen). Always include a `span` and, if the fix is clear, a `Suggestion`.

4. **Call `diagnostic_engine.emit_error(error)`** — never `panic!` or `eprintln!` for user-facing errors.

5. **Write an error test case** using `ErrorTestCase` with `message_pattern` matching the new message and the correct `line`.

6. **Run `cargo test`** to confirm the new test passes and no existing tests regress.

The error code must appear verbatim in the formatted message string so that `cln explain <CODE>` can surface the definition.
