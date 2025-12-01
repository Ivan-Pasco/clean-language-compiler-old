# Clean Language Error Handling and Debugging Guide

This document provides comprehensive guidance for Claude on error handling, debugging infrastructure, and diagnostic systems within the Clean Language compiler. This knowledge is essential for maintaining code quality, implementing robust error recovery, and providing excellent developer experience.

> 🔗 **Related Documentation**: [AST Reference](./ast-reference.md) • [IR Documentation](./intermediate-representation.md) • [Parser Documentation](./parser.md) • [Development Guide](./development-guide.md)

## Overview

The Clean Language compiler implements a sophisticated error handling system designed to provide excellent developer experience through clear error messages, helpful suggestions, and robust recovery mechanisms. The system operates at multiple levels throughout the compilation pipeline and supports both batch error collection and immediate error reporting strategies.

## Error System Architecture

### 1. Error Type Hierarchy (`src/error/mod.rs`)

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
    Replace,      // Replace text at span
    Insert,       // Insert text at span start
    Remove,       // Remove text at span
    Note,         // Just a note, no code change
}

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub span: Option<Span>,
    pub local_variables: HashMap<String, String>,  // name -> debug representation
}

#[derive(Debug, Clone)]
pub struct CodeGenContext {
    pub function: Option<String>,
    pub basic_block: Option<String>,
    pub instruction: Option<String>,
}
```

**Error System Features:**
- **Hierarchical Types**: Specific error types for different compilation phases
- **Rich Context**: Detailed information including spans, suggestions, and related locations
- **Severity Levels**: Different severity levels for appropriate handling
- **Recovery Support**: Distinguishes between recoverable and fatal errors
- **Suggestion System**: Automated fixes and helpful hints

### 2. Warning System

```rust
/// Warning types for non-fatal issues
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

### 3. Diagnostic Collection and Reporting

```rust
/// Central diagnostic collection system
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
    Human,      // Human-readable format
    Json,       // JSON format for tools
    Short,      // Compact format
    Detailed,   // Verbose format with extra context
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
    
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
    
    pub fn warning_count(&self) -> usize {
        self.warnings.len()
    }
    
    pub fn emit_diagnostics(&self, output: &mut dyn std::io::Write) -> std::io::Result<()> {
        match self.settings.error_format {
            ErrorFormat::Human => self.emit_human_format(output),
            ErrorFormat::Json => self.emit_json_format(output),
            ErrorFormat::Short => self.emit_short_format(output),
            ErrorFormat::Detailed => self.emit_detailed_format(output),
        }
    }
    
    fn emit_human_format(&self, output: &mut dyn std::io::Write) -> std::io::Result<()> {
        for error in &self.errors {
            self.emit_human_error(output, error)?;
        }
        
        for warning in &self.warnings {
            self.emit_human_warning(output, warning)?;
        }
        
        // Summary
        if !self.errors.is_empty() || !self.warnings.is_empty() {
            writeln!(output, "\nCompilation finished with {} error(s) and {} warning(s)",
                     self.errors.len(), self.warnings.len())?;
        }
        
        Ok(())
    }
    
    fn emit_human_error(&self, output: &mut dyn std::io::Write, error: &CompilerError) -> std::io::Result<()> {
        let color = if self.settings.color_output { "\x1b[31m" } else { "" };  // Red
        let reset = if self.settings.color_output { "\x1b[0m" } else { "" };
        
        if let Some(span) = error.primary_span() {
            let source_info = self.source_manager.get_source_info(span);
            writeln!(output, "{}error{}: {} --> {}:{}:{}", 
                     color, reset, error, 
                     source_info.file_path.display(),
                     span.start.line, span.start.column)?;
            
            // Show source code context
            self.emit_source_context(output, span)?;
            
            // Show related spans
            match error {
                CompilerError::SemanticError { related_spans, .. } => {
                    for (related_span, message) in related_spans {
                        writeln!(output, "  {} --> {}:{}:{}", 
                                 message,
                                 self.source_manager.get_source_info(related_span).file_path.display(),
                                 related_span.start.line, related_span.start.column)?;
                        self.emit_source_context(output, related_span)?;
                    }
                }
                _ => {}
            }
            
            // Show suggestions
            if self.settings.show_suggestions {
                let suggestions = match error {
                    CompilerError::ParseError { suggestions, .. } => suggestions,
                    CompilerError::SemanticError { suggestions, .. } => suggestions,
                    CompilerError::TypeError { suggestions, .. } => suggestions,
                    _ => &vec![],
                };
                
                for suggestion in suggestions {
                    self.emit_suggestion(output, suggestion)?;
                }
            }
        } else {
            writeln!(output, "{}error{}: {}", color, reset, error)?;
        }
        
        writeln!(output)?;
        Ok(())
    }
    
    fn emit_source_context(&self, output: &mut dyn std::io::Write, span: &Span) -> std::io::Result<()> {
        let source_info = self.source_manager.get_source_info(span);
        let line_number = span.start.line;
        
        // Get source lines around the error
        let context_lines = 2;
        let start_line = line_number.saturating_sub(context_lines);
        let end_line = std::cmp::min(line_number + context_lines, source_info.line_count);
        
        let line_num_width = end_line.to_string().len();
        
        for line_num in start_line..=end_line {
            let line_content = source_info.get_line(line_num).unwrap_or("");
            
            if line_num == line_number {
                // Error line - highlight it
                let color = if self.settings.color_output { "\x1b[1;31m" } else { "" };  // Bold red
                let reset = if self.settings.color_output { "\x1b[0m" } else { "" };
                
                writeln!(output, " {:width$} | {}{}{}", 
                         line_num, color, line_content, reset, width = line_num_width)?;
                
                // Add underline/caret pointing to the error
                let mut underline = " ".repeat(line_num_width + 3);
                let start_col = span.start.column as usize;
                let end_col = span.end.column as usize;
                
                underline.push_str(&" ".repeat(start_col));
                if end_col > start_col {
                    underline.push_str(&"^".repeat(end_col - start_col));
                } else {
                    underline.push('^');
                }
                
                writeln!(output, "{}{}{}", color, underline, reset)?;
            } else {
                // Context line
                writeln!(output, " {:width$} | {}", 
                         line_num, line_content, width = line_num_width)?;
            }
        }
        
        Ok(())
    }
    
    fn emit_suggestion(&self, output: &mut dyn std::io::Write, suggestion: &Suggestion) -> std::io::Result<()> {
        let color = if self.settings.color_output { "\x1b[36m" } else { "" };  // Cyan
        let reset = if self.settings.color_output { "\x1b[0m" } else { "" };
        
        write!(output, "  {}help{}: {}", color, reset, suggestion.message)?;
        
        if let Some(replacement) = &suggestion.replacement {
            match suggestion.kind {
                SuggestionKind::Replace => {
                    writeln!(output, " (replace with: `{}`)", replacement)?;
                }
                SuggestionKind::Insert => {
                    writeln!(output, " (insert: `{}`)", replacement)?;
                }
                _ => {
                    writeln!(output)?;
                }
            }
        } else {
            writeln!(output)?;
        }
        
        Ok(())
    }
}
```

