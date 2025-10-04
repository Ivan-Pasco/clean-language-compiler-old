use super::expression_parser::{parse_expression, parse_start_expression};
use super::type_parser::parse_type;
use super::Rule;
use super::{convert_to_ast_location, get_location};
use crate::ast::{Expression, Statement};
use crate::error::CompilerError;
use pest::iterators::Pair;

pub fn parse_statement(pair: Pair<Rule>) -> Result<Statement, CompilerError> {
    let ast_location = convert_to_ast_location(&get_location(&pair));
    let inner = pair.into_inner().next().unwrap();

    match inner.as_rule() {
        Rule::variable_decl => parse_variable_declaration(inner, ast_location),
        Rule::assignment => parse_assignment_statement(inner, ast_location),
        Rule::print_parenthesized_stmt => parse_print_parenthesized_statement(inner, ast_location),
        Rule::print_newline_stmt => parse_print_newline_statement(inner, ast_location),
        Rule::print_bare_stmt => parse_print_bare_statement(inner, ast_location),
        Rule::if_stmt => parse_if_statement(inner, ast_location),
        Rule::while_stmt => parse_while_statement(inner, ast_location),
        Rule::iterate_stmt => parse_iterate_statement(inner, ast_location),
        Rule::range_iterate_stmt => parse_range_iterate_statement(inner, ast_location),
        Rule::test => parse_test_statement(inner, ast_location),
        Rule::return_stmt => parse_return_statement(inner, ast_location),
        Rule::error_stmt => parse_error_statement(inner, ast_location),
        Rule::on_error_block => parse_on_error_block_statement(inner, ast_location),
        Rule::standalone_on_error => parse_standalone_on_error_statement(inner, ast_location),
        Rule::apply_block => parse_apply_block_statement(inner, ast_location),
        Rule::type_apply_block => parse_type_apply_block_statement(inner, ast_location),
        Rule::function_apply_block => parse_function_apply_block_statement(inner, ast_location),
        Rule::method_apply_block => parse_method_apply_block_statement(inner, ast_location),
        Rule::constant_apply_block => parse_constant_apply_block_statement(inner, ast_location),
        Rule::background_stmt => parse_background_statement(inner, ast_location),
        Rule::later_assignment => parse_later_assignment_statement(inner, ast_location),
        Rule::import_block => parse_import_block_statement(inner, ast_location),
        Rule::private_block => parse_private_block_statement(inner, ast_location),
        Rule::start_expr => {
            let expr = parse_start_expression(inner)?;
            Ok(Statement::Expression {
                expr,
                location: Some(ast_location),
            })
        }
        Rule::function_call => {
            let expr = parse_expression(inner)?;
            Ok(Statement::Expression {
                expr,
                location: Some(ast_location),
            })
        }
        Rule::method_call => {
            let expr = parse_expression(inner)?;
            Ok(Statement::Expression {
                expr,
                location: Some(ast_location),
            })
        }
        Rule::chained_method_call => {
            let expr = parse_expression(inner)?;
            Ok(Statement::Expression {
                expr,
                location: Some(ast_location),
            })
        }
        Rule::expression => {
            let expr = parse_expression(inner)?;
            Ok(Statement::Expression {
                expr,
                location: Some(ast_location),
            })
        }
        Rule::base_call => {
            let expr = parse_expression(inner)?;
            Ok(Statement::Expression {
                expr,
                location: Some(ast_location),
            })
        }
        _ => Err(CompilerError::parse_error(
            format!("Unexpected statement: {:?}", inner.as_rule()),
            Some(ast_location),
            Some("Expected a valid statement".to_string()),
        )),
    }
}

