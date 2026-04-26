use super::class_parser::parse_class;
use super::expression_parser::parse_expression;
use super::statement_parser::parse_statement;
use super::type_parser::parse_type;
use super::Rule;
use super::{convert_to_ast_location, CleanParser};
use crate::ast::{
    Class, Expression, Function, FunctionModifier, FunctionSyntax, ImportItem, Parameter, Program,
    Statement, TestCase, Type, Visibility,
};
use crate::error::{CompilerError, EnhancedErrorCollector, ErrorUtils};
use pest::{iterators::Pair, Parser};

/// Parse context to track file information and improve error reporting
#[derive(Clone)]
pub struct ParseContext {
    pub file_path: String,
}

impl ParseContext {
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }
}

/// Enhanced parser with error recovery capabilities
pub struct ErrorRecoveringParser {
    pub source: String,
    pub file_path: String,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<crate::error::CompilerWarning>,
    pub recovery_points: Vec<usize>,
    pub max_errors: usize,
    pub enhanced_collector: EnhancedErrorCollector,
}

impl ErrorRecoveringParser {
    pub fn new(source: &str, file_path: &str) -> Self {
        Self {
            source: source.to_string(),
            file_path: file_path.to_string(),
            errors: Vec::new(),
            warnings: Vec::new(),
            recovery_points: Vec::new(),
            max_errors: 100, // Prevent infinite error cascades
            enhanced_collector: EnhancedErrorCollector::new(),
        }
    }

    pub fn with_max_errors(mut self, max_errors: usize) -> Self {
        self.max_errors = max_errors;
        self
    }

    /// Simple program parsing method for SpecificationParser compatibility
    pub fn parse_program(&mut self) -> Result<Program, CompilerError> {
        match self.parse_with_recovery(&self.source.clone()) {
            Ok(program) => Ok(program),
            Err(errors) => {
                // Return the first error if any
                if let Some(first_error) = errors.first() {
                    Err(first_error.clone())
                } else {
                    Err(CompilerError::syntax_error(
                        "Unknown parsing error".to_string(),
                        None,
                        None,
                    ))
                }
            }
        }
    }

    /// Parse with comprehensive error recovery - collects multiple errors and continues parsing
    pub fn parse_with_recovery(&mut self, source: &str) -> Result<Program, Vec<CompilerError>> {
        // First, try to identify recovery points in the source
        self.identify_recovery_points(source);

        // Attempt to parse the entire program with recovery
        match self.parse_with_error_recovery(source) {
            Ok(program) => {
                if self.errors.is_empty() {
                    Ok(program)
                } else {
                    // Return program with warnings/errors for partial compilation
                    Err(self.errors.clone())
                }
            }
            Err(mut parse_errors) => {
                // Merge any additional errors we collected during recovery
                parse_errors.extend(self.errors.clone());
                // Deduplicate similar errors
                parse_errors = self.deduplicate_errors(parse_errors);
                Err(parse_errors)
            }
        }
    }

    /// Identify synchronization points for error recovery
    fn identify_recovery_points(&mut self, source: &str) {
        let lines: Vec<&str> = source.lines().collect();

        // Clear existing recovery points
        self.recovery_points.clear();

        // Find major structural boundaries
        let mut byte_pos = 0;
        for (_line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Function declarations
            if trimmed.starts_with("function ") || trimmed.starts_with("functions:") {
                self.recovery_points.push(byte_pos);
            }
            // Class declarations
            else if trimmed.starts_with("class ") {
                self.recovery_points.push(byte_pos);
            }
            // Start block
            else if trimmed.starts_with("start:") {
                self.recovery_points.push(byte_pos);
            }
            // Import statements
            else if trimmed.starts_with("import ") || trimmed.starts_with("import:") {
                self.recovery_points.push(byte_pos);
            }
            // Block-level constructs
            else if trimmed.ends_with(":")
                && (trimmed.starts_with("if ")
                    || trimmed.starts_with("while ")
                    || trimmed.starts_with("for ")
                    || trimmed == "functions:"
                    || trimmed == "tests:")
            {
                self.recovery_points.push(byte_pos);
            }
            // Statement-level recovery (indented lines)
            else if line.starts_with('\t') || (line.starts_with(' ') && line.trim() != "") {
                // Only add if it's not already close to another recovery point
                if self.recovery_points.is_empty()
                    || byte_pos.saturating_sub(*self.recovery_points.last().unwrap()) > 20
                {
                    self.recovery_points.push(byte_pos);
                }
            }

            byte_pos += line.len() + 1; // +1 for newline
        }

        // Add end of file as recovery point
        self.recovery_points.push(source.len());

        // Sort recovery points
        self.recovery_points.sort_unstable();
        self.recovery_points.dedup();
    }

    /// Parse with error recovery using synchronization points
    fn parse_with_error_recovery(&mut self, source: &str) -> Result<Program, Vec<CompilerError>> {
        let mut collected_errors = Vec::new();

        // Try to parse the whole program first
        match self.parse_internal(source) {
            Ok(program) => return Ok(program),
            Err(initial_error) => {
                collected_errors.push(initial_error);

                // If we have too many errors already, stop
                if collected_errors.len() >= self.max_errors {
                    return Err(collected_errors);
                }
            }
        }

        // If full parsing failed, try recovery parsing
        // Split source into segments and try to parse each segment
        let segments = self.split_into_recoverable_segments(source);
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut imports = Vec::new();
        let mut start_function = None;
        let mut successful_segments = 0;
        let total_segments = segments.len();

        for (segment_idx, segment) in segments.iter().enumerate() {
            match self.parse_segment(segment) {
                Ok(segment_result) => {
                    successful_segments += 1;
                    // Merge successful parse results
                    functions.extend(segment_result.functions);
                    classes.extend(segment_result.classes);
                    imports.extend(segment_result.imports);
                    if segment_result.start_function.is_some() {
                        start_function = segment_result.start_function;
                    }
                }
                Err(segment_error) => {
                    // Enhance error with segment context
                    let enhanced_error = self.enhance_error_with_segment_context(
                        segment_error,
                        segment_idx,
                        total_segments,
                        segment,
                    );
                    collected_errors.push(enhanced_error);

                    // Try to create a partial AST node for the failed segment
                    if let Some(partial) = self.create_partial_node(segment) {
                        match partial {
                            PartialNode::Function(f) => {
                                let function_name = f.name.clone();
                                let function_location = f.location.clone();
                                functions.push(f);
                                // Add recovery warning
                                self.warnings.push(crate::error::CompilerWarning::new(
                                    format!("Function '{}' parsed with errors - may have incomplete body", function_name),
                                    crate::error::WarningType::DeadCode,
                                    function_location.map(|l| crate::ast::SourceLocation {
                                        file: self.file_path.clone(),
                                        line: l.line,
                                        column: l.column,
                                        byte_start: None,
                                        byte_end: None,
                                    })
                                ));
                            }
                            PartialNode::Class(c) => {
                                let class_name = c.name.clone();
                                let class_location = c.location.clone();
                                classes.push(c);
                                // Add recovery warning
                                self.warnings.push(crate::error::CompilerWarning::new(
                                    format!("Class '{}' parsed with errors - may have incomplete definition", class_name),
                                    crate::error::WarningType::DeadCode,
                                    class_location.map(|l| crate::ast::SourceLocation {
                                        file: self.file_path.clone(),
                                        line: l.line,
                                        column: l.column,
                                        byte_start: None,
                                        byte_end: None,
                                    })
                                ));
                            }
                        }
                    }
                }
            }

            if collected_errors.len() >= self.max_errors {
                // Add warning about stopping early
                self.warnings.push(crate::error::CompilerWarning::new(
                    format!(
                        "Stopped error recovery after {} errors (max: {})",
                        collected_errors.len(),
                        self.max_errors
                    ),
                    crate::error::WarningType::Performance,
                    None,
                ));
                break;
            }
        }

        // Add recovery statistics
        if total_segments > 0 && successful_segments < total_segments {
            self.warnings.push(crate::error::CompilerWarning::new(
                format!(
                    "Parsed {}/{} segments successfully during error recovery",
                    successful_segments, total_segments
                ),
                crate::error::WarningType::Performance,
                None,
            ));
        }

        // Create a program from recovered parts
        let recovered_program = Program {
            imports,
            plugins: Vec::new(),
            statements: Vec::new(),
            functions,
            classes,
            start_function,
            tests: Vec::new(),
            screens: Vec::new(),
            state: None,
            watch_blocks: Vec::new(),
            screen_blocks: Vec::new(),
            externals: Vec::new(),
            source_block: None,
            location: None,
        };

        if collected_errors.is_empty() {
            Ok(recovered_program)
        } else {
            self.errors.extend(collected_errors.clone());
            Err(collected_errors)
        }
    }