## Error Recovery Strategies

### 1. Parser Error Recovery (`src/parser/recovery.rs`)

```rust
/// Parser error recovery implementation
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

#[derive(Debug, Clone)]
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
    Skip,                    // Skip tokens until recovery point
    Insert(TokenKind),      // Insert missing token
    Replace(TokenKind),     // Replace current token
    Synchronize,            // Find next statement/declaration boundary
}

impl ErrorRecovery {
    pub fn new() -> Self {
        Self {
            recovery_points: vec![
                // Statement-level recovery
                RecoveryPoint {
                    token_kind: TokenKind::Semicolon,
                    context: ParseContext::Statement,
                    action: RecoveryAction::Synchronize,
                },
                RecoveryPoint {
                    token_kind: TokenKind::RightBrace,
                    context: ParseContext::Statement,
                    action: RecoveryAction::Synchronize,
                },
                
                // Declaration-level recovery
                RecoveryPoint {
                    token_kind: TokenKind::Function,
                    context: ParseContext::Declaration,
                    action: RecoveryAction::Synchronize,
                },
                RecoveryPoint {
                    token_kind: TokenKind::Class,
                    context: ParseContext::Declaration,
                    action: RecoveryAction::Synchronize,
                },
                
                // Expression-level recovery
                RecoveryPoint {
                    token_kind: TokenKind::RightParen,
                    context: ParseContext::Expression,
                    action: RecoveryAction::Skip,
                },
                RecoveryPoint {
                    token_kind: TokenKind::Comma,
                    context: ParseContext::Arguments,
                    action: RecoveryAction::Skip,
                },
            ],
            max_recovery_attempts: 10,
            current_attempts: 0,
        }
    }
    
    pub fn attempt_recovery(&mut self, parser: &mut Parser, error: ParseError) -> RecoveryResult {
        if self.current_attempts >= self.max_recovery_attempts {
            return RecoveryResult::GiveUp;
        }
        
        self.current_attempts += 1;
        
        // Find appropriate recovery strategy
        let current_context = parser.current_context();
        
        for recovery_point in &self.recovery_points {
            if recovery_point.context == current_context {
                match self.execute_recovery(parser, recovery_point) {
                    Ok(()) => return RecoveryResult::Recovered,
                    Err(_) => continue,
                }
            }
        }
        
        // Fallback: synchronize to next statement boundary
        self.synchronize_to_statement(parser);
        RecoveryResult::Recovered
    }
    
    fn execute_recovery(&self, parser: &mut Parser, recovery_point: &RecoveryPoint) -> Result<(), RecoveryError> {
        match &recovery_point.action {
            RecoveryAction::Skip => {
                while !parser.is_at_end() && parser.current_token().kind != recovery_point.token_kind {
                    parser.advance();
                }
                Ok(())
            }
            RecoveryAction::Insert(token_kind) => {
                parser.insert_synthetic_token(*token_kind);
                Ok(())
            }
            RecoveryAction::Replace(token_kind) => {
                parser.replace_current_token(*token_kind);
                Ok(())
            }
            RecoveryAction::Synchronize => {
                self.synchronize_to_token(parser, recovery_point.token_kind);
                Ok(())
            }
        }
    }
    
    fn synchronize_to_statement(&self, parser: &mut Parser) {
        while !parser.is_at_end() {
            match parser.current_token().kind {
                TokenKind::Semicolon => {
                    parser.advance();
                    return;
                }
                TokenKind::Function | TokenKind::Class | TokenKind::Data | 
                TokenKind::If | TokenKind::While | TokenKind::For | TokenKind::Return => {
                    return;
                }
                _ => parser.advance(),
            }
        }
    }
    
    fn synchronize_to_token(&self, parser: &mut Parser, target: TokenKind) {
        while !parser.is_at_end() && parser.current_token().kind != target {
            parser.advance();
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum RecoveryResult {
    Recovered,
    GiveUp,
}

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("Cannot recover from this error")]
    CannotRecover,
    #[error("Recovery limit exceeded")]
    LimitExceeded,
}
```