fn parse_variable_declaration(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();

    // Handle the case where we get a nested rule
    let (type_part, name_part, initializer) = if parts.len() == 1 {
        let inner = parts.next().unwrap();
        match inner.as_rule() {
            Rule::variable_decl_without_assignment => {
                let mut inner_parts = inner.into_inner();
                let type_part = inner_parts.next().unwrap();
                let name_part = inner_parts.next().unwrap();
                (type_part, name_part, None)
            }
            Rule::variable_decl_with_assignment => {
                let mut inner_parts = inner.into_inner();
                let type_part = inner_parts.next().unwrap();
                let name_part = inner_parts.next().unwrap();
                let initializer_part = inner_parts.next().unwrap();
                let initializer = Some(parse_expression(initializer_part)?);
                (type_part, name_part, initializer)
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!(
                        "Unexpected variable declaration rule: {:?}",
                        inner.as_rule()
                    ),
                    Some(ast_location),
                    None,
                ));
            }
        }
    } else {
        // Direct parts case
        let type_part = parts.next().unwrap();
        let name_part = parts.next().unwrap();
        let initializer = parts
            .next()
            .map(|expr_part| parse_expression(expr_part))
            .transpose()?;
        (type_part, name_part, initializer)
    };

    let type_ = parse_type(type_part)?;
    let name = name_part.as_str().to_string();

    Ok(Statement::VariableDecl {
        name,
        type_,
        initializer,
        location: Some(ast_location),
    })
}

fn parse_assignment_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let target_part = parts.next().unwrap();
    let value = parse_expression(parts.next().unwrap())?;

    // Check if this is a property assignment (e.g., list.type = "line")
    if target_part.as_rule() == Rule::property_access {
        // Parse as property assignment expression
        let property_expr = super::expression_parser::parse_property_access(target_part)?;
        if let Expression::PropertyAccess {
            object,
            property,
            location,
        } = property_expr
        {
            return Ok(Statement::Expression {
                expr: Expression::PropertyAssignment {
                    object,
                    property,
                    value: Box::new(value),
                    location,
                },
                location: Some(ast_location),
            });
        }
        // If we get here, something went wrong with property parsing
        return Err(CompilerError::parse_error(
            "Failed to parse property assignment".to_string(),
            Some(ast_location),
            Some("Property assignment parsing failed".to_string()),
        ));
    }

    // Check if this is a list assignment (e.g., numbers[0] = 99)
    if target_part.as_rule() == Rule::list_access {
        // Parse as list assignment expression
        let list_expr = super::expression_parser::parse_list_access(target_part)?;
        if let Expression::ListAccess(list, index) = list_expr {
            return Ok(Statement::Expression {
                expr: Expression::ListAssignment {
                    list,
                    index,
                    value: Box::new(value),
                    location: ast_location.clone(),
                },
                location: Some(ast_location),
            });
        }
        // If we get here, something went wrong with list access parsing
        return Err(CompilerError::parse_error(
            "Failed to parse list assignment".to_string(),
            Some(ast_location),
            Some("List assignment parsing failed".to_string()),
        ));
    }

    // Regular variable assignment
    let target = target_part.as_str().trim().to_string();
    Ok(Statement::Assignment {
        target,
        value,
        location: Some(ast_location),
    })
}

fn parse_else_block_recursive(else_block: Pair<Rule>) -> Result<Vec<Statement>, CompilerError> {
    let mut parts = else_block.into_inner();

    // Skip whitespace and INDENT tokens
    while let Some(part) = parts.next() {
        match part.as_rule() {
            Rule::expression => {
                // This is an "else if" - we have: "else" "if" expression indented_block else_block?
                let ast_location = convert_to_ast_location(&get_location(&part));
                let condition = parse_expression(part)?;

                // Get the indented block for this else if
                let then_block = parts.next().unwrap(); // This should be the indented_block
                let mut then_stmts = Vec::new();
                parse_indented_block_statements(then_block, &mut then_stmts)?;

                // Check if there's another else block
                let mut else_branch = None;
                if let Some(next_else) = parts.next() {
                    if next_else.as_rule() == Rule::else_block {
                        else_branch = Some(parse_else_block_recursive(next_else)?);
                    }
                }

                // Return a single if statement representing the else if
                return Ok(vec![Statement::If {
                    condition,
                    then_branch: then_stmts,
                    else_branch,
                    location: Some(ast_location),
                }]);
            }
            Rule::indented_block => {
                // This is a simple "else" block
                let mut else_stmts = Vec::new();
                parse_indented_block_statements(part, &mut else_stmts)?;
                return Ok(else_stmts);
            }
            _ => continue, // Skip whitespace and other tokens
        }
    }

    Ok(Vec::new())
}