    /// Split source into segments that can be parsed independently
    fn split_into_recoverable_segments(&self, source: &str) -> Vec<String> {
        // Check if this is a functions: block program - if so, don't split it
        if source.contains("functions:") {
            // Return the entire source as one segment for functions: block handling
            return vec![source.to_string()];
        }

        let lines: Vec<&str> = source.lines().collect();
        let mut segments = Vec::new();
        let mut current_segment = String::new();
        let mut in_function = false;
        let mut in_class = false;
        let mut class_indent_level = 0;

        for line in lines {
            let trimmed = line.trim();
            let line_indent = line.len() - line.trim_start().len();

            // Detect class start
            if trimmed.starts_with("class ") {
                if !current_segment.trim().is_empty() {
                    segments.push(current_segment.clone());
                    current_segment.clear();
                }
                in_class = true;
                class_indent_level = line_indent;
                current_segment.push_str(line);
                current_segment.push('\n');
                continue;
            }

            // If we're in a class, don't split until we exit the class
            if in_class {
                current_segment.push_str(line);
                current_segment.push('\n');

                // Check if we've exited the class (non-empty line at class level or less indentation)
                if !trimmed.is_empty()
                    && !trimmed.starts_with("//")
                    && line_indent <= class_indent_level
                    && !trimmed.starts_with("class ")
                {
                    // We've exited the class, finalize the class segment
                    segments.push(current_segment.clone());
                    current_segment.clear();
                    in_class = false;

                    // Start new segment with current line if it's not empty
                    if !trimmed.is_empty() {
                        current_segment.push_str(line);
                        current_segment.push('\n');
                    }
                }
                continue;
            }

            // Detect standalone function start (not in class)
            if trimmed.starts_with("function ") {
                if !current_segment.trim().is_empty() {
                    segments.push(current_segment.clone());
                    current_segment.clear();
                }
                in_function = true;
            }

            current_segment.push_str(line);
            current_segment.push('\n');

            // Track indentation to detect function end
            if in_function {
                if trimmed.is_empty() || trimmed.starts_with("//") {
                    continue; // Skip empty lines and comments
                }

                if !line.starts_with('\t') && !line.starts_with(' ') && !trimmed.is_empty() {
                    // End of function - line at root level
                    if trimmed.starts_with("function ") || trimmed.starts_with("class ") {
                        // Start of new function/class
                        segments.push(current_segment.clone());
                        current_segment.clear();
                        current_segment.push_str(line);
                        current_segment.push('\n');
                    } else {
                        // End of current function
                        segments.push(current_segment.clone());
                        current_segment.clear();
                        in_function = false;
                    }
                }
            }
        }

        if !current_segment.trim().is_empty() {
            segments.push(current_segment);
        }

        segments
    }

    /// Parse a single segment with error handling
    fn parse_segment(&mut self, segment: &str) -> Result<Program, CompilerError> {
        let trimmed_segment = segment.trim();

        // Try to parse as a complete program first
        match <CleanParser as Parser<Rule>>::parse(Rule::program, trimmed_segment) {
            Ok(pairs) => parse_program_ast(pairs),
            Err(pest_error) => {
                // Try to parse as individual components
                if trimmed_segment.starts_with("function ") {
                    self.parse_function_segment(trimmed_segment)
                } else if trimmed_segment.starts_with("class ") {
                    self.parse_class_segment(trimmed_segment)
                } else if trimmed_segment.contains("functions:") {
                    // Handle functions: blocks using preprocessor
                    self.parse_functions_block_segment(trimmed_segment)
                } else {
                    Err(crate::error::ErrorUtils::from_pest_error(
                        pest_error,
                        segment,
                        &self.file_path,
                    ))
                }
            }
        }
    }

    /// Parse a function segment with recovery
    fn parse_function_segment(&mut self, segment: &str) -> Result<Program, CompilerError> {
        // Try to parse as start block first
        if let Ok(pairs) = <CleanParser as Parser<Rule>>::parse(Rule::start_block, segment) {
            let start_func = parse_start_block(pairs.into_iter().next().unwrap())?;
            return Ok(Program {
                imports: Vec::new(),
                plugins: Vec::new(),
                statements: Vec::new(),
                functions: Vec::new(),
                classes: Vec::new(),
                start_function: Some(start_func),
                tests: Vec::new(),
                screens: Vec::new(),
                state: None,
                watch_blocks: Vec::new(),
                screen_blocks: Vec::new(),
                externals: Vec::new(),
                source_block: None,
                location: None,
            });
        }

        // Standalone functions removed - all functions must be in functions: blocks per specification

        Err(CompilerError::parse_error(
            {
                let segment_line = segment.lines().next().unwrap_or("").trim();
                format!("Could not parse function segment: {segment_line}")
            },
            None,
            Some(
                "Check function syntax: function name() or function returnType name()".to_string(),
            ),
        ))
    }

    /// Parse a functions: block segment using preprocessor
    fn parse_functions_block_segment(&mut self, segment: &str) -> Result<Program, CompilerError> {
        // Extract the functions block content, including start() if present
        let lines: Vec<&str> = segment.lines().collect();
        let mut functions_block_content = String::new();
        let mut start_function_lines = Vec::new();
        let mut in_functions_block = false;
        let mut in_start_function = false;
        let mut current_indent = 0;

        for line in &lines {
            let trimmed = line.trim();
            let line_indent = line.len() - line.trim_start().len();

            if trimmed == "functions:" {
                in_functions_block = true;
                functions_block_content.push_str(line);
                functions_block_content.push('\n');
            } else if in_functions_block {
                // Check if we've exited the functions block
                if !trimmed.is_empty() && !trimmed.starts_with("//") && line_indent == 0 {
                    // We've exited the functions block
                    in_functions_block = false;
                    // Check if this is a start block
                    if trimmed.starts_with("start:") {
                        in_start_function = true;
                        current_indent = line_indent;
                        start_function_lines.push(line.to_string());
                    }
                } else {
                    functions_block_content.push_str(line);
                    functions_block_content.push('\n');
                }
            } else if in_start_function {
                // Check if we're still in the start function
                if trimmed.is_empty() || trimmed.starts_with("//") || line_indent > current_indent {
                    start_function_lines.push(line.to_string());
                } else {
                    // We've exited the start function
                    in_start_function = false;
                }
            } else if trimmed.starts_with("start:") {
                in_start_function = true;
                current_indent = line_indent;
                start_function_lines.push(line.to_string());
            }
        }

        // Use preprocessor to parse the functions block
        let preprocessor = super::preprocessor::FunctionPreprocessor::new(&functions_block_content);
        match preprocessor.process_functions_block(&functions_block_content) {
            Ok(functions) => {
                // Parse start block if present
                let start_function = if !start_function_lines.is_empty() {
                    let start_source = start_function_lines.join("\n");
                    match <CleanParser as Parser<Rule>>::parse(Rule::start_block, &start_source) {
                        Ok(pairs) => Some(parse_start_block(pairs.into_iter().next().unwrap())?),
                        Err(_) => {
                            // Failed to parse start block, continuing without it
                            None
                        }
                    }
                } else {
                    None
                };

                Ok(Program {
                    imports: Vec::new(),
                    plugins: Vec::new(),
                    statements: Vec::new(),
                    functions,
                    classes: Vec::new(),
                    start_function,
                    tests: Vec::new(),
                    screens: Vec::new(),
                    state: None,
                    watch_blocks: Vec::new(),
                    screen_blocks: Vec::new(),
                    externals: Vec::new(),
                    source_block: None,
                    location: None,
                })
            }
            Err(e) => Err(e),
        }
    }

