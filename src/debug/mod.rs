use crate::ast::{AssignmentTarget, Expression, Function, Program, Statement, Type};

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
            Type::List(inner, _behavior) => format!("list<{}>", Self::type_to_string(inner)),
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
                let target_str = match target {
                    AssignmentTarget::Variable(name) => name.clone(),
                    AssignmentTarget::Index { collection, .. } => format!("{collection}[…]"),
                    AssignmentTarget::Property { object, path } => {
                        format!("{object}.{}", path.join("."))
                    }
                };
                format!("assign to {target_str}")
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

    /// Validate code style and return issues.
    ///
    /// Checks the leading whitespace of each line (Clean Language indents
    /// with tabs only), trailing whitespace, and lines longer than 120
    /// characters.
    pub fn validate_style(source: &str) -> Vec<String> {
        let mut issues = Vec::new();

        for (line_num, line) in source.lines().enumerate() {
            let line_no = line_num + 1;
            let indent: String = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let only_whitespace = indent.chars().count() == line.chars().count();
            if !only_whitespace && indent.contains(' ') {
                if indent.contains('\t') {
                    issues.push(format!(
                        "Line {line_no}: Mixed spaces and tabs in indentation"
                    ));
                } else {
                    issues.push(format!(
                        "Line {line_no}: Indentation uses spaces; Clean Language indents with tabs"
                    ));
                }
            }

            if line.ends_with(' ') || line.ends_with('\t') {
                issues.push(format!("Line {line_no}: Trailing whitespace"));
            }

            let length = line.chars().count();
            if length > 120 {
                issues.push(format!(
                    "Line {line_no}: Line too long ({length} characters)"
                ));
            }
        }

        issues
    }
}

#[cfg(test)]
mod tests {
    use super::DebugUtils;

    #[test]
    fn style_flags_space_indentation_and_mixed_indentation() {
        let issues = DebugUtils::validate_style("start:\n    print(1)\n\t print(2)\n");
        assert_eq!(
            issues,
            vec![
                "Line 2: Indentation uses spaces; Clean Language indents with tabs".to_string(),
                "Line 3: Mixed spaces and tabs in indentation".to_string(),
            ]
        );
    }

    #[test]
    fn style_ignores_tabs_inside_code_and_counts_characters() {
        // A tab after the code, not in the indentation, is not mixed indentation.
        let source = format!(
            "start:\n\tprint(\"a\tb\")\n\tprint(\"{}\")\n",
            "é".repeat(110)
        );
        assert!(DebugUtils::validate_style(&source).is_empty());
        let long = format!("\tprint(\"{}\")\n", "é".repeat(120));
        assert_eq!(
            DebugUtils::validate_style(&long),
            vec!["Line 1: Line too long (130 characters)".to_string()]
        );
    }

    #[test]
    fn style_flags_trailing_whitespace() {
        assert_eq!(
            DebugUtils::validate_style("start:\t\n\tprint(1) \n"),
            vec![
                "Line 1: Trailing whitespace".to_string(),
                "Line 2: Trailing whitespace".to_string(),
            ]
        );
    }
}