pub fn parse_indented_block_statements(
    block: Pair<Rule>,
    statements: &mut Vec<Statement>,
) -> Result<(), CompilerError> {
    for stmt_pair in block.into_inner() {
        match stmt_pair.as_rule() {
            Rule::statement => {
                statements.push(parse_statement(stmt_pair)?);
            }
            Rule::function_nested_block => {
                // Handle statements within function_nested_block
                for nested_stmt in stmt_pair.into_inner() {
                    if nested_stmt.as_rule() == Rule::statement {
                        statements.push(parse_statement(nested_stmt)?);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_if_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let condition = parse_expression(parts.next().unwrap())?;

    let mut then_branch = Vec::new();
    let mut else_branch = None;

    for part in parts {
        match part.as_rule() {
            Rule::indented_block => {
                // This is the then branch (first indented block)
                for stmt_pair in part.into_inner() {
                    match stmt_pair.as_rule() {
                        Rule::statement => {
                            then_branch.push(parse_statement(stmt_pair)?);
                        }
                        Rule::function_nested_block => {
                            // Handle statements within function_nested_block
                            for nested_stmt in stmt_pair.into_inner() {
                                if nested_stmt.as_rule() == Rule::statement {
                                    then_branch.push(parse_statement(nested_stmt)?);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Rule::else_block => {
                // Parse the else block recursively to handle else if chains
                else_branch = Some(parse_else_block_recursive(part)?);
            }
            _ => {}
        }
    }

    Ok(Statement::If {
        condition,
        then_branch,
        else_branch,
        location: Some(ast_location),
    })
}

fn parse_while_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let condition = parse_expression(parts.next().unwrap())?;

    let mut body = Vec::new();
    for part in parts {
        if part.as_rule() == Rule::indented_block {
            for stmt_pair in part.into_inner() {
                if stmt_pair.as_rule() == Rule::statement {
                    body.push(parse_statement(stmt_pair)?);
                }
            }
        }
    }

    Ok(Statement::While {
        condition,
        body,
        location: Some(ast_location),
    })
}

fn parse_iterate_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let iterator = parts.next().unwrap().as_str().to_string();
    let collection = parse_expression(parts.next().unwrap())?;

    let mut body = Vec::new();
    for part in parts {
        if part.as_rule() == Rule::indented_block {
            for stmt_pair in part.into_inner() {
                if stmt_pair.as_rule() == Rule::statement {
                    body.push(parse_statement(stmt_pair)?);
                }
            }
        }
    }

    Ok(Statement::Iterate {
        iterator,
        collection,
        body,
        location: Some(ast_location),
    })
}

fn parse_range_iterate_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let iterator = parts.next().unwrap().as_str().to_string();
    let start = parse_expression(parts.next().unwrap())?;
    let end = parse_expression(parts.next().unwrap())?;

    let mut step = None;
    let mut body = Vec::new();

    for part in parts {
        match part.as_rule() {
            Rule::expression => {
                step = Some(parse_expression(part)?);
            }
            Rule::indented_block => {
                for stmt_pair in part.into_inner() {
                    match stmt_pair.as_rule() {
                        Rule::statement => {
                            body.push(parse_statement(stmt_pair)?);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::RangeIterate {
        iterator,
        start,
        end,
        step,
        body,
        location: Some(ast_location),
    })
}

fn parse_test_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let name = parts.next().unwrap().as_str().trim_matches('"').to_string();

    let mut body = Vec::new();
    for part in parts {
        match part.as_rule() {
            Rule::indented_block => {
                for stmt_pair in part.into_inner() {
                    match stmt_pair.as_rule() {
                        Rule::statement => {
                            body.push(parse_statement(stmt_pair)?);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::Test {
        name,
        body,
        location: Some(ast_location),
    })
}

fn parse_return_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let value = pair
        .into_inner()
        .next()
        .map(|expr_part| parse_expression(expr_part))
        .transpose()?;

    Ok(Statement::Return {
        value,
        location: Some(ast_location),
    })
}

fn parse_error_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    // error("message") syntax
    let mut inner = pair.into_inner();
    let message_expr = inner.next().unwrap();
    let message = parse_expression(message_expr)?;

    Ok(Statement::Error {
        message,
        location: Some(ast_location),
    })
}

fn parse_on_error_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    // This should be handled as an expression, not a statement
    // Convert the onError block to an expression statement
    let expr = super::expression_parser::parse_expression(pair)?;
    Ok(Statement::Expression {
        expr,
        location: Some(ast_location),
    })
}

fn parse_standalone_on_error_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut inner = pair.into_inner();

    // Skip "onError" and ":" tokens
    inner.next(); // "onError"
    inner.next(); // ":"

    // Parse the indented block
    let block_pair = inner.next().ok_or_else(|| CompilerError::Syntax {
        context: Box::new(crate::error::ErrorContext {
            message: "Expected block after onError:".to_string(),
            location: Some(ast_location.clone()),
            help: Some("Add indented statements after onError:".to_string()),
            error_type: crate::error::ErrorType::Syntax,
            suggestions: vec!["Add indented statements after onError:".to_string()],
            source_snippet: None,
            stack_trace: Vec::new(),
            severity: crate::error::ErrorSeverity::Error,
            error_code: Some("P001".to_string()),
            related_errors: Vec::new(),
        }),
    })?;

    let mut block = Vec::new();
    parse_indented_block_statements(block_pair, &mut block)?;

    Ok(Statement::StandaloneErrorHandler {
        body: block,
        location: Some(ast_location),
    })
}

fn parse_apply_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    // apply_block is a choice rule, so we need to dispatch to the specific type
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::type_apply_block => parse_type_apply_block_statement(inner, ast_location),
        Rule::function_apply_block => parse_function_apply_block_statement(inner, ast_location),
        Rule::method_apply_block => parse_method_apply_block_statement(inner, ast_location),
        Rule::constant_apply_block => parse_constant_apply_block_statement(inner, ast_location),
        _ => Err(CompilerError::parse_error(
            format!("Unexpected apply block type: {:?}", inner.as_rule()),
            Some(ast_location),
            Some("Expected type, function, method, or constant apply block".to_string()),
        )),
    }
}

fn parse_type_apply_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let type_part = parts.next().unwrap();

    // Parse the specific type allowed in type_apply_block: core_type | sized_type | matrix_type | array_type | pairs_type
    let type_ = match type_part.as_rule() {
        Rule::core_type => {
            let type_str = type_part.as_str();
            match type_str {
                "integer" => crate::ast::Type::Integer,
                "number" => crate::ast::Type::Number,
                "boolean" => crate::ast::Type::Boolean,
                "string" => crate::ast::Type::String,
                "void" => crate::ast::Type::Void,
                _ => return Err(CompilerError::parse_error(
                    format!("Unknown core type: {type_str}"),
                    Some(ast_location),
                    Some("Valid core types are: integer, number, boolean, string, void".to_string())
                ))
            }
        },
        Rule::sized_type => parse_type(type_part)?,
        Rule::matrix_type => parse_type(type_part)?,
        Rule::list_type => parse_type(type_part)?,
        Rule::pairs_type => parse_type(type_part)?,
        _ => return Err(CompilerError::parse_error(
            format!("Unexpected type in type apply block: {:?}", type_part.as_rule()),
            Some(ast_location),
            Some("Type apply blocks only support core types, sized types, matrix types, array types, and pairs types".to_string())
        ))
    };

    let mut assignments = Vec::new();
    for part in parts {
        match part.as_rule() {
            Rule::indented_variable_assignments => {
                for assignment_pair in part.into_inner() {
                    if assignment_pair.as_rule() == Rule::variable_assignment {
                        let mut assignment_parts = assignment_pair.into_inner();
                        let name = assignment_parts.next().unwrap().as_str().to_string();
                        let initializer = assignment_parts
                            .next()
                            .map(|expr_pair| parse_expression(expr_pair))
                            .transpose()?;

                        assignments.push(crate::ast::VariableAssignment { name, initializer });
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::TypeApplyBlock {
        type_,
        assignments,
        location: Some(ast_location),
    })
}

fn parse_function_apply_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let function_name = parts.next().unwrap().as_str().to_string();

    let mut expressions = Vec::new();
    for part in parts {
        match part.as_rule() {
            Rule::indented_expressions => {
                for expr_pair in part.into_inner() {
                    if expr_pair.as_rule() == Rule::expression {
                        expressions.push(parse_expression(expr_pair)?);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::FunctionApplyBlock {
        function_name,
        expressions,
        location: Some(ast_location),
    })
}

fn parse_method_apply_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let method_call_chain_part = parts.next().unwrap();

    // Parse the method call chain: object.method or object.method1.method2
    let mut chain_parts = method_call_chain_part.into_inner();
    let object_name = chain_parts.next().unwrap().as_str().to_string();
    let mut method_chain = Vec::new();

    for part in chain_parts {
        if part.as_rule() == Rule::identifier {
            method_chain.push(part.as_str().to_string());
        }
    }

    let mut expressions = Vec::new();
    for part in parts {
        match part.as_rule() {
            Rule::indented_expressions => {
                for expr_pair in part.into_inner() {
                    if expr_pair.as_rule() == Rule::expression {
                        expressions.push(parse_expression(expr_pair)?);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::MethodApplyBlock {
        object_name,
        method_chain,
        expressions,
        location: Some(ast_location),
    })
}

fn parse_constant_apply_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut constants = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::indented_constant_assignments => {
                for constant_pair in part.into_inner() {
                    if constant_pair.as_rule() == Rule::constant_assignment {
                        let mut constant_parts = constant_pair.into_inner();
                        let type_ = parse_type(constant_parts.next().unwrap())?;
                        let name = constant_parts.next().unwrap().as_str().to_string();
                        let value = parse_expression(constant_parts.next().unwrap())?;

                        constants.push(crate::ast::ConstantAssignment { type_, name, value });
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::ConstantApplyBlock {
        constants,
        location: Some(ast_location),
    })
}

fn parse_background_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let expression = parse_expression(parts.next().unwrap())?;

    Ok(Statement::Background {
        expression,
        location: Some(ast_location),
    })
}

fn parse_later_assignment_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let variable = parts.next().unwrap().as_str().to_string();
    let expression = parse_expression(parts.next().unwrap())?;

    Ok(Statement::LaterAssignment {
        variable,
        expression,
        location: Some(ast_location),
    })
}

fn parse_print_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let expression = parse_expression(parts.next().unwrap())?;

    Ok(Statement::Print {
        expression,
        newline: false,
        location: Some(ast_location),
    })
}

fn parse_print_newline_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    use super::expression_parser::parse_argument_expression;

    let mut expressions = Vec::new();

    // Parse argument_list (print(...) + syntax)
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::argument_list {
            for arg_expr in inner.into_inner() {
                if arg_expr.as_rule() == Rule::argument_expression {
                    expressions.push(parse_argument_expression(arg_expr)?);
                }
            }
        }
    }

    if expressions.len() == 1 {
        // Single argument - use Print
        Ok(Statement::Print {
            expression: expressions.into_iter().next().unwrap(),
            newline: true, // print(...) + explicitly adds newline
            location: Some(ast_location),
        })
    } else {
        // Multiple arguments - use PrintBlock
        Ok(Statement::PrintBlock {
            expressions,
            newline: true,
            location: Some(ast_location),
        })
    }
}

fn parse_print_bare_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let expression = parse_expression(parts.next().unwrap())?;

    Ok(Statement::Print {
        expression,
        newline: false, // bare print doesn't add newline by default
        location: Some(ast_location),
    })
}

fn parse_print_parenthesized_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    use super::expression_parser::parse_argument_expression;

    let mut expressions = Vec::new();

    // Parse argument_list
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::argument_list {
            for arg_expr in inner.into_inner() {
                if arg_expr.as_rule() == Rule::argument_expression {
                    expressions.push(parse_argument_expression(arg_expr)?);
                }
            }
        }
    }

    if expressions.len() == 1 {
        // Single argument - use Print
        Ok(Statement::Print {
            expression: expressions.into_iter().next().unwrap(),
            newline: true, // print(...) acts like print(...) + without explicit +
            location: Some(ast_location),
        })
    } else {
        // Multiple arguments - use PrintBlock
        Ok(Statement::PrintBlock {
            expressions,
            newline: true,
            location: Some(ast_location),
        })
    }
}

fn parse_printl_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let expression = parse_expression(parts.next().unwrap())?;

    Ok(Statement::Print {
        expression,
        newline: true,
        location: Some(ast_location),
    })
}

fn parse_println_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let expression = parse_expression(parts.next().unwrap())?;

    Ok(Statement::Print {
        expression,
        newline: true,
        location: Some(ast_location),
    })
}

fn parse_import_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut imports = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::import_list => {
                for import_pair in part.into_inner() {
                    if import_pair.as_rule() == Rule::import_item {
                        // Parse import item: Math, Math.sqrt, Utils as U, Json.decode as jd
                        let mut item_parts = import_pair.into_inner();
                        let first_part = item_parts.next().unwrap().as_str();

                        let import_item = if let Some(second_part) = item_parts.next() {
                            let second_str = second_part.as_str();
                            if second_str == "as" {
                                // Format: ModuleName as Alias
                                let alias = item_parts.next().unwrap().as_str();
                                crate::ast::ImportItem {
                                    name: first_part.to_string(),
                                    alias: Some(alias.to_string()),
                                }
                            } else {
                                // Format: ModuleName.symbol or ModuleName.symbol as alias
                                let symbol_name = format!("{first_part}.{second_str}");
                                if let Some(as_keyword) = item_parts.next() {
                                    if as_keyword.as_str() == "as" {
                                        let alias = item_parts.next().unwrap().as_str();
                                        crate::ast::ImportItem {
                                            name: symbol_name,
                                            alias: Some(alias.to_string()),
                                        }
                                    } else {
                                        crate::ast::ImportItem {
                                            name: symbol_name,
                                            alias: None,
                                        }
                                    }
                                } else {
                                    crate::ast::ImportItem {
                                        name: symbol_name,
                                        alias: None,
                                    }
                                }
                            }
                        } else {
                            // Simple module import: Math
                            crate::ast::ImportItem {
                                name: first_part.to_string(),
                                alias: None,
                            }
                        };

                        imports.push(import_item);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::Import {
        imports,
        location: Some(ast_location),
    })
}

fn parse_private_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    // For now, treat private blocks as import statements with a special marker
    // This is a simplified implementation - in a full implementation, you'd want
    // to handle private visibility properly
    let mut private_items = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::indented_private_list => {
                for private_pair in part.into_inner() {
                    match private_pair.as_rule() {
                        Rule::private_item => {
                            // Handle private function or identifier
                            let mut item_inner = private_pair.into_inner();
                            if let Some(item) = item_inner.next() {
                                match item.as_rule() {
                                    Rule::identifier => {
                                        private_items.push(crate::ast::ImportItem {
                                            name: format!("private:{}", item.as_str()),
                                            alias: None,
                                        });
                                    }
                                    Rule::function_in_block => {
                                        // Handle private function - parse as regular function but mark as private
                                        // This is a simplified approach
                                        private_items.push(crate::ast::ImportItem {
                                            name: "private:function".to_string(),
                                            alias: None,
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Statement::Import {
        imports: private_items,
        location: Some(ast_location),
    })
}