### 2. Semantic Analysis Error Recovery

```rust
/// Semantic analysis error recovery for type checking
pub struct SemanticErrorRecovery {
    type_assumptions: HashMap<NodeId, Type>,
    error_types: HashMap<NodeId, Type>,
}

impl SemanticErrorRecovery {
    pub fn new() -> Self {
        Self {
            type_assumptions: HashMap::new(),
            error_types: HashMap::new(),
        }
    }
    
    pub fn recover_from_type_error(&mut self, node_id: NodeId, error: &TypeError) -> Type {
        // Create a placeholder error type to continue analysis
        let error_type = match error {
            TypeError::UndefinedVariable { expected_type, .. } => {
                expected_type.clone().unwrap_or(Type::Unknown)
            }
            TypeError::TypeMismatch { expected_type, .. } => {
                expected_type.clone()
            }
            TypeError::MethodNotFound { receiver_type, .. } => {
                receiver_type.clone()
            }
            _ => Type::Error,
        };
        
        self.error_types.insert(node_id, error_type.clone());
        error_type
    }
    
    pub fn assume_type(&mut self, node_id: NodeId, assumed_type: Type) {
        self.type_assumptions.insert(node_id, assumed_type);
    }
    
    pub fn get_assumed_type(&self, node_id: NodeId) -> Option<&Type> {
        self.type_assumptions.get(&node_id)
    }
    
    pub fn has_error_type(&self, node_id: NodeId) -> bool {
        self.error_types.contains_key(&node_id)
    }
}

/// Special error types for recovery
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorType {
    Unknown,        // Unknown type, assume any for compatibility
    Error,          // Explicit error type, propagate errors
    Infer(String),  // Type to be inferred later
}

impl Type {
    pub fn is_error_type(&self) -> bool {
        matches!(self, Type::Error | Type::Unknown)
    }
    
    pub fn error_compatible_with(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Error, _) | (_, Type::Error) => true,
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            _ => self == other,
        }
    }
}
```

## Debugging Infrastructure

### 1. Debug Information Generation

