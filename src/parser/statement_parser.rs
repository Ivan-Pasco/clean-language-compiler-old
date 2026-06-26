use super::expression_parser::{parse_expression, parse_start_expression};
use super::parser_impl::parse_validate_check_stmt;
use super::type_parser::parse_type;
use super::Rule;
use super::{convert_to_ast_location, get_location};
use crate::ast::{AssignmentTarget, Expression, ResetTarget, Statement};
use crate::error::CompilerError;
use pest::iterators::Pair;

pub fn parse_statement(pair: Pair<Rule>) -> Result<Statement, CompilerError> {
    let ast_location = convert_to_ast_location(&get_location(&pair));
    let inner = pair
        .into_inner()
        .next()
        .expect("invariant: statement grammar child");

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
        Rule::break_stmt => Ok(Statement::Break {
            location: Some(ast_location),
        }),
        Rule::continue_stmt => Ok(Statement::Continue {
            location: Some(ast_location),
        }),
        Rule::error_stmt => parse_error_statement(inner, ast_location),
        Rule::require_stmt => parse_require_statement(inner, ast_location),
        Rule::on_error_block => parse_on_error_block_statement(inner, ast_location),
        Rule::standalone_on_error => parse_standalone_on_error_statement(inner, ast_location),
        Rule::apply_block => parse_apply_block_statement(inner, ast_location),
        Rule::type_apply_block => parse_type_apply_block_statement(inner, ast_location),
        Rule::function_apply_block => parse_function_apply_block_statement(inner, ast_location),
        Rule::method_apply_block => parse_method_apply_block_statement(inner, ast_location),
        Rule::constant_apply_block => parse_constant_apply_block_statement(inner, ast_location),
        Rule::framework_block => parse_framework_block_statement(inner, ast_location),
        Rule::validate_check_stmt => parse_validate_check_stmt(inner),
        Rule::reset_stmt => parse_reset_statement(inner, ast_location),
        Rule::background_stmt => parse_background_statement(inner, ast_location),
        Rule::later_assignment => parse_later_assignment_statement(inner, ast_location),
        Rule::import_block => parse_import_block_statement(inner, ast_location),
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
        let inner = parts.next().expect("invariant: variable_decl inner child");
        match inner.as_rule() {
            Rule::variable_decl_without_assignment => {
                let mut inner_parts = inner.into_inner();
                let type_part = inner_parts
                    .next()
                    .expect("invariant: variable_decl_without_assignment type");
                let name_part = inner_parts
                    .next()
                    .expect("invariant: variable_decl_without_assignment name");
                (type_part, name_part, None)
            }
            Rule::variable_decl_with_assignment => {
                let mut inner_parts = inner.into_inner();
                let type_part = inner_parts
                    .next()
                    .expect("invariant: variable_decl_with_assignment type");
                let name_part = inner_parts
                    .next()
                    .expect("invariant: variable_decl_with_assignment name");
                let initializer_part = inner_parts
                    .next()
                    .expect("invariant: variable_decl_with_assignment initializer");
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
        let type_part = parts
            .next()
            .expect("invariant: variable_decl direct type child");
        let name_part = parts
            .next()
            .expect("invariant: variable_decl direct name child");
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
    let target_part = parts.next().expect("invariant: assignment target child");
    let value = parse_expression(parts.next().expect("invariant: assignment value child"))?;

    // Check if this is an assignment_target with property access or array access
    if target_part.as_rule() == Rule::assignment_target {
        let target_parts: Vec<_> = target_part.clone().into_inner().collect();

        // If we have more than just an identifier (i.e., we have property access or array access)
        if target_parts.len() > 1 {
            let identifier = target_parts[0].as_str().to_string();

            // Check if it's a property access (second part is an identifier)
            if target_parts[1].as_rule() == Rule::identifier {
                let property_name = target_parts[1].as_str().to_string();

                return Ok(Statement::Expression {
                    expr: Expression::PropertyAssignment {
                        object: Box::new(Expression::Variable(identifier)),
                        property: property_name,
                        value: Box::new(value),
                        location: ast_location.clone(),
                    },
                    location: Some(ast_location),
                });
            }
            // Check if it's an array access (second part is an expression for the index)
            else if target_parts[1].as_rule() == Rule::additive_expression {
                let index = parse_expression(target_parts[1].clone())?;

                return Ok(Statement::Expression {
                    expr: Expression::ListAssignment {
                        list: Box::new(Expression::Variable(identifier)),
                        index: Box::new(index),
                        value: Box::new(value),
                        location: ast_location.clone(),
                    },
                    location: Some(ast_location),
                });
            }
        }
    }

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

    // Regular variable assignment — foundation/spec/grammar.ebnf `assignment_target`
    let target_str = target_part.as_str().trim().to_string();
    Ok(Statement::Assignment {
        target: AssignmentTarget::Variable(target_str),
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
                let then_block = parts.next().expect("invariant: else_if then_block child"); // This should be the indented_block
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
            Rule::simple_indented_block => {
                // Handle simple_indented_block -> statement (direct parsing)
                for stmt in stmt_pair.into_inner() {
                    if stmt.as_rule() == Rule::statement {
                        statements.push(parse_statement(stmt)?);
                    }
                }
            }
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
    let condition = parse_expression(parts.next().expect("invariant: if_stmt condition child"))?;

    let mut then_branch = Vec::new();
    let mut else_branch = None;

    for part in parts {
        match part.as_rule() {
            Rule::indented_block => {
                // This is the then branch - use the shared parsing function
                parse_indented_block_statements(part, &mut then_branch)?;
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
    let condition = parse_expression(parts.next().expect("invariant: while_stmt condition child"))?;

    let mut body = Vec::new();
    for part in parts {
        if part.as_rule() == Rule::indented_block {
            parse_indented_block_statements(part, &mut body)?;
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
    let iterator = parts
        .next()
        .expect("invariant: iterate_stmt iterator child")
        .as_str()
        .to_string();
    let collection = parse_expression(
        parts
            .next()
            .expect("invariant: iterate_stmt collection child"),
    )?;

    let mut body = Vec::new();
    for part in parts {
        if part.as_rule() == Rule::indented_block {
            // Use the shared helper that properly handles simple_indented_block and function_nested_block
            parse_indented_block_statements(part, &mut body)?;
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
    let iterator = parts
        .next()
        .expect("invariant: range_iterate_stmt iterator child")
        .as_str()
        .to_string();
    let start = parse_expression(
        parts
            .next()
            .expect("invariant: range_iterate_stmt start child"),
    )?;
    let end = parse_expression(
        parts
            .next()
            .expect("invariant: range_iterate_stmt end child"),
    )?;

    let mut step = None;
    let mut body = Vec::new();

    for part in parts {
        match part.as_rule() {
            Rule::expression => {
                step = Some(parse_expression(part)?);
            }
            Rule::indented_block => {
                // Use the shared helper that properly handles simple_indented_block and function_nested_block
                parse_indented_block_statements(part, &mut body)?;
            }
            _ => {}
        }
    }

    Ok(Statement::RangeIterate {
        iterator,
        start,
        end,
        step,
        inclusive: true, // pest parser grammar doesn't expose inclusive; default true
        body,
        location: Some(ast_location),
    })
}

fn parse_test_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let name = parts
        .next()
        .expect("invariant: test_stmt name child")
        .as_str()
        .trim_matches('"')
        .to_string();

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
    let message_expr = inner.next().expect("invariant: error_stmt message child");
    let message = parse_expression(message_expr)?;

    Ok(Statement::Error {
        message,
        location: Some(ast_location),
    })
}

/// Parse require statement: require <condition>
/// Declares a precondition that must be true at runtime
fn parse_require_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    // require <expression>
    let mut inner = pair.into_inner();
    let condition_expr = inner
        .next()
        .expect("invariant: require_stmt condition child");
    let condition = parse_expression(condition_expr)?;

    Ok(Statement::Require {
        condition,
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
    let inner = pair
        .into_inner()
        .next()
        .expect("invariant: apply_block inner child");
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
    let type_part = parts
        .next()
        .expect("invariant: type_apply_block type child");

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
        if part.as_rule() == Rule::indented_variable_assignments {
            for assignment_pair in part.into_inner() {
                if assignment_pair.as_rule() == Rule::variable_assignment {
                    let mut assignment_parts = assignment_pair.into_inner();
                    let name = assignment_parts
                        .next()
                        .expect("invariant: variable_assignment name child")
                        .as_str()
                        .to_string();
                    let initializer = assignment_parts
                        .next()
                        .map(|expr_pair| parse_expression(expr_pair))
                        .transpose()?;

                    assignments.push(crate::ast::VariableAssignment { name, initializer });
                }
            }
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
    let function_name = parts
        .next()
        .expect("invariant: function_apply_block name child")
        .as_str()
        .to_string();

    let mut expressions = Vec::new();
    for part in parts {
        if part.as_rule() == Rule::indented_expressions {
            for expr_pair in part.into_inner() {
                if expr_pair.as_rule() == Rule::expression {
                    expressions.push(parse_expression(expr_pair)?);
                }
            }
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
    let method_call_chain_part = parts
        .next()
        .expect("invariant: method_apply_block chain child");

    // Parse the method call chain: object.method or object.method1.method2
    let mut chain_parts = method_call_chain_part.into_inner();
    let object_name = chain_parts
        .next()
        .expect("invariant: method_call_chain object child")
        .as_str()
        .to_string();
    let mut method_chain = Vec::new();

    for part in chain_parts {
        if part.as_rule() == Rule::identifier {
            method_chain.push(part.as_str().to_string());
        }
    }

    let mut expressions = Vec::new();
    for part in parts {
        if part.as_rule() == Rule::indented_expressions {
            for expr_pair in part.into_inner() {
                if expr_pair.as_rule() == Rule::expression {
                    expressions.push(parse_expression(expr_pair)?);
                }
            }
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
        if part.as_rule() == Rule::indented_constant_assignments {
            for constant_pair in part.into_inner() {
                if constant_pair.as_rule() == Rule::constant_assignment {
                    let mut constant_parts = constant_pair.into_inner();
                    let type_ = parse_type(
                        constant_parts
                            .next()
                            .expect("invariant: constant_assignment type child"),
                    )?;
                    let name = constant_parts
                        .next()
                        .expect("invariant: constant_assignment name child")
                        .as_str()
                        .to_string();
                    let value = parse_expression(
                        constant_parts
                            .next()
                            .expect("invariant: constant_assignment value child"),
                    )?;

                    constants.push(crate::ast::ConstantAssignment { type_, name, value });
                }
            }
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
    let expression = parse_expression(
        parts
            .next()
            .expect("invariant: background_stmt expression child"),
    )?;

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
    let variable = parts
        .next()
        .expect("invariant: later_assignment variable child")
        .as_str()
        .to_string();
    let expression = parse_expression(
        parts
            .next()
            .expect("invariant: later_assignment expression child"),
    )?;

    Ok(Statement::LaterAssignment {
        variable,
        expression,
        location: Some(ast_location),
    })
}

fn parse_print_newline_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    use super::expression_parser::parse_argument_item;

    let mut expressions = Vec::new();

    // Parse argument_list (print(...) + syntax)
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::argument_list {
            for arg_expr in inner.into_inner() {
                if matches!(
                    arg_expr.as_rule(),
                    Rule::argument_item | Rule::argument_expression
                ) {
                    expressions.push(parse_argument_item(arg_expr)?);
                }
            }
        }
    }

    if expressions.len() == 1 {
        // Single argument - use Print
        Ok(Statement::Print {
            expression: expressions
                .into_iter()
                .next()
                .expect("invariant: single-element vec verified above"),
            newline: true, // print(...) + adds newline (legacy pest-based parser path)
            location: Some(ast_location),
        })
    } else {
        // Multiple arguments - use PrintBlock
        Ok(Statement::PrintBlock {
            expressions,
            newline: true, // print(...) + adds newline (legacy pest-based parser path)
            location: Some(ast_location),
        })
    }
}

fn parse_print_bare_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();
    let expression = parse_expression(
        parts
            .next()
            .expect("invariant: print_bare_stmt expression child"),
    )?;

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
    use super::expression_parser::parse_argument_item;

    let mut expressions = Vec::new();

    // Parse argument_list
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::argument_list {
            for arg_expr in inner.into_inner() {
                if matches!(
                    arg_expr.as_rule(),
                    Rule::argument_item | Rule::argument_expression
                ) {
                    expressions.push(parse_argument_item(arg_expr)?);
                }
            }
        }
    }

    if expressions.len() == 1 {
        // Single argument - use Print
        Ok(Statement::Print {
            expression: expressions
                .into_iter()
                .next()
                .expect("invariant: single-element vec verified above"),
            newline: false, // legacy pest-based parser path (not used by 7-stage pipeline)
            location: Some(ast_location),
        })
    } else {
        // Multiple arguments - use PrintBlock
        Ok(Statement::PrintBlock {
            expressions,
            newline: false,
            location: Some(ast_location),
        })
    }
}

fn parse_import_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut imports = Vec::new();

    for part in pair.into_inner() {
        if part.as_rule() == Rule::import_list {
            for import_pair in part.into_inner() {
                if import_pair.as_rule() == Rule::import_item {
                    // Parse import item: Math, Math.sqrt, Utils as U, Json.decode as jd
                    let mut item_parts = import_pair.into_inner();
                    let first_part = item_parts
                        .next()
                        .expect("invariant: import_item first child")
                        .as_str();

                    let import_item = if let Some(second_part) = item_parts.next() {
                        let second_str = second_part.as_str();
                        if second_str == "as" {
                            // Format: ModuleName as Alias
                            let alias = item_parts
                                .next()
                                .expect("invariant: import_item alias child")
                                .as_str();
                            crate::ast::ImportItem {
                                name: first_part.to_string(),
                                alias: Some(alias.to_string()),
                                is_file_import: false,
                            }
                        } else {
                            // Format: ModuleName.symbol or ModuleName.symbol as alias
                            let symbol_name = format!("{first_part}.{second_str}");
                            if let Some(as_keyword) = item_parts.next() {
                                if as_keyword.as_str() == "as" {
                                    let alias = item_parts
                                        .next()
                                        .expect("invariant: import_item dot alias child")
                                        .as_str();
                                    crate::ast::ImportItem {
                                        name: symbol_name,
                                        alias: Some(alias.to_string()),
                                        is_file_import: false,
                                    }
                                } else {
                                    crate::ast::ImportItem {
                                        name: symbol_name,
                                        alias: None,
                                        is_file_import: false,
                                    }
                                }
                            } else {
                                crate::ast::ImportItem {
                                    name: symbol_name,
                                    alias: None,
                                    is_file_import: false,
                                }
                            }
                        }
                    } else {
                        // Simple module import: Math
                        crate::ast::ImportItem {
                            name: first_part.to_string(),
                            alias: None,
                            is_file_import: false,
                        }
                    };

                    imports.push(import_item);
                }
            }
        }
    }

    Ok(Statement::Import {
        imports,
        location: Some(ast_location),
    })
}

/// Parse framework block statement (e.g., endpoints:, data, component)
/// These blocks are captured as raw text and expanded by plugins
fn parse_framework_block_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();

    // First part is the block name (identifier)
    let name_pair = parts.next().ok_or_else(|| {
        CompilerError::parse_error(
            "Missing block name in framework block".to_string(),
            Some(ast_location.clone()),
            None,
        )
    })?;
    let name = name_pair.as_str().to_string();

    // Second part is the content (raw text captured by framework_block_content)
    let content_pair = parts.next().ok_or_else(|| {
        CompilerError::parse_error(
            format!("Missing content in framework block '{}'", name),
            Some(ast_location.clone()),
            None,
        )
    })?;

    // Extract raw content and clean up indentation
    let raw_content = content_pair.as_str();
    let content = clean_framework_block_content(raw_content);

    tracing::debug!(
        block_name = %name,
        content_length = content.len(),
        "Parsed framework block"
    );

    Ok(Statement::FrameworkBlock {
        name,
        content,
        attributes: vec![], // Attributes parsed separately when using @decorator syntax
        location: Some(ast_location),
    })
}

/// Clean up framework block content by removing common indentation
fn clean_framework_block_content(raw_content: &str) -> String {
    let lines: Vec<&str> = raw_content.lines().collect();

    if lines.is_empty() {
        return String::new();
    }

    // Find minimum indentation (count leading tabs)
    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|&c| c == '\t').count())
        .min()
        .unwrap_or(0);

    // Remove the common indentation from each line
    lines
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                line.chars().skip(min_indent).collect()
            }
        })
        .collect::<Vec<String>>()
        .join("\n")
        .trim()
        .to_string()
}

// ============================================================================
// STATE MANAGEMENT PARSERS
// ============================================================================

/// Parse reset statement: reset count OR reset state
fn parse_reset_statement(
    pair: Pair<Rule>,
    ast_location: crate::ast::SourceLocation,
) -> Result<Statement, CompilerError> {
    let mut parts = pair.into_inner();

    // The grammar captures either "state" keyword or an identifier
    let target = if let Some(target_pair) = parts.next() {
        let target_str = target_pair.as_str();
        if target_str == "state" {
            ResetTarget::AllState
        } else {
            ResetTarget::Variable(target_str.to_string())
        }
    } else {
        // Default to all state if no target specified
        ResetTarget::AllState
    };

    Ok(Statement::ResetStmt {
        target,
        location: Some(ast_location),
    })
}