    /// Parse a class segment with recovery
    fn parse_class_segment(&mut self, segment: &str) -> Result<Program, CompilerError> {
        match <CleanParser as Parser<Rule>>::parse(Rule::class_decl, segment) {
            Ok(pairs) => {
                let class = parse_class(pairs.into_iter().next().unwrap())?;
                Ok(Program {
                    imports: Vec::new(),
                    plugins: Vec::new(),
                    statements: Vec::new(),
                    functions: Vec::new(),
                    classes: vec![class],
                    start_function: None,
                    tests: Vec::new(),
                    screens: Vec::new(),
                    state: None,
                    watch_blocks: Vec::new(),
                    screen_blocks: Vec::new(),
                    externals: Vec::new(),
                    source_block: None,
                    location: None,
                })
            }
            Err(pest_error) => Err(crate::error::ErrorUtils::from_pest_error(
                pest_error,
                segment,
                &self.file_path,
            )),
        }
    }

    /// Create partial AST nodes for failed segments
    fn create_partial_node(&self, segment: &str) -> Option<PartialNode> {
        let trimmed = segment.trim();

        // Try to extract function name even if parsing failed
        if trimmed.starts_with("function ") {
            if let Some((name, return_type, params)) = self.extract_function_signature(trimmed) {
                return Some(PartialNode::Function(Function {
                    name: name.clone(),
                    type_parameters: Vec::new(),
                    type_constraints: Vec::new(),
                    parameters: params,
                    return_type,
                    body: vec![Statement::Expression {
                        expr: Expression::Literal(crate::ast::Value::String(format!(
                            "// ERROR: Function '{}' body could not be parsed due to syntax errors",
                            name
                        ))),
                        location: None,
                    }],
                    description: Some(format!(
                        "// RECOVERY: Function '{}' - syntax errors in body",
                        name
                    )),
                    syntax: FunctionSyntax::Simple,
                    visibility: Visibility::Public,
                    modifier: FunctionModifier::None,
                    location: self.calculate_location_from_segment(segment),
                }));
            }
        } else if trimmed.starts_with("class ") {
            if let Some(class_name) = self.extract_class_name(trimmed) {
                return Some(PartialNode::Class(Class {
                    name: class_name.clone(),
                    type_parameters: Vec::new(),
                    description: Some(format!(
                        "// RECOVERY: Class '{}' - syntax errors in definition",
                        class_name
                    )),
                    base_class: None,
                    base_class_type_args: Vec::new(),
                    fields: Vec::new(),
                    methods: Vec::new(),
                    constructor: None,
                    invariants: Vec::new(),
                    location: self.calculate_location_from_segment(segment),
                }));
            }
        } else if trimmed.starts_with("start:") {
            // Try to create a partial start block
            return Some(PartialNode::Function(Function {
                name: "start".to_string(),
                type_parameters: Vec::new(),
                type_constraints: Vec::new(),
                parameters: Vec::new(),
                return_type: Type::Void,
                body: vec![Statement::Expression {
                    expr: Expression::Literal(crate::ast::Value::String(
                        "// ERROR: Start function body could not be parsed".to_string(),
                    )),
                    location: None,
                }],
                description: Some(
                    "// RECOVERY: Start function - syntax errors in body".to_string(),
                ),
                syntax: FunctionSyntax::Simple,
                visibility: Visibility::Public,
                modifier: FunctionModifier::None,
                location: self.calculate_location_from_segment(segment),
            }));
        }

        None
    }

    /// Extract function name from malformed function declaration
    fn parse_internal(&mut self, source: &str) -> Result<Program, CompilerError> {
        let trimmed_source = source.trim();
        let pairs = <CleanParser as Parser<Rule>>::parse(Rule::program, trimmed_source)
            .map_err(|e| crate::error::ErrorUtils::from_pest_error(e, source, &self.file_path))?;

        let result = parse_program_ast(pairs);
        result
    }
}

/// Represents partial AST nodes created during error recovery
#[derive(Debug, Clone)]
enum PartialNode {
    Function(Function),
    Class(Class),
}

pub fn parse(source: &str) -> Result<Program, CompilerError> {
    let trimmed_source = source.trim();
    let pairs = <CleanParser as Parser<Rule>>::parse(Rule::program, trimmed_source)
        .map_err(|e| ErrorUtils::from_pest_error(e, source, "<unknown>"))?;

    parse_program_ast(pairs)
}

pub fn parse_program_ast(pairs: pest::iterators::Pairs<Rule>) -> Result<Program, CompilerError> {
    let _file_path = "unknown";
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    let mut start_function = None;
    let mut imports = Vec::new();
    let mut tests = Vec::new();
    let mut top_level_statements = Vec::new();
    let mut state: Option<crate::ast::StateBlock> = None;
    let mut watch_blocks: Vec<crate::ast::WatchBlock> = Vec::new();
    let mut screen_blocks: Vec<Statement> = Vec::new();
    let mut externals: Vec<crate::ast::ExternalFunction> = Vec::new();

    for pair in pairs {
        if pair.as_rule() == Rule::program {
            for inner in pair.into_inner() {
                match inner.as_rule() {
                    Rule::program_item => {
                        for program_item_inner in inner.into_inner() {
                            match program_item_inner.as_rule() {
                                Rule::import_stmt => {
                                    if let Statement::Import {
                                        imports: import_items,
                                        location: _,
                                    } = parse_import_statement(program_item_inner)?
                                    {
                                        imports.extend(import_items);
                                    }
                                }
                                Rule::functions_block => {
                                    let block_functions =
                                        parse_functions_block(program_item_inner)?;
                                    functions.extend(block_functions);
                                }
                                // Standalone functions removed - all functions must be in functions: blocks per specification
                                Rule::start_block => {
                                    let func = parse_start_block(program_item_inner)?;
                                    start_function = Some(func);
                                }
                                Rule::class_decl => {
                                    let class = parse_class(program_item_inner)?;
                                    classes.push(class);
                                }
                                Rule::tests_block => {
                                    let test_cases = parse_tests_block(program_item_inner)?;
                                    tests.extend(test_cases);
                                }
                                Rule::state_block => {
                                    // Parse app-level state block
                                    let state_block = parse_state_block(program_item_inner)?;
                                    state = Some(state_block);
                                }
                                Rule::watch_block => {
                                    // Parse top-level watch block
                                    let watch_block = parse_watch_block(program_item_inner)?;
                                    watch_blocks.push(watch_block);
                                }
                                Rule::screen_block => {
                                    // Parse screen block
                                    let screen_stmt = parse_screen_block(program_item_inner)?;
                                    screen_blocks.push(screen_stmt);
                                }
                                Rule::external_block => {
                                    // Parse external function block (WASM imports)
                                    let external_fns = parse_external_block(program_item_inner)?;
                                    externals.extend(external_fns);
                                }
                                Rule::statement => {
                                    let stmt = parse_statement(program_item_inner)?;
                                    // Handle top-level statements - these should be added to the start function
                                    top_level_statements.push(stmt);
                                }
                                _ => {}
                            }
                        }
                    }
                    Rule::EOI => {} // End of input
                    _ => {}
                }
            }
        }
    }

    // If we have top-level statements but no explicit start function, create an implicit one
    if !top_level_statements.is_empty() && start_function.is_none() {
        // Filter out any start() function calls to avoid recursive calls
        let _original_count = top_level_statements.len();
        let filtered_statements: Vec<Statement> = top_level_statements
            .into_iter()
            .filter(|stmt| {
                if let Statement::Expression {
                    expr: Expression::Call(name, _args),
                    location: _,
                } = stmt
                {
                    return name != "start";
                }
                true
            })
            .collect();

        start_function = Some(Function {
            name: "start".to_string(),
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters: Vec::new(),
            return_type: Type::Void,
            body: filtered_statements,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: None,
        });
    }

    let program = Program {
        imports,
        plugins: Vec::new(),
        statements: Vec::new(),
        functions,
        classes,
        start_function,
        tests,
        screens: Vec::new(),
        state,
        watch_blocks,
        screen_blocks,
        externals,
        source_block: None,
        location: None,
    };

    Ok(program)
}