```rust
/// Debug information generation for Clean Language programs
pub struct DebugInfoGenerator {
    source_manager: SourceManager,
    symbol_table: SymbolTable,
    debug_info: DebugInfo,
}

#[derive(Debug, Clone)]
pub struct DebugInfo {
    pub compilation_unit: CompilationUnit,
    pub functions: Vec<FunctionDebugInfo>,
    pub types: Vec<TypeDebugInfo>,
    pub variables: Vec<VariableDebugInfo>,
    pub source_map: SourceMap,
}

#[derive(Debug, Clone)]
pub struct CompilationUnit {
    pub name: String,
    pub directory: PathBuf,
    pub producer: String,  // Compiler version
    pub language: String,  // "Clean Language"
    pub optimization_level: OptimizationLevel,
}

#[derive(Debug, Clone)]
pub struct FunctionDebugInfo {
    pub name: String,
    pub mangled_name: Option<String>,
    pub return_type: TypeRef,
    pub parameters: Vec<ParameterDebugInfo>,
    pub local_variables: Vec<LocalVariableDebugInfo>,
    pub line_table: LineTable,
    pub scope_tree: ScopeTree,
}

#[derive(Debug, Clone)]
pub struct ParameterDebugInfo {
    pub name: String,
    pub type_ref: TypeRef,
    pub location: VariableLocation,
}

#[derive(Debug, Clone)]
pub struct LocalVariableDebugInfo {
    pub name: String,
    pub type_ref: TypeRef,
    pub scope: ScopeId,
    pub location: VariableLocation,
    pub live_ranges: Vec<LiveRange>,
}

#[derive(Debug, Clone)]
pub enum VariableLocation {
    Register(u32),
    Memory(i32),     // Offset from frame pointer
    Constant(ConstValue),
    Optimized,       // Variable optimized away
}

#[derive(Debug, Clone)]
pub struct LiveRange {
    pub start_address: u32,
    pub end_address: u32,
    pub location: VariableLocation,
}

#[derive(Debug, Clone)]
pub struct LineTable {
    pub entries: Vec<LineEntry>,
}

#[derive(Debug, Clone)]
pub struct LineEntry {
    pub address: u32,
    pub file: FileId,
    pub line: u32,
    pub column: u32,
    pub is_statement: bool,
    pub is_basic_block: bool,
}

impl DebugInfoGenerator {
    pub fn generate_for_function(&mut self, function: &MIRFunction, wasm_function: &WasmFunction) -> FunctionDebugInfo {
        let mut debug_info = FunctionDebugInfo {
            name: function.name.to_string(),
            mangled_name: None,
            return_type: self.get_type_ref(&function.signature.return_type),
            parameters: Vec::new(),
            local_variables: Vec::new(),
            line_table: LineTable { entries: Vec::new() },
            scope_tree: ScopeTree::new(),
        };
        
        // Generate parameter debug info
        for (i, param) in function.signature.parameters.iter().enumerate() {
            debug_info.parameters.push(ParameterDebugInfo {
                name: param.name.clone(),
                type_ref: self.get_type_ref(&param.ty),
                location: VariableLocation::Register(i as u32),  // WASM parameters are in registers
            });
        }
        
        // Generate local variable debug info
        for (local_id, local_var) in function.local_variables.iter() {
            let location = self.get_variable_location(local_id, wasm_function);
            let live_ranges = self.compute_live_ranges(local_id, function, wasm_function);
            
            debug_info.local_variables.push(LocalVariableDebugInfo {
                name: local_var.name.clone(),
                type_ref: self.get_type_ref(&local_var.ty),
                scope: local_var.scope,
                location,
                live_ranges,
            });
        }
        
        // Generate line table
        debug_info.line_table = self.generate_line_table(function, wasm_function);
        
        debug_info
    }
    
    fn get_variable_location(&self, local_id: LocalId, wasm_function: &WasmFunction) -> VariableLocation {
        // Map MIR local to WASM local index
        if let Some(wasm_local) = wasm_function.local_mapping.get(&local_id) {
            VariableLocation::Register(*wasm_local)
        } else {
            VariableLocation::Optimized
        }
    }
    
    fn compute_live_ranges(&self, local_id: LocalId, function: &MIRFunction, wasm_function: &WasmFunction) -> Vec<LiveRange> {
        let mut ranges = Vec::new();
        
        // Perform liveness analysis to compute precise live ranges
        let liveness = LiveVariableAnalysis::analyze(function);
        
        let mut current_range: Option<(u32, VariableLocation)> = None;
        
        for (block_id, block) in function.basic_blocks.iter() {
            let block_start_addr = wasm_function.block_addresses.get(&block_id).copied().unwrap_or(0);
            
            if liveness.is_live_at_block_start(block_id, local_id) {
                if current_range.is_none() {
                    let location = self.get_variable_location(local_id, wasm_function);
                    current_range = Some((block_start_addr, location));
                }
            }
            
            let block_end_addr = block_start_addr + wasm_function.block_sizes.get(&block_id).copied().unwrap_or(0);
            
            if !liveness.is_live_at_block_end(block_id, local_id) {
                if let Some((start_addr, location)) = current_range.take() {
                    ranges.push(LiveRange {
                        start_address: start_addr,
                        end_address: block_end_addr,
                        location,
                    });
                }
            }
        }
        
        ranges
    }
    
    fn generate_line_table(&self, function: &MIRFunction, wasm_function: &WasmFunction) -> LineTable {
        let mut entries = Vec::new();
        
        for (block_id, block) in function.basic_blocks.iter() {
            let block_addr = wasm_function.block_addresses.get(&block_id).copied().unwrap_or(0);
            
            for (stmt_idx, statement) in block.statements.iter().enumerate() {
                if let Some(span) = statement.span() {
                    let stmt_addr = block_addr + stmt_idx as u32;  // Simplified address calculation
                    
                    entries.push(LineEntry {
                        address: stmt_addr,
                        file: span.file_id,
                        line: span.start.line,
                        column: span.start.column,
                        is_statement: true,
                        is_basic_block: stmt_idx == 0,
                    });
                }
            }
        }
        
        entries.sort_by_key(|entry| entry.address);
        LineTable { entries }
    }
}
```

