use super::class_parser::parse_class;
use super::expression_parser::parse_expression;
use super::statement_parser::parse_statement;
use super::type_parser::parse_type;
use super::Rule;
use super::{convert_to_ast_location, CleanParser};
use crate::ast::{
    Class, Constructor, Expression, Field, Function, FunctionModifier, FunctionSyntax, ImportItem,
    Parameter, Program, Statement, TestCase, Type, Visibility,
};
use crate::error::{CompilerError, ErrorUtils};
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
        }
    }

    pub fn with_max_errors(mut self, max_errors: usize) -> Self {
        self.max_errors = max_errors;
        self
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
                    Err(self.errors.clone())
                }
            }
            Err(mut parse_errors) => {
                // Merge any additional errors we collected during recovery
                parse_errors.extend(self.errors.clone());
                Err(parse_errors)
            }
        }
    }

    /// Identify synchronization points for error recovery
    fn identify_recovery_points(&mut self, source: &str) {
        let mut pos = 0;
        let chars: Vec<char> = source.chars().collect();

        while pos < chars.len() {
            // Look for function boundaries
            if pos + 8 < chars.len() && chars[pos..pos + 8].iter().collect::<String>() == "function"
            {
                self.recovery_points.push(pos);
            }

            // Look for class boundaries
            if pos + 5 < chars.len() && chars[pos..pos + 5].iter().collect::<String>() == "class" {
                self.recovery_points.push(pos);
            }

            // Look for statement boundaries (lines starting with tabs/spaces)
            if (pos == 0 || chars[pos - 1] == '\n')
                && pos < chars.len()
                && (chars[pos] == '\t' || chars[pos] == ' ')
            {
                self.recovery_points.push(pos);
            }

            pos += 1;
        }
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

        for segment in segments {
            match self.parse_segment(&segment) {
                Ok(segment_result) => {
                    // Merge successful parse results
                    functions.extend(segment_result.functions);
                    classes.extend(segment_result.classes);
                    imports.extend(segment_result.imports);
                    if segment_result.start_function.is_some() {
                        start_function = segment_result.start_function;
                    }
                }
                Err(segment_error) => {
                    collected_errors.push(segment_error);

                    // Try to create a partial AST node for the failed segment
                    if let Some(partial) = self.create_partial_node(&segment) {
                        match partial {
                            PartialNode::Function(f) => functions.push(f),
                            PartialNode::Class(c) => classes.push(c),
                            // Add other cases as needed
                        }
                    }
                }
            }

            if collected_errors.len() >= self.max_errors {
                break;
            }
        }

        // Create a program from recovered parts
        let recovered_program = Program {
            imports,
            statements: Vec::new(),
            functions,
            classes,
            start_function,
            tests: Vec::new(),
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
        // Try to parse as start function first
        if let Ok(pairs) = <CleanParser as Parser<Rule>>::parse(Rule::start_function, segment) {
            let start_func = parse_start_function(pairs.into_iter().next().unwrap())?;
            return Ok(Program {
                imports: Vec::new(),
                statements: Vec::new(),
                functions: Vec::new(),
                classes: Vec::new(),
                start_function: Some(start_func),
                tests: Vec::new(),
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
                    // Check if this is a start function
                    if trimmed.starts_with("start()") {
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
            } else if trimmed.starts_with("start()") {
                in_start_function = true;
                current_indent = line_indent;
                start_function_lines.push(line.to_string());
            }
        }

        // Use preprocessor to parse the functions block
        let preprocessor = super::preprocessor::FunctionPreprocessor::new(&functions_block_content);
        match preprocessor.process_functions_block(&functions_block_content) {
            Ok(functions) => {
                // Parse start function if present
                let start_function = if !start_function_lines.is_empty() {
                    let start_source = start_function_lines.join("\n");
                    match <CleanParser as Parser<Rule>>::parse(Rule::start_function, &start_source)
                    {
                        Ok(pairs) => Some(parse_start_function(pairs.into_iter().next().unwrap())?),
                        Err(_) => {
                            println!(
                                "DEBUG: Failed to parse start function, continuing without it"
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                Ok(Program {
                    imports: Vec::new(),
                    statements: Vec::new(),
                    functions,
                    classes: Vec::new(),
                    start_function,
                    tests: Vec::new(),
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
                    statements: Vec::new(),
                    functions: Vec::new(),
                    classes: vec![class],
                    start_function: None,
                    tests: Vec::new(),
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
            if let Some(name) = self.extract_function_name(trimmed) {
                return Some(PartialNode::Function(Function {
                    name,
                    type_parameters: Vec::new(),
                    type_constraints: Vec::new(),
                    parameters: Vec::new(),
                    return_type: Type::Void,
                    body: Vec::new(), // Empty body for failed parse
                    description: Some(
                        "// Parse error - function body could not be parsed".to_string(),
                    ),
                    syntax: FunctionSyntax::Simple,
                    visibility: Visibility::Public,
                    modifier: FunctionModifier::None,
                    location: None,
                }));
            }
        }

        None
    }

    /// Extract function name from malformed function declaration
    fn extract_function_name(&self, segment: &str) -> Option<String> {
        // Look for pattern: function [type] name(
        let words: Vec<&str> = segment.split_whitespace().collect();
        if words.len() >= 2 && words[0] == "function" {
            // Case 1: function name(
            if words[1].contains('(') {
                return Some(words[1].split('(').next().unwrap().to_string());
            }
            // Case 2: function type name(
            if words.len() >= 3 && words[2].contains('(') {
                return Some(words[2].split('(').next().unwrap().to_string());
            }
            // Case 3: just function name
            return Some(words[1].to_string());
        }
        None
    }

    fn parse_internal(&mut self, source: &str) -> Result<Program, CompilerError> {
        let trimmed_source = source.trim();
        let pairs = <CleanParser as Parser<Rule>>::parse(Rule::program, trimmed_source)
            .map_err(|e| crate::error::ErrorUtils::from_pest_error(e, source, &self.file_path))?;

        parse_program_ast(pairs)
    }
}

/// Represents partial AST nodes created during error recovery
#[derive(Debug, Clone)]
#[allow(dead_code)]
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

pub fn parse_with_file(source: &str, file_path: &str) -> Result<Program, CompilerError> {
    let trimmed_source = source.trim();

    // Check if this is a functions: block program - handle it specially
    if source.contains("functions:") {
        match parse_with_preprocessing(source, file_path) {
            Ok(program) => {
                return Ok(program);
            }
            Err(_e) => {
                // Continue to traditional parsing as fallback
            }
        }
    }

    // Try traditional parsing first
    match <CleanParser as Parser<Rule>>::parse(Rule::program, trimmed_source) {
        Ok(pairs) => parse_program_ast(pairs),
        Err(pest_error) => {
            // If traditional parsing fails, use recovery parsing instead of preprocessing
            let error_msg = pest_error.to_string();
            println!("DEBUG: Traditional parsing failed ({error_msg}), trying recovery parsing");

            // Use recovery parsing which handles both classes and functions correctly
            match super::CleanParser::parse_program_with_recovery(source, file_path) {
                Ok(program) => {
                    println!("DEBUG: Recovery parsing succeeded!");
                    Ok(program)
                }
                Err(errors) => {
                    println!(
                        "DEBUG: Recovery parsing also failed with {} errors",
                        errors.len()
                    );
                    // Return the first error from recovery parsing, or the original error if recovery had no errors
                    if let Some(first_error) = errors.first() {
                        Err(first_error.clone())
                    } else {
                        Err(ErrorUtils::from_pest_error(pest_error, source, file_path))
                    }
                }
            }
        }
    }
}

/// Parse using preprocessing approach for complex multi-function programs
fn parse_with_preprocessing(source: &str, _file_path: &str) -> Result<Program, CompilerError> {
    // Parse the full program using traditional parsing since preprocessing has limitations
    // Just disable preprocessing for now and fall back to traditional parsing
    match <CleanParser as Parser<Rule>>::parse(Rule::program, source) {
        Ok(pairs) => {
            println!("DEBUG: Traditional parsing succeeded in preprocessing path");
            parse_program_ast(pairs)
        }
        Err(traditional_error) => {
            println!(
                "DEBUG: Traditional parsing failed in preprocessing path: {traditional_error}"
            );
            Err(ErrorUtils::from_pest_error(
                traditional_error,
                source,
                "<unknown>",
            ))
        }
    }
}

pub fn parse_program_ast(pairs: pest::iterators::Pairs<Rule>) -> Result<Program, CompilerError> {
    let mut functions = Vec::new();
    let mut classes = Vec::new();
    let mut start_function = None;
    let mut imports = Vec::new();
    let mut tests = Vec::new();
    let mut top_level_statements = Vec::new();

    for pair in pairs {
        if pair.as_rule() == Rule::program {
            for inner in pair.into_inner() {
                match inner.as_rule() {
                    Rule::program_item => {
                        for program_item_inner in inner.into_inner() {
                            println!(
                                "DEBUG: Parsing program item: {:?}",
                                program_item_inner.as_rule()
                            );
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
                                    println!("DEBUG: Found standalone functions_block rule (not part of class)");
                                    let block_functions =
                                        parse_functions_block(program_item_inner)?;
                                    functions.extend(block_functions);
                                }
                                // Standalone functions removed - all functions must be in functions: blocks per specification
                                Rule::start_function => {
                                    let func = parse_start_function(program_item_inner)?;
                                    start_function = Some(func);
                                }
                                Rule::implicit_start_function => {
                                    let func = parse_start_function(program_item_inner)?;
                                    start_function = Some(func);
                                }
                                Rule::class_decl => {
                                    println!("DEBUG: Found class_decl rule, calling parse_class");
                                    let class = parse_class(program_item_inner)?;
                                    println!(
                                        "DEBUG: parse_class returned class: {class_name}",
                                        class_name = class.name
                                    );
                                    classes.push(class);
                                }
                                Rule::tests_block => {
                                    let test_cases = parse_tests_block(program_item_inner)?;
                                    tests.extend(test_cases);
                                }
                                Rule::statement => {
                                    // Handle top-level statements - these should be added to the start function
                                    let stmt = parse_statement(program_item_inner)?;
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

    println!(
        "DEBUG: Building program with {} functions, {} classes",
        functions.len(),
        classes.len()
    );
    for (i, class) in classes.iter().enumerate() {
        println!(
            "DEBUG: Class {}: {} with {} methods",
            i,
            class.name,
            class.methods.len()
        );
    }
    let program = Program {
        imports,
        statements: Vec::new(),
        functions,
        classes,
        start_function,
        tests,
        location: None,
    };

    Ok(program)
}

pub fn parse_start_function(pair: Pair<Rule>) -> Result<Function, CompilerError> {
    let name = "start".to_string();
    let mut body = Vec::new();
    let location = Some(convert_to_ast_location(&get_location(&pair)));

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::indented_block {
            for stmt_pair in inner.into_inner() {
                if stmt_pair.as_rule() == Rule::statement {
                    body.push(parse_statement(stmt_pair)?);
                }
            }
        }
    }

    // Start function must always have void return type for WebAssembly compatibility
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

// parse_standalone_function removed - all functions must be in functions: blocks per specification

/// Parse a parameter from a parameter list: type identifier [= default_value]
fn parse_parameter(pair: Pair<Rule>) -> Result<Parameter, CompilerError> {
    let mut param_type = None;
    let mut param_name = String::new();
    let mut default_value = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::type_ => {
                param_type = Some(parse_type(inner)?);
            }
            Rule::identifier | Rule::parameter_name => {
                param_name = inner.as_str().to_string();
            }
            Rule::expression => {
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
    super::SourceLocation {
        start: span.start(),
        end: span.end(),
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

    println!(
        "DEBUG: parse_functions_block_with_context called with class_context: {:?}",
        class_context.as_ref().map(|c| &c.class_name)
    );

    // Try preprocessing approach first for multi-function blocks
    let preprocessor = if let Some(context) = class_context {
        println!(
            "DEBUG: Creating preprocessor with class context: {}",
            context.class_name
        );
        super::preprocessor::FunctionPreprocessor::with_class_context(source_text, context)
    } else {
        println!("DEBUG: Creating preprocessor without class context");
        super::preprocessor::FunctionPreprocessor::new(source_text)
    };

    match preprocessor.process_functions_block(source_text) {
        Ok(functions) => {
            if !functions.is_empty() {
                println!(
                    "DEBUG: Preprocessor succeeded, found {} functions",
                    functions.len()
                );
                for (i, func) in functions.iter().enumerate() {
                    println!(
                        "DEBUG: Function {}: {} - description: {:?}",
                        i, func.name, func.description
                    );
                }
                return Ok(functions);
            }
        }
        Err(e) => {
            println!("DEBUG: Preprocessor failed: {e}, falling back to traditional");
        }
    }

    // Fall back to traditional parsing
    parse_functions_block_traditional(functions_block)
}

pub fn parse_functions_block_traditional(
    functions_block: Pair<Rule>,
) -> Result<Vec<Function>, CompilerError> {
    println!("DEBUG: Using traditional functions block parsing (no class context)");
    let mut functions = Vec::new();

    for item in functions_block.into_inner() {
        if item.as_rule() == Rule::indented_functions_block {
            for func_item in item.into_inner() {
                if func_item.as_rule() == Rule::function_in_block {
                    let func = parse_function_in_block(func_item)?;
                    functions.push(func);
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

#[allow(non_snake_case)]
#[allow(dead_code)]
pub fn parse_class_decl_OLD_UNUSED(class_pair: Pair<Rule>) -> Result<Class, CompilerError> {
    let mut class_name = String::new();
    let mut type_parameters = Vec::new();
    let mut base_class = None;
    let mut base_class_type_args = Vec::new();
    let mut fields = Vec::new();
    let mut methods = Vec::new();
    let mut constructor = None;
    let location = Some(convert_to_ast_location(&get_location(&class_pair)));

    for item in class_pair.into_inner() {
        match item.as_rule() {
            Rule::identifier => {
                class_name = item.as_str().to_string();
            }
            Rule::type_ => {
                // Parse "is BaseClass" or "is BaseClass<Args>" inheritance
                let type_result = parse_type(item)?;
                match type_result {
                    Type::Object(class_name) => {
                        base_class = Some(class_name);
                    }
                    Type::Generic(boxed_type, type_args) => {
                        if let Type::Object(class_name) = *boxed_type {
                            base_class = Some(class_name);
                            base_class_type_args = type_args;
                        }
                    }
                    Type::TypeParameter(class_name) => {
                        // Handle simple class names that are parsed as type parameters
                        base_class = Some(class_name);
                    }
                    _ => {
                        return Err(CompilerError::parse_error(
                            "Base class must be a class type".to_string(),
                            location.clone(),
                            Some("Use a valid class name for inheritance".to_string()),
                        ));
                    }
                }
            }
            Rule::indented_class_body => {
                // Parse the class body containing field declarations, constructors, methods
                for body_item in item.into_inner() {
                    if body_item.as_rule() == Rule::class_body_item {
                        for class_item in body_item.into_inner() {
                            match class_item.as_rule() {
                                Rule::class_field => {
                                    let field = parse_class_field(class_item)?;

                                    // If field type is a type parameter, add it to type_parameters if not already present
                                    if let Type::TypeParameter(ref param_name) = field.type_ {
                                        if !type_parameters.contains(param_name) {
                                            type_parameters.push(param_name.clone());
                                        }
                                    }

                                    fields.push(field);
                                }
                                Rule::constructor => {
                                    constructor = Some(parse_constructor(class_item)?);
                                }
                                Rule::functions_block => {
                                    let block_methods = parse_functions_block(class_item)?;
                                    methods.extend(block_methods);
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

    if class_name.is_empty() {
        return Err(CompilerError::parse_error(
            "Class is missing a name".to_string(),
            location.clone(),
            None,
        ));
    }

    Ok(Class {
        name: class_name,
        type_parameters,
        description: None,
        base_class,
        base_class_type_args,
        fields,
        methods,
        constructor,
        location,
    })
}

/// Parse a field declaration within a class
#[allow(dead_code)]
fn parse_class_field(field_pair: Pair<Rule>) -> Result<Field, CompilerError> {
    let mut field_name = String::new();
    let mut field_type = None;
    let mut default_value = None;

    for item in field_pair.into_inner() {
        match item.as_rule() {
            Rule::type_ => {
                field_type = Some(parse_type(item)?);
            }
            Rule::identifier => {
                field_name = item.as_str().to_string();
            }
            Rule::expression => {
                default_value = Some(parse_expression(item)?);
            }
            _ => {}
        }
    }

    if field_name.is_empty() {
        return Err(CompilerError::parse_error(
            "Field is missing a name".to_string(),
            None,
            None,
        ));
    }

    let field_type = field_type.ok_or_else(|| {
        CompilerError::parse_error("Field is missing a type".to_string(), None, None)
    })?;

    Ok(Field {
        name: field_name,
        type_: field_type,
        visibility: Visibility::Public,
        is_static: false,
        default_value,
    })
}

/// Parse a constructor within a class
#[allow(dead_code)]
fn parse_constructor(constructor_pair: Pair<Rule>) -> Result<Constructor, CompilerError> {
    let mut parameters = Vec::new();
    let mut body = Vec::new();
    let location = Some(convert_to_ast_location(&get_location(&constructor_pair)));

    for item in constructor_pair.into_inner() {
        match item.as_rule() {
            Rule::constructor_parameter_list => {
                for param_item in item.into_inner() {
                    if param_item.as_rule() == Rule::constructor_parameter {
                        let param = parse_constructor_parameter(param_item)?;
                        parameters.push(param);
                    }
                }
            }
            Rule::indented_block => {
                for stmt_pair in item.into_inner() {
                    if stmt_pair.as_rule() == Rule::statement {
                        body.push(parse_statement(stmt_pair)?);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Constructor::new(parameters, body, location))
}

/// Parse a constructor parameter
#[allow(dead_code)]
fn parse_constructor_parameter(param_pair: Pair<Rule>) -> Result<Parameter, CompilerError> {
    let mut param_name = String::new();
    let mut param_type = None;

    for item in param_pair.into_inner() {
        match item.as_rule() {
            Rule::type_ => {
                param_type = Some(parse_type(item)?);
            }
            Rule::identifier | Rule::parameter_name => {
                param_name = item.as_str().to_string();
            }
            _ => {}
        }
    }

    if param_name.is_empty() {
        return Err(CompilerError::parse_error(
            "Constructor parameter is missing a name".to_string(),
            None,
            None,
        ));
    }

    // If no type is specified, we'll infer it from class fields later in semantic analysis
    // For now, mark it as unresolved with a special marker
    let param_type = param_type.unwrap_or(Type::Any);

    Ok(Parameter::new(param_name, param_type))
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

/// Parse a single input_declaration as a Parameter
/// Used when input declarations appear directly in function bodies
#[allow(dead_code)]
fn parse_input_declaration_as_parameter(
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
                "Missing parameter name in input declaration",
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
            "Missing type in input parameter declaration",
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
            Rule::function_type => {
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
            Rule::function_body => {
                // function_body = (setup_block ~ indented_block) | indented_block
                let mut found_body = false;
                let mut _found_setup = false;
                for body_item in item.into_inner() {
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
                        Rule::function_statements => {
                            found_body = true;
                            // Process statements, handling input_declaration specially
                            for stmt_pair in body_item.into_inner() {
                                if stmt_pair.as_rule() == Rule::statement {
                                    // Check if this statement contains an input_declaration
                                    let inner = stmt_pair.clone().into_inner().next().unwrap();
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
                                        body.push(parse_statement(stmt_pair)?);
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
            return Ok(ImportItem { name, alias });
        }
    } else if import_text.contains('.') {
        // Single symbol import like "Math.sqrt"
        let name = import_text.to_string();
        return Ok(ImportItem { name, alias: None });
    } else {
        // Simple module import like "Math"
        let name = identifiers[0].clone();
        return Ok(ImportItem { name, alias: None });
    }

    Err(CompilerError::parse_error(
        format!("Invalid import syntax: '{import_text}'"),
        None,
        None,
    ))
}

/// Parse a later assignment statement
fn _unused_parse_later_assignment(pair: Pair<Rule>) -> Result<Statement, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut variable = String::new();
    let mut expression = None;

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => {
                variable = inner.as_str().to_string();
            }
            Rule::expression => {
                // Fixed: Use proper expression parsing instead of placeholder
                expression = Some(crate::parser::expression_parser::parse_expression(inner)?);
            }
            _ => {}
        }
    }

    if variable.is_empty() {
        return Err(CompilerError::parse_error(
            "Later assignment is missing variable name".to_string(),
            location.clone(),
            None,
        ));
    }

    let expression = expression.ok_or_else(|| {
        CompilerError::parse_error(
            "Later assignment is missing expression".to_string(),
            location.clone(),
            None,
        )
    })?;

    Ok(Statement::LaterAssignment {
        variable,
        expression,
        location,
    })
}

/// Parse a background statement
fn _unused_parse_background_statement(pair: Pair<Rule>) -> Result<Statement, CompilerError> {
    let location = Some(convert_to_ast_location(&get_location(&pair)));
    let mut expression = None;

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::expression {
            // Fixed: Use proper expression parsing instead of placeholder
            expression = Some(crate::parser::expression_parser::parse_expression(inner)?);
            break;
        }
    }

    let expression = expression.ok_or_else(|| {
        CompilerError::parse_error(
            "Background statement is missing expression".to_string(),
            location.clone(),
            None,
        )
    })?;

    Ok(Statement::Background {
        expression,
        location,
    })
}

/// Parse a start expression for async programming
fn _unused_parse_start_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = convert_to_ast_location(&get_location(&pair));
    let mut expression = None;

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::expression {
            // Fixed: Use proper expression parsing instead of placeholder
            expression = Some(Box::new(
                crate::parser::expression_parser::parse_expression(inner)?,
            ));
            break;
        }
    }

    let expression = expression.ok_or_else(|| {
        CompilerError::parse_error(
            "Start expression is missing inner expression".to_string(),
            Some(location.clone()),
            None,
        )
    })?;

    Ok(Expression::StartExpression {
        expression,
        location,
    })
}

// Note: We'll need to add parse_expression function - this is a placeholder reference
fn _unused_parse_expression(_pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    // Fixed: This function should delegate to the proper expression parser
    // For now, return an error indicating this shouldn't be used
    Err(CompilerError::parse_error(
        "This function is deprecated - use crate::parser::expression_parser::parse_expression instead".to_string(),
        None,
        Some("Use the expression_parser module for parsing expressions".to_string())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CleanParser;
    use pest::Parser;

    #[test]
    fn test_parse_function_in_block_valid() {
        let source = "integer add()\n\tinput\n\t\tinteger a\n\t\tinteger b\n\n\treturn a + b";
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
        let source = "integer ()\n\treturn 42";
        let result = <CleanParser as Parser<Rule>>::parse(Rule::function_in_block, source);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("identifier") || err.to_string().contains("size_specifier")
        );
    }

    #[test]
    fn debug_parse_tree_for_function_in_block() {
        let source = "integer add()\n\tinput\n\t\tinteger a\n\t\tinteger b\n\t\n\treturn a + b";
        let mut pairs =
            <CleanParser as Parser<Rule>>::parse(Rule::function_in_block, source).unwrap();
        let pair = pairs.next().unwrap();
        println!("Parse tree: {:#?}", pair);
    }
}