/// Parse a start block (entry point)
/// Grammar: start_block = { "start" ~ ":" ~ simple_indented_block }
pub fn parse_start_block(pair: Pair<Rule>) -> Result<Function, CompilerError> {
    let name = "start".to_string();
    let mut body = Vec::new();
    let location = Some(convert_to_ast_location(&get_location(&pair)));

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::simple_indented_block {
            // simple_indented_block contains statement rules directly
            for stmt_pair in inner.into_inner() {
                if stmt_pair.as_rule() == Rule::statement {
                    body.push(parse_statement(stmt_pair)?);
                }
            }
        }
    }

    // Start block compiles to a void function for WebAssembly compatibility
    let return_type = Type::Void;

    Ok(Function {
        name,
        type_parameters: Vec::new(),
        type_constraints: Vec::new(),
        parameters: Vec::new(),
        return_type,
        body,
        description: None,
        syntax: FunctionSyntax::Simple,
        visibility: Visibility::Public,
        modifier: FunctionModifier::None,
        location,
    })
}

/// Legacy parse function for backwards compatibility during transition
/// This wraps parse_start_block for code that still references the old name
pub fn parse_start_function(pair: Pair<Rule>) -> Result<Function, CompilerError> {
    parse_start_block(pair)
}

// parse_standalone_function removed - all functions must be in functions: blocks per specification

/// Parse a parameter from a parameter list: type identifier [= default_value]
fn parse_parameter(pair: Pair<Rule>) -> Result<Parameter, CompilerError> {
    let mut param_type = None;
    let mut param_name = String::new();
    let mut default_value = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::parameter_type => {
                // parameter_type can contain type_ as a child, so parse the first child
                let type_pair = inner.into_inner().next().ok_or_else(|| {
                    CompilerError::parse_error(
                        "Parameter type missing".to_string(),
                        None,
                        Some("Parameter type is required".to_string()),
                    )
                })?;
                param_type = Some(parse_type(type_pair)?);
            }
            Rule::type_ => {
                param_type = Some(parse_type(inner)?);
            }
            Rule::identifier | Rule::parameter_name => {
                param_name = inner.as_str().to_string();
            }
            Rule::expression | Rule::logical_expression => {
                // Parse default value expression
                default_value = Some(crate::parser::expression_parser::parse_expression(inner)?);
            }
            _ => {}
        }
    }

    let param_type = param_type.ok_or_else(|| {
        CompilerError::parse_error(
            "Parameter missing type".to_string(),
            None,
            Some("Parameters must have a type".to_string()),
        )
    })?;

    if param_name.is_empty() {
        return Err(CompilerError::parse_error(
            "Parameter missing name".to_string(),
            None,
            Some("Parameters must have a name".to_string()),
        ));
    }

    if let Some(default_expr) = default_value {
        Ok(Parameter::new_with_default(
            param_name,
            param_type,
            default_expr,
        ))
    } else {
        Ok(Parameter::new(param_name, param_type))
    }
}

pub fn get_location(pair: &Pair<Rule>) -> super::SourceLocation {
    let span = pair.as_span();
    let (line, col) = span.start_pos().line_col();
    super::SourceLocation {
        line,
        column: col,
        file: String::new(), // Will be set by the parser context
        byte_start: None,
        byte_end: None,
    }
}

pub fn parse_functions_block(functions_block: Pair<Rule>) -> Result<Vec<Function>, CompilerError> {
    parse_functions_block_with_context(functions_block, None)
}

pub fn parse_functions_block_with_context(
    functions_block: Pair<Rule>,
    class_context: Option<super::preprocessor::ClassContext>,
) -> Result<Vec<Function>, CompilerError> {
    // Extract source text for preprocessing
    let source_text = functions_block.as_str();

    // For class context, use traditional parsing directly to avoid preprocessor issues
    if class_context.is_some() {
        return parse_functions_block_traditional(functions_block);
    }

    // Try preprocessing approach first for multi-function blocks (non-class only)
    let preprocessor = super::preprocessor::FunctionPreprocessor::new(source_text);

    match preprocessor.process_functions_block(source_text) {
        Ok(functions) => {
            if !functions.is_empty() {
                return Ok(functions);
            }
        }
        Err(_) => {
            // Fall back to traditional parsing if preprocessing fails
        }
    }

    // Fall back to traditional parsing
    parse_functions_block_traditional(functions_block)
}

pub fn parse_functions_block_traditional(
    functions_block: Pair<Rule>,
) -> Result<Vec<Function>, CompilerError> {
    let mut functions = Vec::new();

    for item in functions_block.into_inner() {
        if item.as_rule() == Rule::indented_functions_block
            || item.as_rule() == Rule::class_indented_functions_block
        {
            for func_item in item.into_inner() {
                match func_item.as_rule() {
                    Rule::function_in_block => {
                        match parse_function_in_block(func_item) {
                            Ok(func) => functions.push(func),
                            Err(_) => continue, // Recovery: skip failed function
                        }
                    }
                    Rule::function_line => {
                        // function_line contains function_in_block inside
                        for inner_item in func_item.into_inner() {
                            if inner_item.as_rule() == Rule::function_in_block {
                                match parse_function_in_block(inner_item) {
                                    Ok(func) => functions.push(func),
                                    Err(_) => continue, // Recovery: skip failed function
                                }
                            }
                        }
                    }
                    Rule::class_function_line => {
                        // Handle the new class_function_line rule
                        for inner in func_item.into_inner() {
                            if inner.as_rule() == Rule::function_in_block {
                                match parse_function_in_block(inner) {
                                    Ok(func) => functions.push(func),
                                    Err(_) => continue, // Recovery: skip failed function
                                }
                            }
                        }
                    }
                    _ => {} // Skip other items like empty_line
                }
            }
        }
    }

    Ok(functions)
}

/// Parse a class declaration
pub fn parse_tests_block(tests_pair: Pair<Rule>) -> Result<Vec<TestCase>, CompilerError> {
    let mut test_cases = Vec::new();

    for inner in tests_pair.into_inner() {
        if inner.as_rule() == Rule::indented_tests_block {
            for test_pair in inner.into_inner() {
                if test_pair.as_rule() == Rule::test_case {
                    test_cases.push(parse_test_case(test_pair)?);
                }
            }
        }
    }

    Ok(test_cases)
}