### 2. Runtime Debugging Support

```rust
/// Runtime debugging support for Clean Language programs
pub struct RuntimeDebugger {
    breakpoints: HashMap<u32, Breakpoint>,  // address -> breakpoint
    call_stack: Vec<StackFrame>,
    variable_inspector: VariableInspector,
    memory_inspector: MemoryInspector,
}

#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub id: BreakpointId,
    pub address: u32,
    pub condition: Option<String>,  // Conditional breakpoint expression
    pub hit_count: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub address: u32,
    pub local_variables: HashMap<String, DebugValue>,
    pub parameters: HashMap<String, DebugValue>,
}

#[derive(Debug, Clone)]
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
    Unavailable(String),  // Reason why value is not available
}

impl RuntimeDebugger {
    pub fn new() -> Self {
        Self {
            breakpoints: HashMap::new(),
            call_stack: Vec::new(),
            variable_inspector: VariableInspector::new(),
            memory_inspector: MemoryInspector::new(),
        }
    }
    
    pub fn set_breakpoint(&mut self, address: u32, condition: Option<String>) -> BreakpointId {
        let id = BreakpointId::new();
        let breakpoint = Breakpoint {
            id,
            address,
            condition,
            hit_count: 0,
            enabled: true,
        };
        
        self.breakpoints.insert(address, breakpoint);
        id
    }
    
    pub fn should_break(&mut self, address: u32, runtime_state: &RuntimeState) -> bool {
        if let Some(breakpoint) = self.breakpoints.get_mut(&address) {
            if !breakpoint.enabled {
                return false;
            }
            
            breakpoint.hit_count += 1;
            
            // Check condition if present
            if let Some(condition) = &breakpoint.condition {
                match self.evaluate_condition(condition, runtime_state) {
                    Ok(true) => true,
                    Ok(false) => false,
                    Err(e) => {
                        eprintln!("Breakpoint condition evaluation error: {}", e);
                        true  // Break on condition evaluation error
                    }
                }
            } else {
                true
            }
        } else {
            false
        }
    }
    
    pub fn inspect_variable(&self, name: &str, runtime_state: &RuntimeState) -> Result<DebugValue, DebugError> {
        self.variable_inspector.inspect(name, runtime_state)
    }
    
    pub fn inspect_memory(&self, address: u32, size: u32, runtime_state: &RuntimeState) -> Result<Vec<u8>, DebugError> {
        self.memory_inspector.read(address, size, runtime_state)
    }
    
    pub fn get_call_stack(&self) -> &[StackFrame] {
        &self.call_stack
    }
    
    fn evaluate_condition(&self, condition: &str, runtime_state: &RuntimeState) -> Result<bool, DebugError> {
        // Simple expression evaluator for breakpoint conditions
        // This would typically use a small expression parser
        
        // For now, support simple variable comparisons
        if let Some((var_name, expected_value)) = condition.split_once("==") {
            let var_name = var_name.trim();
            let expected_value = expected_value.trim();
            
            let actual_value = self.inspect_variable(var_name, runtime_state)?;
            let expected_debug_value = self.parse_debug_value(expected_value)?;
            
            Ok(actual_value == expected_debug_value)
        } else {
            Err(DebugError::InvalidCondition {
                condition: condition.to_string(),
            })
        }
    }
    
    fn parse_debug_value(&self, value_str: &str) -> Result<DebugValue, DebugError> {
        // Simple parser for debug values in conditions
        if let Ok(int_val) = value_str.parse::<i64>() {
            Ok(DebugValue::Integer(int_val))
        } else if let Ok(float_val) = value_str.parse::<f64>() {
            Ok(DebugValue::Number(float_val))
        } else if value_str == "true" {
            Ok(DebugValue::Boolean(true))
        } else if value_str == "false" {
            Ok(DebugValue::Boolean(false))
        } else if value_str.starts_with('"') && value_str.ends_with('"') {
            Ok(DebugValue::String(value_str[1..value_str.len()-1].to_string()))
        } else {
            Err(DebugError::InvalidValue {
                value: value_str.to_string(),
            })
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DebugError {
    #[error("Variable not found: {name}")]
    VariableNotFound { name: String },
    
    #[error("Invalid memory address: {address}")]
    InvalidAddress { address: u32 },
    
    #[error("Invalid breakpoint condition: {condition}")]
    InvalidCondition { condition: String },
    
    #[error("Invalid debug value: {value}")]
    InvalidValue { value: String },
    
    #[error("Runtime state unavailable")]
    RuntimeStateUnavailable,
}
```

### 3. Performance Profiling Integration

