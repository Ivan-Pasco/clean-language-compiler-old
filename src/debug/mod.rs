use crate::ast::{Expression, Function, Program, Statement, Type};
use crate::error::CompilerError;
use crate::error::CompilerWarning;
use crate::parser::Rule;
use std::fmt::Write;

/// Debug utilities for Clean Language development
pub struct DebugUtils;

impl DebugUtils {
    /// Print a detailed AST structure for debugging
    pub fn print_ast(program: &Program) {
        println!("🌳 AST Structure:");
        println!("{}", "═".repeat(50));

        if !program.imports.is_empty() {
            println!("📦 Imports ({}):", program.imports.len());
            for (i, import) in program.imports.iter().enumerate() {
                println!("  {}. {}", i + 1, import.name);
            }
            println!();
        }

        if !program.functions.is_empty() {
            println!("🔧 Functions ({}):", program.functions.len());
            for (i, func) in program.functions.iter().enumerate() {
                Self::print_function_summary(func, i + 1);
            }
            println!();
        }

        if let Some(start_func) = &program.start_function {
            println!("🚀 Start Function:");
            Self::print_function_details(start_func, 1);
            println!();
        }

        if !program.classes.is_empty() {
            println!("🏗️  Classes ({}):", program.classes.len());
            for (i, class) in program.classes.iter().enumerate() {
                println!(
                    "  {}. {} (fields: {}, methods: {})",
                    i + 1,
                    class.name,
                    class.fields.len(),
                    class.methods.len()
                );
            }
        }
    }

    /// Print a summary of a function
    fn print_function_summary(func: &Function, index: usize) {
        println!(
            "  {}. {} -> {} (params: {}, statements: {})",
            index,
            func.name,
            Self::type_to_string(&func.return_type),
            func.parameters.len(),
            func.body.len()
        );
    }

    /// Print detailed function information
    fn print_function_details(func: &Function, indent: usize) {
        let prefix = "  ".repeat(indent);
        println!("{}📋 Name: {}", prefix, func.name);
        println!(
            "{}📤 Return Type: {}",
            prefix,
            Self::type_to_string(&func.return_type)
        );

        if !func.parameters.is_empty() {
            println!("{prefix}📥 Parameters:");
            for param in &func.parameters {
                println!(
                    "{}  • {} : {}",
                    prefix,
                    param.name,
                    Self::type_to_string(&param.type_)
                );
            }
        }

        if !func.body.is_empty() {
            println!("{}📝 Body ({} statements):", prefix, func.body.len());
            for (i, stmt) in func.body.iter().enumerate() {
                println!("{}  {}. {}", prefix, i + 1, Self::statement_to_string(stmt));
            }
        }
    }

    /// Convert a type to a readable string
    fn type_to_string(type_: &Type) -> String {
        match type_ {
            Type::Integer => "integer".to_string(),
            Type::Number => "number".to_string(),
            Type::Boolean => "boolean".to_string(),
            Type::String => "string".to_string(),
            Type::Void => "void".to_string(),
            Type::List(inner) => format!("list<{}>", Self::type_to_string(inner)),
            Type::Object(name) => name.clone(),
            Type::Any => "any".to_string(),
            _ => format!("{type_:?}"),
        }
    }

    /// Convert a statement to a readable string
    fn statement_to_string(stmt: &Statement) -> String {
        match stmt {
            Statement::VariableDecl { name, type_, .. } => {
                format!("var {name} : {}", Self::type_to_string(type_))
            }
            Statement::Assignment { target, .. } => {
                format!("assign to {target}")
            }
            Statement::Expression { expr, .. } => {
                format!("expr: {}", Self::expression_to_string(expr))
            }
            Statement::Print { expression, .. } => {
                format!("print({})", Self::expression_to_string(expression))
            }
            Statement::Return { value, .. } => {
                if let Some(val) = value {
                    format!("return {}", Self::expression_to_string(val))
                } else {
                    "return".to_string()
                }
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let mut result = format!(
                    "if {} (then: {} stmts",
                    Self::expression_to_string(condition),
                    then_branch.len()
                );
                if let Some(else_stmts) = else_branch {
                    result.push_str(&format!(", else: {len} stmts", len = else_stmts.len()));
                }
                result.push(')');
                result
            }
            _ => format!("{stmt:?}").chars().take(50).collect::<String>() + "...",
        }
    }