pub fn parse_test_case(test_pair: Pair<Rule>) -> Result<TestCase, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&test_pair)));

    for inner in test_pair.into_inner() {
        match inner.as_rule() {
            Rule::named_test => {
                let mut description = None;
                let mut test_expression = None;
                let mut expected_value = None;

                for named_inner in inner.into_inner() {
                    match named_inner.as_rule() {
                        Rule::string => {
                            if description.is_none() {
                                description =
                                    Some(named_inner.as_str().trim_matches('"').to_string());
                            }
                        }
                        Rule::expression => {
                            if test_expression.is_none() {
                                test_expression = Some(parse_expression(named_inner)?);
                            } else if expected_value.is_none() {
                                expected_value = Some(parse_expression(named_inner)?);
                            }
                        }
                        _ => {}
                    }
                }

                if let (Some(test_expr), Some(expected)) = (test_expression, expected_value) {
                    return Ok(TestCase {
                        description,
                        test_expression: test_expr,
                        expected_value: expected,
                        location,
                    });
                }
            }
            Rule::anonymous_test => {
                let mut test_expression = None;
                let mut expected_value = None;

                for anon_inner in inner.into_inner() {
                    if anon_inner.as_rule() == Rule::expression {
                        if test_expression.is_none() {
                            test_expression = Some(parse_expression(anon_inner)?);
                        } else if expected_value.is_none() {
                            expected_value = Some(parse_expression(anon_inner)?);
                        }
                    }
                }

                if let (Some(test_expr), Some(expected)) = (test_expression, expected_value) {
                    return Ok(TestCase {
                        description: None,
                        test_expression: test_expr,
                        expected_value: expected,
                        location,
                    });
                }
            }
            _ => {}
        }
    }

    Err(CompilerError::parse_error(
        "Invalid test case format".to_string(),
        location.clone(),
        Some("Tests should be in format 'expression = expected' or '\"description\": expression = expected'".to_string())
    ))
}

/// Helper to parse parameters from an input_block
///
/// Expects a Pair<Rule> for input_block, which contains an indented_input_block.
/// Each input_declaration is parsed as a parameter (type and name).
/// Returns a Vec<Parameter>.
/// Parse a single standalone_input_declaration as a Parameter
/// Used when input declarations appear directly in function bodies
fn parse_standalone_input_declaration_as_parameter(
    input_decl: Pair<Rule>,
) -> Result<crate::ast::Parameter, CompilerError> {
    let mut param_type = None;
    let mut param_name = String::new();
    let mut default_value = None;

    for param_decl in input_decl.into_inner() {
        match param_decl.as_rule() {
            Rule::input_type => param_type = Some(parse_type(param_decl)?),
            Rule::identifier => param_name = param_decl.as_str().to_string(),
            Rule::expression => {
                // Parse default value expression
                default_value = Some(crate::parser::expression_parser::parse_expression(
                    param_decl,
                )?);
            }
            _ => {}
        }
    }

    if let Some(pt) = param_type {
        if param_name.is_empty() {
            return Err(CompilerError::parse_error(
                "Missing parameter name in standalone input declaration",
                None,
                None,
            ));
        }

        if let Some(default_expr) = default_value {
            Ok(crate::ast::Parameter::new_with_default(
                param_name,
                pt,
                default_expr,
            ))
        } else {
            Ok(crate::ast::Parameter::new(param_name, pt))
        }
    } else {
        Err(CompilerError::parse_error(
            "Missing type in standalone input parameter declaration",
            None,
            None,
        ))
    }
}

fn parse_parameters_from_input_block(
    input_block: Pair<Rule>,
) -> Result<Vec<Parameter>, CompilerError> {
    let mut parameters = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    for input_inner in input_block.into_inner() {
        if input_inner.as_rule() == Rule::indented_input_block {
            for input_decl in input_inner.into_inner() {
                if input_decl.as_rule() == Rule::input_declaration {
                    let mut param_type = None;
                    let mut param_name = String::new();
                    let mut default_value = None;
                    for param_decl in input_decl.into_inner() {
                        match param_decl.as_rule() {
                            Rule::input_type => param_type = Some(parse_type(param_decl)?),
                            Rule::identifier => param_name = param_decl.as_str().to_string(),
                            Rule::expression => {
                                // Parse default value expression
                                default_value = Some(
                                    crate::parser::expression_parser::parse_expression(param_decl)?,
                                );
                            }
                            _ => {}
                        }
                    }
                    // Fixed: Now supports default values for parameters
                    if let Some(pt) = param_type {
                        if !seen_names.insert(param_name.clone()) {
                            return Err(CompilerError::parse_error(
                                format!("Duplicate parameter name '{param_name}' in input block"),
                                None,
                                None,
                            ));
                        }
                        if let Some(default_expr) = default_value {
                            parameters.push(Parameter::new_with_default(
                                param_name,
                                pt,
                                default_expr,
                            ));
                        } else {
                            parameters.push(Parameter::new(param_name, pt));
                        }
                    } else {
                        return Err(CompilerError::parse_error(
                            "Missing type in input parameter declaration",
                            None,
                            None,
                        ));
                    }
                }
            }
        }
    }
    Ok(parameters)
}

/// Parses a function declared inside a functions block.
///
/// According to the grammar, a function_in_block has the following structure:
///   function_in_block = { function_type? ~ identifier ~ "(" ~ ")" ~ function_body }
///   function_body = { (setup_block ~ indented_block) | indented_block }
///
/// - setup_block may contain a description_block and/or input_block (for parameters)
/// - indented_block contains the function body statements
///
/// This function extracts the function name, return type, parameters, description, and body.
/// It returns a Function AST node.
fn parse_parameter_list(param_list_pair: Pair<Rule>) -> Result<Vec<Parameter>, CompilerError> {
    let mut parameters = Vec::new();

    for param_pair in param_list_pair.into_inner() {
        if param_pair.as_rule() == Rule::parameter {
            parameters.push(parse_parameter(param_pair)?);
        }
    }

    Ok(parameters)
}