```rust
/// Performance profiling support
pub struct Profiler {
    enabled: bool,
    samples: Vec<ProfileSample>,
    function_stats: HashMap<String, FunctionStats>,
    memory_stats: MemoryStats,
    start_time: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct ProfileSample {
    pub timestamp: std::time::Duration,
    pub instruction_pointer: u32,
    pub function_name: String,
    pub call_stack_depth: usize,
}

#[derive(Debug, Clone)]
pub struct FunctionStats {
    pub name: String,
    pub call_count: u64,
    pub total_time: std::time::Duration,
    pub exclusive_time: std::time::Duration,
    pub memory_allocated: u64,
    pub memory_freed: u64,
}

#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub total_allocated: u64,
    pub total_freed: u64,
    pub current_usage: u64,
    pub peak_usage: u64,
    pub allocation_count: u64,
    pub free_count: u64,
    pub gc_count: u64,
    pub gc_time: std::time::Duration,
}

impl Profiler {
    pub fn new() -> Self {
        Self {
            enabled: false,
            samples: Vec::new(),
            function_stats: HashMap::new(),
            memory_stats: MemoryStats {
                total_allocated: 0,
                total_freed: 0,
                current_usage: 0,
                peak_usage: 0,
                allocation_count: 0,
                free_count: 0,
                gc_count: 0,
                gc_time: std::time::Duration::default(),
            },
            start_time: std::time::Instant::now(),
        }
    }
    
    pub fn enable(&mut self) {
        self.enabled = true;
        self.start_time = std::time::Instant::now();
    }
    
    pub fn disable(&mut self) {
        self.enabled = false;
    }
    
    pub fn sample(&mut self, instruction_pointer: u32, function_name: String, call_stack_depth: usize) {
        if !self.enabled {
            return;
        }
        
        let sample = ProfileSample {
            timestamp: self.start_time.elapsed(),
            instruction_pointer,
            function_name: function_name.clone(),
            call_stack_depth,
        };
        
        self.samples.push(sample);
        
        // Update function stats
        let stats = self.function_stats.entry(function_name).or_insert(FunctionStats {
            name: function_name.clone(),
            call_count: 0,
            total_time: std::time::Duration::default(),
            exclusive_time: std::time::Duration::default(),
            memory_allocated: 0,
            memory_freed: 0,
        });
        
        stats.call_count += 1;
    }
    
    pub fn record_allocation(&mut self, size: u64) {
        if !self.enabled {
            return;
        }
        
        self.memory_stats.total_allocated += size;
        self.memory_stats.current_usage += size;
        self.memory_stats.allocation_count += 1;
        
        if self.memory_stats.current_usage > self.memory_stats.peak_usage {
            self.memory_stats.peak_usage = self.memory_stats.current_usage;
        }
    }
    
    pub fn record_deallocation(&mut self, size: u64) {
        if !self.enabled {
            return;
        }
        
        self.memory_stats.total_freed += size;
        self.memory_stats.current_usage = self.memory_stats.current_usage.saturating_sub(size);
        self.memory_stats.free_count += 1;
    }
    
    pub fn record_gc(&mut self, gc_time: std::time::Duration) {
        if !self.enabled {
            return;
        }
        
        self.memory_stats.gc_count += 1;
        self.memory_stats.gc_time += gc_time;
    }
    
    pub fn generate_report(&self) -> ProfileReport {
        ProfileReport::new(&self.samples, &self.function_stats, &self.memory_stats)
    }
}

pub struct ProfileReport {
    pub total_samples: usize,
    pub duration: std::time::Duration,
    pub hot_functions: Vec<FunctionStats>,
    pub memory_summary: MemoryStats,
    pub call_graph: CallGraph,
}

impl ProfileReport {
    pub fn new(samples: &[ProfileSample], function_stats: &HashMap<String, FunctionStats>, memory_stats: &MemoryStats) -> Self {
        let total_samples = samples.len();
        let duration = samples.last().map(|s| s.timestamp).unwrap_or_default();
        
        let mut hot_functions: Vec<FunctionStats> = function_stats.values().cloned().collect();
        hot_functions.sort_by_key(|stats| std::cmp::Reverse(stats.call_count));
        hot_functions.truncate(20);  // Top 20 functions
        
        let call_graph = CallGraph::from_samples(samples);
        
        Self {
            total_samples,
            duration,
            hot_functions,
            memory_summary: memory_stats.clone(),
            call_graph,
        }
    }
    
    pub fn print_report(&self, output: &mut dyn std::io::Write) -> std::io::Result<()> {
        writeln!(output, "Profile Report")?;
        writeln!(output, "=============")?;
        writeln!(output, "Total samples: {}", self.total_samples)?;
        writeln!(output, "Duration: {:.2}ms", self.duration.as_millis())?;
        writeln!(output)?;
        
        writeln!(output, "Hot Functions:")?;
        writeln!(output, "{:<30} {:<10} {:<15} {:<15}", "Function", "Calls", "Total Time", "Avg Time")?;
        writeln!(output, "{:-<70}", "")?;
        
        for stats in &self.hot_functions {
            let avg_time = if stats.call_count > 0 {
                stats.total_time / stats.call_count as u32
            } else {
                std::time::Duration::default()
            };
            
            writeln!(output, "{:<30} {:<10} {:<15.2} {:<15.2}",
                     stats.name,
                     stats.call_count,
                     stats.total_time.as_millis(),
                     avg_time.as_millis())?;
        }
        
        writeln!(output)?;
        writeln!(output, "Memory Statistics:")?;
        writeln!(output, "Total allocated: {} bytes", self.memory_summary.total_allocated)?;
        writeln!(output, "Total freed: {} bytes", self.memory_summary.total_freed)?;
        writeln!(output, "Current usage: {} bytes", self.memory_summary.current_usage)?;
        writeln!(output, "Peak usage: {} bytes", self.memory_summary.peak_usage)?;
        writeln!(output, "GC count: {}", self.memory_summary.gc_count)?;
        writeln!(output, "GC time: {:.2}ms", self.memory_summary.gc_time.as_millis())?;
        
        Ok(())
    }
}
```