    /// Convert an expression to a readable string
    fn expression_to_string(expr: &Expression) -> String {
        match expr {
            Expression::Literal(val) => format!("{val:?}"),
            Expression::Variable(name) => name.clone(),
            Expression::Call(name, arguments) => {
                format!("{name}({len})", len = arguments.len())
            }
            Expression::Binary(left, operator, right) => {
                format!(
                    "{} {:?} {}",
                    Self::expression_to_string(left),
                    operator,
                    Self::expression_to_string(right)
                )
            }
            _ => format!("{expr:?}").chars().take(30).collect::<String>() + "...",
        }
    }

    /// Analyze code complexity and provide suggestions
    pub fn analyze_complexity(program: &Program) -> Vec<String> {
        let mut suggestions = Vec::new();

        // Check function complexity
        for func in &program.functions {
            if func.body.len() > 20 {
                suggestions.push(format!(
                    "Function '{}' has {} statements. Consider breaking it into smaller functions.",
                    func.name,
                    func.body.len()
                ));
            }

            if func.parameters.len() > 5 {
                suggestions.push(format!(
                    "Function '{}' has {} parameters. Consider using a struct or reducing parameters.",
                    func.name, func.parameters.len()
                ));
            }
        }

        // Check for deeply nested structures
        for func in &program.functions {
            let max_depth = Self::calculate_nesting_depth(&func.body);
            if max_depth > 4 {
                suggestions.push(format!(
                    "Function '{}' has nesting depth of {}. Consider refactoring to reduce complexity.",
                    func.name, max_depth
                ));
            }
        }

        suggestions
    }

    /// Calculate maximum nesting depth in statements
    fn calculate_nesting_depth(statements: &[Statement]) -> usize {
        let mut max_depth = 0;

        for stmt in statements {
            let depth = match stmt {
                Statement::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    let then_depth = Self::calculate_nesting_depth(then_branch);
                    let else_depth = else_branch
                        .as_ref()
                        .map(|branch| Self::calculate_nesting_depth(branch))
                        .unwrap_or(0);
                    1 + then_depth.max(else_depth)
                }
                Statement::Iterate { body, .. } | Statement::RangeIterate { body, .. } => {
                    1 + Self::calculate_nesting_depth(body)
                }
                Statement::Test { body, .. } => 1 + Self::calculate_nesting_depth(body),
                _ => 0,
            };
            max_depth = max_depth.max(depth);
        }