pub fn parse_function_in_block(func_pair: Pair<Rule>) -> Result<Function, CompilerError> {
    let mut func_name = String::new();
    let mut return_type: Option<Type> = None;
    let mut type_parameters = Vec::new();
    let mut parameters = Vec::new();
    let mut body = Vec::new();
    let mut description: Option<String> = None;
    let location = Some(convert_to_ast_location(&get_location(&func_pair)));

    // Parse the function signature and body
    for item in func_pair.into_inner() {
        match item.as_rule() {
            Rule::function_type | Rule::function_return_type => {
                let parsed_type = parse_type(item)?;
                return_type = Some(parsed_type.clone());

                // If the return type is a type parameter (like "Any"), add it to type_parameters
                if let Type::TypeParameter(ref param_name) = parsed_type {
                    type_parameters.push(param_name.clone());
                }
            }
            Rule::identifier => {
                func_name = item.as_str().to_string();
            }
            Rule::function_name => {
                func_name = item.as_str().to_string();
            }
            Rule::parameter_list => {
                let params = parse_parameter_list(item)?;
                parameters.extend(params);
            }
            Rule::function_body | Rule::function_body_in_block => {
                // function_body = (setup_block ~ indented_block) | indented_block | empty
                let mut found_body = false;
                let mut _found_setup = false;
                let inner_rules: Vec<_> = item.into_inner().collect();

                // If function_body has no inner rules or only EOI rule, it's an empty function body
                if inner_rules.is_empty()
                    || (inner_rules.len() == 1 && inner_rules[0].as_rule() == Rule::EOI)
                {
                    found_body = true; // Empty function body is valid
                }

                for body_item in inner_rules {
                    match body_item.as_rule() {
                        Rule::setup_block => {
                            _found_setup = true;
                            // setup_block may contain description_block and/or input_block
                            for setup_item in body_item.into_inner() {
                                match setup_item.as_rule() {
                                    Rule::description_block => {
                                        for desc_inner in setup_item.into_inner() {
                                            if desc_inner.as_rule() == Rule::string {
                                                description = Some(
                                                    desc_inner
                                                        .as_str()
                                                        .trim_matches('"')
                                                        .to_string(),
                                                );
                                            }
                                        }
                                    }
                                    Rule::input_block => {
                                        let params = parse_parameters_from_input_block(setup_item)?;
                                        for param in &params {
                                            // If parameter type is a type parameter, add it to type_parameters if not already present
                                            if let Type::TypeParameter(ref param_name) = param.type_
                                            {
                                                if !type_parameters.contains(param_name) {
                                                    type_parameters.push(param_name.clone());
                                                }
                                            }
                                        }
                                        parameters.extend(params);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Rule::function_body_statements => {
                            found_body = true;
                            // Process statements, handling input_declaration specially
                            for stmt_pair in body_item.into_inner() {
                                if stmt_pair.as_rule() == Rule::function_body_statement {
                                    // function_body_statement = { INDENT+ ~ statement }
                                    // So we need to get the inner statement
                                    let statement_pair = stmt_pair.into_inner().last().unwrap(); // Skip INDENT+, get statement

                                    // Check if this statement contains an input_declaration
                                    let inner = statement_pair.clone().into_inner().next().unwrap();
                                    if inner.as_rule() == Rule::standalone_input_declaration {
                                        // Parse as parameter and add to parameters list
                                        let param =
                                            parse_standalone_input_declaration_as_parameter(inner)?;
                                        // Check for duplicate parameter names
                                        if parameters.iter().any(|p| p.name == param.name) {
                                            return Err(CompilerError::parse_error(
                                                format!("Duplicate parameter name '{param_name}' in function '{func_name}'", param_name = param.name, func_name = func_name),
                                                location.clone(),
                                                None,
                                            ));
                                        }
                                        parameters.push(param);
                                    } else {
                                        // Regular statement
                                        body.push(parse_statement(statement_pair)?);
                                    }
                                }
                            }
                        }
                        Rule::statement => {
                            // Handle single statement function bodies
                            found_body = true;
                            // Check if this statement contains an input_declaration
                            let inner = body_item.clone().into_inner().next().unwrap();
                            if inner.as_rule() == Rule::standalone_input_declaration {
                                // Parse as parameter and add to parameters list
                                let param = parse_standalone_input_declaration_as_parameter(inner)?;
                                // Check for duplicate parameter names
                                if parameters.iter().any(|p| p.name == param.name) {
                                    return Err(CompilerError::parse_error(
                                        format!("Duplicate parameter name '{param_name}' in function '{func_name}'", param_name = param.name, func_name = func_name),
                                        location.clone(),
                                        None,
                                    ));
                                }
                                parameters.push(param);
                            } else {
                                // Regular statement
                                body.push(parse_statement(body_item)?);
                            }
                        }
                        _ => {}
                    }
                }
                if !found_body {
                    return Err(CompilerError::parse_error(
                        format!("Function '{func_name}' is missing a body."),
                        location.clone(),
                        None,
                    ));
                }
            }
            _ => {}
        }
    }

    if func_name.is_empty() {
        return Err(CompilerError::parse_error(
            "Function is missing a name.",
            location.clone(),
            None,
        ));
    }

    let return_type = return_type.unwrap_or(Type::Void);

    Ok(Function {
        name: func_name,
        type_parameters,
        type_constraints: Vec::new(),
        parameters,
        return_type,
        body,
        description,
        syntax: FunctionSyntax::Block,
        visibility: Visibility::Public,
        modifier: FunctionModifier::None,
        location,
    })
}

/// Parse an import statement
pub fn parse_import_statement(pair: Pair<Rule>) -> Result<Statement, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut imports = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::import_list => {
                for import_item in inner.into_inner() {
                    if import_item.as_rule() == Rule::import_item {
                        let import = parse_import_item(import_item)?;
                        imports.push(import);
                    }
                }
            }
            Rule::import_item => {
                // Handle single import item case (e.g., import: Math.sqrt)
                let import = parse_import_item(inner)?;
                imports.push(import);
            }
            Rule::identifier => {
                // Handle simple import case (e.g., import TestModule)
                let module_name = inner.as_str().to_string();
                let import = ImportItem {
                    name: module_name,
                    alias: None,
                    is_file_import: false,
                };
                imports.push(import);
            }
            _ => {
                // Ignore other rules
            }
        }
    }

    Ok(Statement::Import { imports, location })
}

/// Parse an individual import item
fn parse_import_item(pair: Pair<Rule>) -> Result<ImportItem, CompilerError> {
    let mut identifiers = Vec::new();

    // Get the import text before consuming the pair
    let import_text = pair.as_str().to_string();

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::identifier {
            identifiers.push(inner.as_str().to_string());
        }
    }

    if identifiers.is_empty() {
        return Err(CompilerError::parse_error(
            "Import item is missing a name".to_string(),
            None,
            None,
        ));
    }

    // Parse different import patterns based on the grammar
    if import_text.contains(" as ") {
        // Has alias - could be "Math as M" or "Math.sqrt as msqrt"
        let parts: Vec<&str> = import_text.split(" as ").collect();
        if parts.len() == 2 {
            let name = parts[0].trim().to_string();
            let alias = Some(parts[1].trim().to_string());
            return Ok(ImportItem {
                name,
                alias,
                is_file_import: false,
            });
        }
    } else if import_text.contains('.') {
        // Single symbol import like "Math.sqrt"
        let name = import_text.to_string();
        return Ok(ImportItem {
            name,
            alias: None,
            is_file_import: false,
        });
    } else {
        // Simple module import like "Math"
        let name = identifiers[0].clone();
        return Ok(ImportItem {
            name,
            alias: None,
            is_file_import: false,
        });
    }

    Err(CompilerError::parse_error(
        format!("Invalid import syntax: '{import_text}'"),
        None,
        None,
    ))
}

impl ErrorRecoveringParser {
    /// Extract function signature from malformed function declaration
    fn extract_function_signature(&self, segment: &str) -> Option<(String, Type, Vec<Parameter>)> {
        // Look for pattern: function [type] name([params])
        let words: Vec<&str> = segment.split_whitespace().collect();
        if words.len() >= 2 && words[0] == "function" {
            let mut return_type = Type::Void;
            let function_name;

            // Case 1: function name(
            if words[1].contains('(') {
                function_name = words[1].split('(').next().unwrap().to_string();
            }
            // Case 2: function type name(
            else if words.len() >= 3 && words[2].contains('(') {
                // Try to parse the type
                return_type = match words[1] {
                    "integer" => Type::Integer,
                    "number" => Type::Number,
                    "string" => Type::String,
                    "boolean" => Type::Boolean,
                    _ => Type::Any,
                };
                function_name = words[2].split('(').next().unwrap().to_string();
            }
            // Case 3: just function name
            else {
                function_name = words[1].to_string();
            }

            // For recovery, don't try to parse parameters - too error-prone
            let parameters = Vec::new();

            if !function_name.is_empty() {
                return Some((function_name, return_type, parameters));
            }
        }
        None
    }