## Testing Error Handling Systems

### 1. Error Testing Framework

```rust
/// Testing framework for error handling
pub mod error_testing {
    use super::*;
    
    pub struct ErrorTestCase {
        pub name: String,
        pub source_code: String,
        pub expected_errors: Vec<ExpectedError>,
        pub expected_warnings: Vec<ExpectedWarning>,
        pub recovery_expected: bool,
    }
    
    #[derive(Debug, Clone)]
    pub struct ExpectedError {
        pub error_type: String,  // Error type name
        pub message_pattern: String,  // Regex pattern for message
        pub line: u32,
        pub column_range: Option<(u32, u32)>,
        pub suggestions: Vec<String>,  // Expected suggestion messages
    }
    
    #[derive(Debug, Clone)]
    pub struct ExpectedWarning {
        pub warning_type: String,
        pub message_pattern: String,
        pub line: u32,
    }
    
    pub fn test_error_cases(test_cases: &[ErrorTestCase]) -> TestResults {
        let mut results = TestResults::new();
        
        for test_case in test_cases {
            let result = test_single_error_case(test_case);
            results.add_result(test_case.name.clone(), result);
        }
        
        results
    }
    
    fn test_single_error_case(test_case: &ErrorTestCase) -> TestResult {
        let mut compiler = Compiler::new();
        let compile_result = compiler.compile_source(&test_case.source_code);
        
        match compile_result {
            Ok(_) if !test_case.expected_errors.is_empty() => {
                TestResult::Failed {
                    reason: "Expected errors but compilation succeeded".to_string(),
                }
            }
            Err(errors) => {
                // Check if we got the expected errors
                if verify_expected_errors(&errors, &test_case.expected_errors) {
                    TestResult::Passed
                } else {
                    TestResult::Failed {
                        reason: format!("Error mismatch. Got: {:?}, Expected: {:?}", 
                                      errors, test_case.expected_errors),
                    }
                }
            }
            Ok(_) => TestResult::Passed,
        }
    }
    
    fn verify_expected_errors(actual_errors: &[CompilerError], expected_errors: &[ExpectedError]) -> bool {
        if actual_errors.len() != expected_errors.len() {
            return false;
        }
        
        for (actual, expected) in actual_errors.iter().zip(expected_errors.iter()) {
            if !matches_expected_error(actual, expected) {
                return false;
            }
        }
        
        true
    }
    
    fn matches_expected_error(actual: &CompilerError, expected: &ExpectedError) -> bool {
        // Check error type
        let actual_type = match actual {
            CompilerError::ParseError { .. } => "ParseError",
            CompilerError::SemanticError { .. } => "SemanticError",
            CompilerError::TypeError { .. } => "TypeError",
            CompilerError::CodeGenError { .. } => "CodeGenError",
            _ => "Other",
        };
        
        if actual_type != expected.error_type {
            return false;
        }
        
        // Check message pattern
        let message = actual.to_string();
        let regex = regex::Regex::new(&expected.message_pattern).unwrap();
        if !regex.is_match(&message) {
            return false;
        }
        
        // Check location
        if let Some(span) = actual.primary_span() {
            if span.start.line != expected.line {
                return false;
            }
            
            if let Some((start_col, end_col)) = expected.column_range {
                if span.start.column < start_col || span.end.column > end_col {
                    return false;
                }
            }
        }
        
        true
    }
    
    #[derive(Debug)]
    pub struct TestResults {
        pub passed: usize,
        pub failed: usize,
        pub results: Vec<(String, TestResult)>,
    }
    
    impl TestResults {
        pub fn new() -> Self {
            Self {
                passed: 0,
                failed: 0,
                results: Vec::new(),
            }
        }
        
        pub fn add_result(&mut self, name: String, result: TestResult) {
            match result {
                TestResult::Passed => self.passed += 1,
                TestResult::Failed { .. } => self.failed += 1,
            }
            self.results.push((name, result));
        }
    }
    
    #[derive(Debug)]
    pub enum TestResult {
        Passed,
        Failed { reason: String },
    }
}
```