        max_depth
    }

    /// Generate a code style report
    pub fn generate_style_report(program: &Program) -> StyleReport {
        let mut report = StyleReport::new();

        // Check naming conventions
        for func in &program.functions {
            if !Self::is_camel_case(&func.name) {
                report.add_warning(format!(
                    "Function '{}' should use camelCase naming convention",
                    func.name
                ));
            }

            for param in &func.parameters {
                if !Self::is_camel_case(&param.name) {
                    report.add_warning(format!(
                        "Parameter '{}' in function '{}' should use camelCase naming convention",
                        param.name, func.name
                    ));
                }
            }
        }

        // Check for missing descriptions
        for func in &program.functions {
            if func.description.is_none() && func.name != "start" {
                report.add_suggestion(format!(
                    "Function '{}' should have a description for better documentation",
                    func.name
                ));
            }
        }

        report
    }

    /// Check if a string follows camelCase convention
    fn is_camel_case(s: &str) -> bool {
        if s.is_empty() {
            return false;
        }

        let first_char = s.chars().next().unwrap();
        if !first_char.is_lowercase() {
            return false;
        }

        // Check for underscores (not allowed in camelCase)
        !s.contains('_')
    }

    /// Print detailed error analysis
    pub fn analyze_error(error: &CompilerError) {
        println!("=== Error Analysis ===");
        println!("Error: {error}");

        // Provide contextual help based on error type
        match error {
            CompilerError::Syntax { .. } => {
                println!("💡 Syntax Error Help:");
                println!("  - Check for missing indentation (Clean Language uses tabs)");
                println!("  - Verify function declarations have proper syntax");
                println!("  - Ensure all blocks are properly indented");
            }
            CompilerError::Type { .. } => {
                println!("💡 Type Error Help:");
                println!("  - Check variable types match expected values");
                println!("  - Verify function arguments match parameter types");
                println!("  - Consider using type conversion methods");
            }
            CompilerError::Memory { .. } => {
                println!("💡 Memory Error Help:");
                println!("  - Check for large data structures");
                println!("  - Consider optimizing memory usage");
                println!("  - Verify array/matrix sizes are reasonable");
            }
            _ => {
                println!("💡 General Help:");
                println!("  - Check the Clean Language documentation");
                println!("  - Verify syntax matches language specification");
                println!("  - Consider simplifying complex expressions");
            }
        }
    }

    /// Analyze multiple errors and provide comprehensive feedback
    pub fn analyze_errors(errors: &[CompilerError]) -> Vec<String> {
        let mut analysis = Vec::new();

        for error in errors {
            analysis.push(format!("Error: {error}"));

            // Add specific suggestions based on error type
            match error {
                CompilerError::Syntax { .. } => {
                    analysis.push("  → Check indentation and syntax".to_string());
                }
                CompilerError::Type { .. } => {
                    analysis.push("  → Verify type compatibility".to_string());
                }
                CompilerError::Memory { .. } => {
                    analysis.push("  → Optimize memory usage".to_string());
                }
                _ => {
                    analysis.push("  → Review code structure".to_string());
                }
            }
        }

        analysis
    }

    /// Validate code style and return issues
    pub fn validate_style(source: &str) -> Vec<String> {
        let mut issues = Vec::new();

        // Basic style checks on source code
        let lines: Vec<&str> = source.lines().collect();

        for (line_num, line) in lines.iter().enumerate() {
            // Check for mixed indentation
            if line.starts_with(' ') && line.contains('\t') {
                issues.push(format!("Line {}: Mixed spaces and tabs", line_num + 1));
            }

            // Check for trailing whitespace
            if line.ends_with(' ') || line.ends_with('\t') {
                issues.push(format!("Line {}: Trailing whitespace", line_num + 1));
            }

            // Check line length
            if line.len() > 120 {
                issues.push(format!(
                    "Line {}: Line too long ({} characters)",
                    line_num + 1,
                    line.len()
                ));
            }
        }

        issues
    }

    /// Create a comprehensive debug report
    pub fn create_debug_report(
        source: &str,
        file_path: &str,
        parse_result: &Result<crate::ast::Program, CompilerError>,
        warnings: &[CompilerWarning],
    ) -> String {
        let mut report = String::new();

        report.push_str(&format!("=== Debug Report for {file_path} ===\n"));
        report.push_str(&format!("Source length: {len} characters\n", len = source.len()));
        report.push_str(&format!("Source lines: {count}\n", count = source.lines().count()));

        match parse_result {
            Ok(program) => {
                report.push_str("✅ Parsing: SUCCESS\n");
                report.push_str(&format!("Functions: {len}\n", len = program.functions.len()));
                report.push_str(&format!("Classes: {len}\n", len = program.classes.len()));
                report.push_str(&format!(
                    "Has start function: {}\n",
                    program.start_function.is_some()
                ));
            }
            Err(error) => {
                report.push_str("❌ Parsing: FAILED\n");
                report.push_str(&format!("Error: {error}\n"));
            }
        }

        if !warnings.is_empty() {
            report.push_str(&format!("⚠️  Warnings: {len}\n", len = warnings.len()));
            for warning in warnings {
                report.push_str(&format!("  - {warning}\n"));
            }
        }

        // Add style analysis
        let style_issues = Self::validate_style(source);
        if !style_issues.is_empty() {
            report.push_str(&format!("🎨 Style Issues: {len}\n", len = style_issues.len()));
            for issue in style_issues {
                report.push_str(&format!("  - {issue}\n"));
            }
        }

        report
    }

    /// Generate a parse tree visualization for debugging
    pub fn visualize_parse_tree(source: &str, rule_name: &str) -> Result<String, String> {
        use crate::parser::CleanParser;
        use pest::Parser;

        let rule = match rule_name {
            "program" => Rule::program,
            "function" => Rule::start_function,
            "statement" => Rule::statement,
            "expression" => Rule::expression,
            "primary" => Rule::primary,
            _ => return Err(format!("Unknown rule: {rule_name}")),
        };

        match <CleanParser as Parser<Rule>>::parse(rule, source) {
            Ok(pairs) => {
                let mut result = String::new();
                for pair in pairs {
                    Self::format_parse_tree(&mut result, &pair, 0);
                }
                Ok(result)
            }
            Err(e) => Err(format!("Parse error: {e}")),
        }
    }

    /// Format a parse tree for visualization
    fn format_parse_tree(output: &mut String, pair: &pest::iterators::Pair<Rule>, depth: usize) {
        let indent = "  ".repeat(depth);
        writeln!(
            output,
            "{}{:?}: \"{}\"",
            indent,
            pair.as_rule(),
            pair.as_str().chars().take(50).collect::<String>()
        )
        .unwrap();

        for inner_pair in pair.clone().into_inner() {
            Self::format_parse_tree(output, &inner_pair, depth + 1);
        }
    }

    /// Generate error recovery suggestions based on common patterns
    pub fn suggest_error_fixes(source: &str, errors: &[CompilerError]) -> Vec<String> {
        let mut suggestions = Vec::new();

        // Analyze source for common patterns
        let lines: Vec<&str> = source.lines().collect();

        // Check for common syntax issues
        for (line_num, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Detect incomplete expressions
            if trimmed.ends_with(" +")
                || trimmed.ends_with(" -")
                || trimmed.ends_with(" *")
                || trimmed.ends_with(" /")
            {
                suggestions.push(format!(
                    "Line {}: Incomplete expression - missing right operand",
                    line_num + 1
                ));
            }

            // Detect missing closing brackets/parentheses
            let open_parens = trimmed.matches('(').count();
            let close_parens = trimmed.matches(')').count();
            if open_parens > close_parens {
                suggestions.push(format!(
                    "Line {}: Missing {} closing parenthesis(es)",
                    line_num + 1,
                    open_parens - close_parens
                ));
            }

            let open_brackets = trimmed.matches('[').count();
            let close_brackets = trimmed.matches(']').count();
            if open_brackets > close_brackets {
                suggestions.push(format!(
                    "Line {}: Missing {} closing bracket(s)",
                    line_num + 1,
                    open_brackets - close_brackets
                ));
            }

            // Detect incorrect function syntax
            if trimmed.starts_with("func ") {
                suggestions.push(format!(
                    "Line {}: Use 'function' instead of 'func'",
                    line_num + 1
                ));
            }

            // Detect arrow function syntax
            if trimmed.contains("->") {
                suggestions.push(format!("Line {}: Clean Language doesn't use '->' syntax. Use 'function returnType name()' format", line_num + 1));
            }

            // Detect let keyword
            if trimmed.contains("let ") {
                suggestions.push(format!(
                    "Line {}: Use explicit types instead of 'let': 'integer x = 5' not 'let x = 5'",
                    line_num + 1
                ));
            }
        }

        // Add general suggestions based on error count
        if errors.len() > 5 {
            suggestions.push("Consider fixing the first few errors and re-parsing - many errors may be cascading from earlier issues".to_string());
        }

        suggestions
    }

    /// Create a comprehensive error report
    pub fn create_error_report(source: &str, errors: &[CompilerError]) -> String {
        let mut report = String::new();

        writeln!(report, "🚨 Clean Language Parser Error Report").unwrap();
        writeln!(report, "{}", "═".repeat(60)).unwrap();
        writeln!(report, "📄 File: {} lines", source.lines().count()).unwrap();
        writeln!(report, "❌ Errors: {}", errors.len()).unwrap();
        writeln!(report).unwrap();

        // Show each error with context
        for (i, error) in errors.iter().enumerate() {
            writeln!(report, "Error {} of {}:", i + 1, errors.len()).unwrap();
            writeln!(report, "{error}").unwrap();
            writeln!(report, "{}", "─".repeat(40)).unwrap();
        }

        // Add analysis
        let analysis = crate::error::ErrorUtils::analyze_multiple_errors(errors);
        for line in analysis {
            writeln!(report, "{line}").unwrap();
        }

        // Add fix suggestions
        let suggestions = Self::suggest_error_fixes(source, errors);
        if !suggestions.is_empty() {
            writeln!(report, "\n🔧 Specific Fix Suggestions:").unwrap();
            for suggestion in suggestions {
                writeln!(report, "• {suggestion}").unwrap();
            }
        }

        report
    }
}

/// Style report for code analysis
pub struct StyleReport {
    warnings: Vec<String>,
    suggestions: Vec<String>,
}

impl StyleReport {
    fn new() -> Self {
        Self {
            warnings: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    fn add_suggestion(&mut self, suggestion: String) {
        self.suggestions.push(suggestion);
    }

    pub fn print(&self) {
        println!("=== Style Report ===");

        if !self.warnings.is_empty() {
            println!("⚠️  Warnings:");
            for warning in &self.warnings {
                println!("  - {warning}");
            }
        }

        if !self.suggestions.is_empty() {
            println!("💡 Suggestions:");
            for suggestion in &self.suggestions {
                println!("  - {suggestion}");
            }
        }

        if self.warnings.is_empty() && self.suggestions.is_empty() {
            println!("✅ No style issues found!");
        }
    }
}