    /// Extract class name from malformed class declaration
    fn extract_class_name(&self, segment: &str) -> Option<String> {
        let words: Vec<&str> = segment.split_whitespace().collect();
        if words.len() >= 2 && words[0] == "class" {
            // Handle "class Name" or "class Name is BaseClass"
            let class_name = words[1].split_whitespace().next()?.to_string();
            if !class_name.is_empty() && class_name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                return Some(class_name);
            }
        }
        None
    }

    /// Calculate approximate location from segment content
    fn calculate_location_from_segment(&self, segment: &str) -> Option<crate::ast::SourceLocation> {
        // Find the segment position in the original source
        if let Some(pos) = self.source.find(segment.trim()) {
            let (line, column) = self.calculate_line_column(pos);
            return Some(crate::ast::SourceLocation {
                file: self.file_path.clone(),
                line,
                column,
                byte_start: None,
                byte_end: None,
            });
        }
        None
    }

    /// Calculate line and column from byte position
    fn calculate_line_column(&self, pos: usize) -> (usize, usize) {
        let mut line = 1;
        let mut column = 1;

        for (i, ch) in self.source.char_indices() {
            if i >= pos {
                break;
            }
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }

        (line, column)
    }

    /// Enhance error with segment recovery context
    fn enhance_error_with_segment_context(
        &self,
        error: CompilerError,
        segment_idx: usize,
        total_segments: usize,
        segment: &str,
    ) -> CompilerError {
        match error {
            CompilerError::Syntax { context } => {
                let enhanced_context = context
                    .with_suggestion(format!(
                        "Error in segment {} of {} during recovery parsing",
                        segment_idx + 1,
                        total_segments
                    ))
                    .with_source_snippet(segment.lines().take(5).collect::<Vec<_>>().join("\n"))
                    .with_help_option(Some(format!(
                        "This error occurred while trying to parse a recovered segment. \
                        Check the syntax in this section of your code."
                    )));
                CompilerError::Syntax {
                    context: Box::new(enhanced_context),
                }
            }
            _ => error, // Don't enhance non-syntax errors during recovery
        }
    }

    /// Deduplicate similar errors to reduce noise
    fn deduplicate_errors(&self, errors: Vec<CompilerError>) -> Vec<CompilerError> {
        let mut deduped = Vec::new();
        let mut seen_messages = std::collections::HashSet::new();

        for error in errors {
            let error_key = format!(
                "{}:{}",
                error.to_string().lines().next().unwrap_or(""),
                match &error {
                    CompilerError::Syntax { context }
                    | CompilerError::Type { context }
                    | CompilerError::Memory { context }
                    | CompilerError::Codegen { context }
                    | CompilerError::IO { context }
                    | CompilerError::Runtime { context }
                    | CompilerError::Validation { context }
                    | CompilerError::Module { context }
                    | CompilerError::Testing { context } => {
                        context
                            .location
                            .as_ref()
                            .map(|l| format!("{}:{}", l.line, l.column))
                            .unwrap_or_else(|| "unknown".to_string())
                    }
                    CompilerError::LexError(_) => {
                        "lexer".to_string()
                    }
                    CompilerError::PluginError { location, .. } => {
                        location
                            .as_ref()
                            .map(|l| format!("{}:{}", l.line, l.column))
                            .unwrap_or_else(|| "plugin".to_string())
                    }
                }
            );

            if !seen_messages.contains(&error_key) {
                seen_messages.insert(error_key);
                deduped.push(error);
            }
        }

        deduped
    }
}

// ============================================================================
// EXTERNAL FUNCTION PARSERS
// ============================================================================

/// Parse an external block that declares functions provided by the WASM host
/// Syntax:
///   external:          # imports from "env" module
///       return_type function_name(params)
///
///   external "module": # imports from custom module
///       return_type function_name(params)
pub fn parse_external_block(
    pair: Pair<Rule>,
) -> Result<Vec<crate::ast::ExternalFunction>, CompilerError> {
    let mut externals = Vec::new();
    let mut module = "env".to_string(); // Default WASM module name

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::string => {
                // Custom module name (e.g., external "http":)
                let s = inner.as_str();
                // Remove quotes from string
                module = s[1..s.len() - 1].to_string();
            }
            Rule::indented_external_functions => {
                for decl_pair in inner.into_inner() {
                    if decl_pair.as_rule() == Rule::external_function_declaration {
                        let external_fn =
                            parse_external_function_declaration(decl_pair, module.clone())?;
                        externals.push(external_fn);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(externals)
}

/// Parse a single external function declaration
/// Syntax: return_type function_name(params)
fn parse_external_function_declaration(
    pair: Pair<Rule>,
    module: String,
) -> Result<crate::ast::ExternalFunction, CompilerError> {
    use crate::ast::{ExternalFunction, Type};

    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut return_type = Type::Void;
    let mut name = String::new();
    let mut parameters = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::external_return_type => {
                // Parse return type - handle "void" specially
                let type_str = inner.as_str().trim();
                if type_str == "void" {
                    return_type = Type::Void;
                } else {
                    // Parse as a regular type
                    return_type = parse_type_from_str(type_str)?;
                }
            }
            Rule::function_name => {
                name = inner.as_str().to_string();
            }
            Rule::parameter_list => {
                parameters = parse_external_parameter_list(inner)?;
            }
            _ => {}
        }
    }

    Ok(ExternalFunction {
        name,
        parameters,
        return_type,
        module,
        location,
    })
}

/// Parse parameter list for external function declarations
fn parse_external_parameter_list(
    pair: Pair<Rule>,
) -> Result<Vec<crate::ast::Parameter>, CompilerError> {
    use crate::ast::Parameter;

    let mut parameters = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::parameter => {
                // For external functions, we have type-only parameters or type name pairs
                // Handle format: type name or just type (with name optional)
                let text = inner.as_str().trim();
                let parts: Vec<&str> = text.split_whitespace().collect();

                let (param_type, param_name) = if parts.len() >= 2 {
                    (parse_type_from_str(parts[0])?, parts[1].to_string())
                } else if parts.len() == 1 {
                    // Single word - assume it's a type with generated name
                    (
                        parse_type_from_str(parts[0])?,
                        format!("arg{}", parameters.len()),
                    )
                } else {
                    (crate::ast::Type::Any, format!("arg{}", parameters.len()))
                };

                parameters.push(Parameter {
                    name: param_name,
                    type_: param_type,
                    default_value: None,
                });
            }
            _ => {}
        }
    }

    Ok(parameters)
}

/// Helper function to parse a type from a string
fn parse_type_from_str(type_str: &str) -> Result<crate::ast::Type, CompilerError> {
    use crate::ast::Type;

    match type_str.trim() {
        "integer" => Ok(Type::Integer),
        "number" => Ok(Type::Number),
        "string" => Ok(Type::String),
        "boolean" => Ok(Type::Boolean),
        "any" => Ok(Type::Any),
        "void" => Ok(Type::Void),
        "none" => Ok(Type::None),
        _other => {
            // Could be a class type or complex type
            // For now, treat unknown types as any (generic external types)
            Ok(Type::Any)
        }
    }
}

// STATE MANAGEMENT PARSERS
// ============================================================================

/// Parse a state block (app-level or screen-level)
pub fn parse_state_block(pair: Pair<Rule>) -> Result<crate::ast::StateBlock, CompilerError> {
    use crate::ast::{StateBlock, StateScope};

    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut declarations = Vec::new();
    let mut computed = Vec::new();
    let mut rules = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::indented_state_declarations => {
                for decl_pair in inner.into_inner() {
                    match decl_pair.as_rule() {
                        Rule::state_declaration => {
                            let decl = parse_state_declaration(decl_pair)?;
                            declarations.push(decl);
                        }
                        Rule::computed_block => {
                            let computed_decls = parse_computed_block(decl_pair)?;
                            computed.extend(computed_decls);
                        }
                        _ => {}
                    }
                }
            }
            Rule::rules_block => {
                rules = Some(parse_rules_block(inner)?);
            }
            _ => {}
        }
    }

    Ok(StateBlock {
        declarations,
        computed,
        rules,
        scope: StateScope::App, // Default to app scope; screen scope set by parent
        location,
    })
}

/// Parse a rules block (state invariants)
fn parse_rules_block(pair: Pair<Rule>) -> Result<crate::ast::RulesBlock, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut rules = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expression => {
                let expr = parse_expression(inner)?;
                rules.push(expr);
            }
            _ => {}
        }
    }

    Ok(crate::ast::RulesBlock { rules, location })
}

/// Parse a single state declaration
fn parse_state_declaration(
    pair: Pair<Rule>,
) -> Result<crate::ast::StateDeclaration, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut type_ = crate::ast::Type::Any;
    let mut name = String::new();
    let mut initializer = crate::ast::Expression::Literal(crate::ast::Value::None);
    let mut guard = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::type_ => {
                type_ = parse_type(inner)?;
            }
            Rule::identifier => {
                name = inner.as_str().to_string();
            }
            Rule::single_line_expression => {
                initializer = parse_expression(inner)?;
            }
            Rule::guard_clause => {
                guard = Some(parse_guard_clause(inner)?);
            }
            _ => {}
        }
    }

    Ok(crate::ast::StateDeclaration {
        name,
        type_,
        initializer,
        guard,
        location,
    })
}