## CLI Error Handling Features

The Clean Language compiler provides comprehensive CLI options for error handling, debugging, and developer experience.

### Verbosity Flags

Control the amount of output from the compiler:

```bash
# Default: Only warnings and errors (no debug spam)
cln compile file.cln -o output.wasm

# Verbose: Show info-level messages
cln -v compile file.cln -o output.wasm

# Debug: Show debug-level messages
cln -vv compile file.cln -o output.wasm

# Trace: Show all messages including internal traces
cln -vvv compile file.cln -o output.wasm

# Quiet: Suppress all output except errors
cln -q compile file.cln -o output.wasm
cln --quiet compile file.cln -o output.wasm
```

### Output Format Options

```bash
# Machine-readable JSON diagnostics (for IDE integration)
cln --json compile file.cln -o output.wasm

# Disable colored output (for CI/CD or piping)
cln --no-color compile file.cln -o output.wasm

# Combine options
cln --json --quiet compile file.cln -o output.wasm
```

### Error Code Explanations

Get detailed explanations for any error code:

```bash
# Explain a type error
cln explain TYP001

# Output:
# error[TYP001]: Type mismatch
#
# Description:
#   The type of an expression doesn't match what was expected.
#   Clean Language is strongly typed - you cannot implicitly convert
#   between incompatible types.
#
# Example of problematic code:
#   start()
#       integer x = "hello"  // Cannot assign string to integer
#
# How to fix:
#   Ensure types match or use explicit conversion:
#   - Use type conversion: x.toInteger(), x.toString()
#   - Declare with correct type: string x = "hello"
#   - Use appropriate literal: integer x = 42

# Case-insensitive - both work
cln explain syn003
cln explain SYN003

# JSON output for IDE plugins
cln --json explain TYP002

# List all available error codes
cln explain UNKNOWN
```

### Available Error Codes

| Category | Codes | Description |
|----------|-------|-------------|
| Syntax | SYN001-SYN010 | Parser and syntax errors |
| Type | TYP001-TYP010 | Type checking errors |
| Memory | MEM001-MEM005 | Memory management errors |
| Runtime | RUN001-RUN005 | Runtime execution errors |

### Error Output Format

The compiler provides Rust/Elm-style error output with source context and underlines:

```
error[TYP002]: Undefined variable
  --> /path/to/file.cln:3:2
   |
 1 | start()
 2 |     // This should cause an error
 3 |     undefinedVariable = 42
   |  ^^^^^^^^^^^^^^^^^
   |  Variable 'undefinedVariable' not found
 4 |     print "Hello"
   |

Summary: 1 error found
  validation: 1
```

### Debug Command

Enhanced debugging with error analysis:

```bash
# Debug with AST display
cln debug file.cln --show-ast

# Debug with style checking
cln debug file.cln --check-style

# Debug with error analysis
cln debug file.cln --analyze-errors

# Combine all debugging options
cln debug file.cln --show-ast --check-style --analyze-errors
```

### Lint Command

Code style and convention validation:

```bash
# Lint a single file
cln lint file.cln

# Lint a directory
cln lint src/

# Show only errors (suppress warnings)
cln lint file.cln --errors-only

# Auto-fix issues (when available)
cln lint file.cln --fix
```

### Parse Command

Detailed parsing information:

```bash
# Parse and show detailed tree
cln parse file.cln --show-tree

# Parse with error recovery mode
cln parse file.cln --recover-errors
```

### Run Command

Execute Clean Language files directly:

```bash
# Run a .cln source file (compiles and executes)
cln run program.cln

# Run with debug output
cln run program.cln --debug

# Run a pre-compiled .wasm file
cln run program.wasm
```

### Environment Variables

You can also control logging via environment variables:

```bash
# Set log level via RUST_LOG
RUST_LOG=debug cln compile file.cln -o output.wasm

# More specific filtering
RUST_LOG=clean_language_compiler::codegen=trace cln compile file.cln -o output.wasm
```

## Best Practices for Claude

When working with Clean Language error handling and debugging:

1. **Error Collection**: Collect multiple errors before stopping compilation when possible
2. **Rich Context**: Always provide span information and related spans for errors
3. **Recovery Strategies**: Implement appropriate error recovery for each compilation phase
4. **User Experience**: Write clear, actionable error messages with suggestions
5. **Debug Information**: Maintain accurate debug information through all transformations
6. **Testing**: Comprehensive testing of error cases and recovery mechanisms
7. **Performance**: Balance error reporting quality with compilation performance
8. **Consistency**: Use consistent error formats and recovery strategies throughout

This error handling documentation provides the foundation for implementing robust error reporting and debugging capabilities in the Clean Language compiler.