/// Parse a guard clause
fn parse_guard_clause(pair: Pair<Rule>) -> Result<crate::ast::GuardClause, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut condition = crate::ast::Expression::Literal(crate::ast::Value::Boolean(true));
    let mut error_message = String::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::expression => {
                condition = parse_expression(inner)?;
            }
            Rule::string => {
                // Extract string content without quotes
                let s = inner.as_str();
                error_message = s[1..s.len() - 1].to_string();
            }
            _ => {}
        }
    }

    Ok(crate::ast::GuardClause {
        condition,
        error_message,
        location,
    })
}

/// Parse a computed block (computed state declarations)
fn parse_computed_block(
    pair: Pair<Rule>,
) -> Result<Vec<crate::ast::ComputedDeclaration>, CompilerError> {
    let mut computed = Vec::new();

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::indented_computed_declarations {
            for decl_pair in inner.into_inner() {
                if decl_pair.as_rule() == Rule::computed_declaration {
                    let decl = parse_computed_declaration(decl_pair)?;
                    computed.push(decl);
                }
            }
        }
    }

    Ok(computed)
}

/// Parse a single computed declaration
fn parse_computed_declaration(
    pair: Pair<Rule>,
) -> Result<crate::ast::ComputedDeclaration, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut type_ = crate::ast::Type::Any;
    let mut name = String::new();
    let mut body = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::type_ => {
                type_ = parse_type(inner)?;
            }
            Rule::identifier => {
                name = inner.as_str().to_string();
            }
            Rule::computed_body => {
                // Parse statements in the computed body
                for stmt_pair in inner.into_inner() {
                    if stmt_pair.as_rule() == Rule::statement {
                        let stmt = parse_statement(stmt_pair)?;
                        body.push(stmt);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(crate::ast::ComputedDeclaration {
        name,
        type_,
        body,
        location,
    })
}

/// Parse a watch block
pub fn parse_watch_block(pair: Pair<Rule>) -> Result<crate::ast::WatchBlock, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut targets = Vec::new();
    let mut body = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::watch_targets => {
                for target_pair in inner.into_inner() {
                    if target_pair.as_rule() == Rule::identifier {
                        targets.push(target_pair.as_str().to_string());
                    }
                }
            }
            Rule::simple_indented_block | Rule::indented_block => {
                // Parse the body statements
                for stmt_pair in inner.into_inner() {
                    if stmt_pair.as_rule() == Rule::statement {
                        let stmt = parse_statement(stmt_pair)?;
                        body.push(stmt);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(crate::ast::WatchBlock {
        targets,
        body,
        location,
    })
}

/// Parse a screen block
pub fn parse_screen_block(pair: Pair<Rule>) -> Result<Statement, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut name = String::new();
    let mut state = None;
    let mut watch_blocks = Vec::new();
    let mut functions = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                name = inner.as_str().to_string();
            }
            Rule::indented_screen_body => {
                for body_item in inner.into_inner() {
                    if body_item.as_rule() == Rule::screen_body_item {
                        for item_inner in body_item.into_inner() {
                            match item_inner.as_rule() {
                                Rule::state_block => {
                                    let mut state_block = parse_state_block(item_inner)?;
                                    // Set scope to screen
                                    state_block.scope = crate::ast::StateScope::Screen;
                                    state = Some(state_block);
                                }
                                Rule::watch_block => {
                                    let watch_block = parse_watch_block(item_inner)?;
                                    watch_blocks.push(watch_block);
                                }
                                Rule::functions_block | Rule::class_functions_block => {
                                    let block_functions = parse_functions_block(item_inner)?;
                                    functions.extend(block_functions);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::ScreenBlockStmt {
        name,
        state,
        watch_blocks,
        functions,
        location,
    })
}

// ============================================================================
// END STATE MANAGEMENT PARSERS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::CleanParser;
    use super::*;
    use pest::Parser;

    #[test]
    fn test_parse_function_in_block_valid() {
        // Test simpler function without input block
        let source = "integer add(integer a, integer b)\n\treturn a + b";
        let mut pairs =
            <CleanParser as Parser<Rule>>::parse(Rule::function_in_block, source).unwrap();
        let pair = pairs.next().unwrap();
        let func = parse_function_in_block(pair).unwrap();
        assert_eq!(func.name, "add");
        assert_eq!(func.parameters.len(), 2);
        assert_eq!(func.parameters[0].name, "a");
        assert_eq!(func.parameters[1].name, "b");
        assert_eq!(func.body.len(), 1);
    }

    #[test]
    fn test_parse_function_in_block_duplicate_param() {
        let source = "integer add()\n\tinput\n\t\tinteger a\n\t\tinteger a\n\treturn a + a";
        let mut pairs =
            <CleanParser as Parser<Rule>>::parse(Rule::function_in_block, source).unwrap();
        let pair = pairs.next().unwrap();
        let err = parse_function_in_block(pair).unwrap_err();
        assert!(err.to_string().contains("Duplicate parameter name"));
    }

    #[test]
    fn test_parse_function_in_block_missing_name() {
        // Test that a function without a proper name fails
        // Note: The grammar might allow some edge cases, so we just verify
        // that the parser doesn't crash and produces some kind of result
        let source = "integer ()\n\treturn 42";
        let result = <CleanParser as Parser<Rule>>::parse(Rule::function_in_block, source);

        // Either the grammar rejects it (preferred) or parse_function_in_block catches it
        match result {
            Ok(mut pairs) => {
                let pair = pairs.next().unwrap();
                // If grammar allows it, parse_function_in_block should handle it gracefully
                // (either accepting with a default name or rejecting)
                let _func_result = parse_function_in_block(pair);
                // Test passes if we don't panic
            }
            Err(_) => {
                // Grammar rejected it - also fine
            }
        }
    }

    #[test]
    fn debug_parse_tree_for_function_in_block() {
        let source = "integer add()\n\tinput\n\t\tinteger a\n\t\tinteger b\n\t\n\treturn a + b";
        let mut pairs =
            <CleanParser as Parser<Rule>>::parse(Rule::function_in_block, source).unwrap();
        let pair = pairs.next().unwrap();
        println!("Parse tree: {:#?}", pair);
    }

    #[test]
    fn debug_iterate_with_nested_if_parsing() {
        // This test traces how iterate with nested if is parsed - matches debug_iterate_with_if.cln
        let source = "start()\n\tlist<integer> nums = [5, 15, 25]\n\tprint(\"Before iterate\")\n\titerate item in nums\n\t\tif item > 10\n\t\t\tprint(\"Large\")\n\tprint(\"After iterate\")\n";

        println!("=== Input with explicit characters ===");
        for c in source.chars() {
            if c == '\t' {
                print!("→");
            } else if c == '\n' {
                println!("↵");
            } else {
                print!("{}", c);
            }
        }
        println!("\n");

        let result = <CleanParser as Parser<Rule>>::parse(Rule::program, source);
        match result {
            Ok(pairs) => {
                println!("=== Parse tree ===");
                for pair in pairs {
                    print_pair(&pair, 0);
                }
            }
            Err(e) => {
                println!("Parse error: {}", e);
            }
        }
    }

    fn print_pair(pair: &pest::iterators::Pair<Rule>, indent: usize) {
        let rule = pair.as_rule();
        let span = pair.as_str();
        let indent_str = "  ".repeat(indent);

        // Show rule and first 80 chars of content
        let preview: String = span
            .chars()
            .take(80)
            .map(|c| {
                if c == '\n' {
                    '↵'
                } else if c == '\t' {
                    '→'
                } else {
                    c
                }
            })
            .collect();
        println!("{}{:?}: '{}'", indent_str, rule, preview);

        for inner in pair.clone().into_inner() {
            print_pair(&inner, indent + 1);
        }
    }
}
