use super::Rule;
use super::{convert_to_ast_location, get_location};
use crate::ast::{BinaryOperator, Expression, StringPart, UnaryOperator, Value};
use crate::error::CompilerError;
use pest::iterators::Pair;

// Helper function to parse integer literals with different bases
fn parse_integer_literal(
    pair: Pair<Rule>,
    location: &super::SourceLocation,
) -> Result<Expression, CompilerError> {
    let num_str = pair.as_str();
    let location_ast = convert_to_ast_location(location);

    let value = match pair.as_rule() {
        Rule::hex_integer => {
            // Remove "0x" or "0X" prefix and handle negative sign
            let (is_negative, hex_part) = if num_str.starts_with('-') {
                (true, &num_str[3..]) // Skip "-0x"
            } else {
                (false, &num_str[2..]) // Skip "0x"
            };

            match i64::from_str_radix(hex_part, 16) {
                Ok(val) => {
                    if is_negative {
                        -val
                    } else {
                        val
                    }
                }
                Err(_) => {
                    return Err(CompilerError::parse_error(
                        format!("Invalid hexadecimal integer: {num_str}"),
                        Some(location_ast),
                        Some("Check that the hexadecimal digits are valid".to_string()),
                    ))
                }
            }
        }
        Rule::binary_integer => {
            // Remove "0b" or "0B" prefix and handle negative sign
            let (is_negative, bin_part) = if num_str.starts_with('-') {
                (true, &num_str[3..]) // Skip "-0b"
            } else {
                (false, &num_str[2..]) // Skip "0b"
            };

            match i64::from_str_radix(bin_part, 2) {
                Ok(val) => {
                    if is_negative {
                        -val
                    } else {
                        val
                    }
                }
                Err(_) => {
                    return Err(CompilerError::parse_error(
                        format!("Invalid binary integer: {num_str}"),
                        Some(location_ast),
                        Some("Check that the binary digits are valid".to_string()),
                    ))
                }
            }
        }
        Rule::octal_integer => {
            // Remove "0o" or "0O" prefix and handle negative sign
            let (is_negative, oct_part) = if num_str.starts_with('-') {
                (true, &num_str[3..]) // Skip "-0o"
            } else {
                (false, &num_str[2..]) // Skip "0o"
            };

            match i64::from_str_radix(oct_part, 8) {
                Ok(val) => {
                    if is_negative {
                        -val
                    } else {
                        val
                    }
                }
                Err(_) => {
                    return Err(CompilerError::parse_error(
                        format!("Invalid octal integer: {num_str}"),
                        Some(location_ast),
                        Some("Check that the octal digits are valid".to_string()),
                    ))
                }
            }
        }
        Rule::decimal_integer => match num_str.parse::<i64>() {
            Ok(val) => val,
            Err(_) => {
                return Err(CompilerError::parse_error(
                    format!("Invalid decimal integer: {num_str}"),
                    Some(location_ast),
                    Some("Check that the integer is in a valid format".to_string()),
                ))
            }
        },
        _ => {
            return Err(CompilerError::parse_error(
                format!("Unexpected integer type: {:?}", pair.as_rule()),
                Some(location_ast),
                Some("Expected a valid integer literal".to_string()),
            ))
        }
    };

    Ok(Expression::Literal(Value::Integer(value)))
}

// Helper function to convert location from parser format to AST format

pub fn parse_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    match pair.as_rule() {
        Rule::expression => {
            // Handle the top-level expression rule
            let inner = pair.into_inner().next().unwrap();
            parse_expression(inner)
        }
        Rule::on_error_expr => {
            // Handle onError expression
            let location = convert_to_ast_location(&get_location(&pair));
            let mut inner = pair.into_inner();
            let expression = parse_expression(inner.next().unwrap())?;
            let fallback = parse_expression(inner.next().unwrap())?;

            Ok(Expression::OnError {
                expression: Box::new(expression),
                fallback: Box::new(fallback),
                location,
            })
        }
        Rule::on_error_block => {
            // Handle onError block
            let location = convert_to_ast_location(&get_location(&pair));
            let mut inner = pair.into_inner();
            let expression = parse_expression(inner.next().unwrap())?;

            // Parse the indented block
            let block_pair = inner.next().unwrap();
            let mut error_handler = Vec::new();

            for stmt_pair in block_pair.into_inner() {
                if stmt_pair.as_rule() == Rule::statement {
                    error_handler.push(crate::parser::statement_parser::parse_statement(stmt_pair)?);
                }
            }

            Ok(Expression::OnErrorBlock {
                expression: Box::new(expression),
                error_handler,
                location,
            })
        }
        Rule::base_expression => {
            parse_base_expression(pair)
        }
        Rule::logical_expression => {
            parse_logical_expression(pair)
        }
        Rule::comparison_expression => {
            parse_comparison_expression(pair)
        }
        Rule::unary_expression => {
            parse_unary_expression(pair)
        }
        Rule::arithmetic_expression => {
            parse_arithmetic_expression(pair)
        }
        Rule::additive_expression => {
            parse_additive_expression(pair)
        }
        Rule::multiplicative_expression => {
            parse_multiplicative_expression(pair)
        }
        Rule::power_expression => {
            parse_power_expression(pair)
        }
        Rule::error_variable => {
            // Parse error variable
            let location = convert_to_ast_location(&get_location(&pair));
            Ok(Expression::ErrorVariable { location })
        }
        Rule::start_expr => {
            // Parse start expression - async start
            let location = convert_to_ast_location(&get_location(&pair));
            let inner = pair.into_inner().next().unwrap();
            let expr = parse_expression(inner)?;
            Ok(Expression::StartExpression {
                expression: Box::new(expr),
                location,
            })
        }
        Rule::function_call => {
            // Parse function call directly
            parse_function_call(pair)
        }
        Rule::conditional_expr => {
            // Parse conditional expression directly
            parse_conditional_expression(pair)
        }
        Rule::parenthesized_expr => {
            // Parse parenthesized expression
            let inner = pair.into_inner().next().unwrap();
            parse_expression(inner)
        }
        _ => {
            Err(CompilerError::parse_error(
                format!("Unsupported expression rule: {:?}", pair.as_rule()),
                Some(convert_to_ast_location(&get_location(&pair))),
                Some("Expected expression, on_error_expr, on_error_block, base_expression, start_expr, conditional_expr, or error_variable".to_string())
            ))
        }
    }
}

pub fn parse_base_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    if let Some(item) = pair.into_inner().next() {
        match item.as_rule() {
            Rule::logical_expression => {
                return parse_logical_expression(item);
            }
            Rule::conditional_expr => {
                return parse_conditional_expression(item);
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unexpected rule in base expression: {:?}", item.as_rule()),
                    Some(convert_to_ast_location(&get_location(&item))),
                    Some("Expected logical expression or conditional expression".to_string()),
                ))
            }
        }
    }

    Err(CompilerError::parse_error(
        "Empty base expression".to_string(),
        None,
        Some("Base expression must contain a logical or conditional expression".to_string()),
    ))
}

pub fn parse_logical_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::comparison_expression => {
                expr_stack.push(parse_comparison_expression(item)?);
            }
            Rule::logical_op => {
                let op = match item.as_str() {
                    "and" => BinaryOperator::And,
                    "or" => BinaryOperator::Or,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid logical operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some("Valid logical operators are: and, or".to_string()),
                        ))
                    }
                };
                op_stack.push(op);
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!(
                        "Unexpected rule in logical expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    Some("Expected comparison expression or logical operator".to_string()),
                ))
            }
        }
    }

    // Build the expression tree from the stacks
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty logical expression".to_string(),
            None,
            Some("Logical expression must contain at least one comparison".to_string()),
        ));
    }

    let mut result = expr_stack.remove(0);
    let mut i = 0;

    while i < op_stack.len() && i < expr_stack.len() {
        let right = expr_stack.remove(0);
        result = Expression::Binary(Box::new(result), op_stack[i].clone(), Box::new(right));
        i += 1;
    }

    Ok(result)
}

pub fn parse_comparison_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::unary_expression => {
                expr_stack.push(parse_unary_expression(item)?);
            }
            Rule::comparison_op => {
                let op = match item.as_str() {
                    "==" => BinaryOperator::Equal,
                    "!=" => BinaryOperator::NotEqual,
                    "<" => BinaryOperator::Less,
                    "<=" => BinaryOperator::LessEqual,
                    ">" => BinaryOperator::Greater,
                    ">=" => BinaryOperator::GreaterEqual,
                    "is" => BinaryOperator::Is,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid comparison operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some(
                                "Valid comparison operators are: ==, !=, <, <=, >, >=, is"
                                    .to_string(),
                            ),
                        ))
                    }
                };
                op_stack.push(op);
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!(
                        "Unexpected rule in comparison expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    Some("Expected arithmetic expression or comparison operator".to_string()),
                ))
            }
        }
    }

    // Build the expression tree from the stacks
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty comparison expression".to_string(),
            None,
            Some(
                "Comparison expression must contain at least one arithmetic expression".to_string(),
            ),
        ));
    }

    let mut result = expr_stack.remove(0);
    let mut i = 0;

    while i < op_stack.len() && i < expr_stack.len() {
        let right = expr_stack.remove(0);
        result = Expression::Binary(Box::new(result), op_stack[i].clone(), Box::new(right));
        i += 1;
    }

    Ok(result)
}

pub fn parse_unary_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut unary_ops = Vec::new();
    let mut additive_expr = None;

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::unary_op => {
                let op = match item.as_str() {
                    "not" => UnaryOperator::Not,
                    "-" => UnaryOperator::Negate,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid unary operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some("Valid unary operators are: not, -".to_string()),
                        ))
                    }
                };
                unary_ops.push(op);
            }
            Rule::additive_expression => {
                additive_expr = Some(parse_additive_expression(item)?);
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unexpected rule in unary expression: {:?}", item.as_rule()),
                    Some(convert_to_ast_location(&get_location(&item))),
                    Some("Expected unary operator or additive expression".to_string()),
                ))
            }
        }
    }

    let mut result = additive_expr.ok_or_else(|| {
        CompilerError::parse_error(
            "Missing additive expression in unary expression".to_string(),
            None,
            Some("Unary expression must contain an additive expression".to_string()),
        )
    })?;

    // Apply unary operators from right to left (since we parsed left to right)
    for op in unary_ops.into_iter().rev() {
        result = Expression::Unary(op, Box::new(result));
    }

    Ok(result)
}

pub fn parse_arithmetic_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    // For backward compatibility, just delegate to additive_expression
    for item in pair.into_inner() {
        if item.as_rule() == Rule::additive_expression {
            return parse_additive_expression(item);
        }
    }

    Err(CompilerError::parse_error(
        "Empty arithmetic expression".to_string(),
        None,
        Some("Arithmetic expression must contain an additive expression".to_string()),
    ))
}

pub fn parse_additive_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::multiplicative_expression => {
                expr_stack.push(parse_multiplicative_expression(item)?);
            }
            Rule::additive_op => {
                let op = match item.as_str() {
                    "+" => BinaryOperator::Add,
                    "-" => BinaryOperator::Subtract,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid additive operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some("Valid additive operators are: +, -".to_string()),
                        ))
                    }
                };
                op_stack.push(op);
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!(
                        "Unexpected rule in additive expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    Some("Expected multiplicative expression or additive operator".to_string()),
                ))
            }
        }
    }

    // Build the expression tree (left-associative)
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty additive expression".to_string(),
            None,
            Some(
                "Additive expression must contain at least one multiplicative expression"
                    .to_string(),
            ),
        ));
    }

    let mut result = expr_stack.remove(0);
    let mut i = 0;

    while i < op_stack.len() && i < expr_stack.len() {
        let right = expr_stack.remove(0);
        result = Expression::Binary(Box::new(result), op_stack[i].clone(), Box::new(right));
        i += 1;
    }

    Ok(result)
}

pub fn parse_multiplicative_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::power_expression => {
                expr_stack.push(parse_power_expression(item)?);
            }
            Rule::multiplicative_op => {
                let op = match item.as_str() {
                    "*" => BinaryOperator::Multiply,
                    "/" => BinaryOperator::Divide,
                    "%" => BinaryOperator::Modulo,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid multiplicative operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some("Valid multiplicative operators are: *, /, %".to_string()),
                        ))
                    }
                };
                op_stack.push(op);
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!(
                        "Unexpected rule in multiplicative expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    Some("Expected power expression or multiplicative operator".to_string()),
                ))
            }
        }
    }

    // Build the expression tree (left-associative)
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty multiplicative expression".to_string(),
            None,
            Some(
                "Multiplicative expression must contain at least one power expression".to_string(),
            ),
        ));
    }

    let mut result = expr_stack.remove(0);
    let mut i = 0;

    while i < op_stack.len() && i < expr_stack.len() {
        let right = expr_stack.remove(0);
        result = Expression::Binary(Box::new(result), op_stack[i].clone(), Box::new(right));
        i += 1;
    }

    Ok(result)
}

pub fn parse_power_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::primary => {
                expr_stack.push(parse_primary(item)?);
            }
            Rule::power_op => {
                let op = match item.as_str() {
                    "^" => BinaryOperator::Power,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid power operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some("Valid power operator is: ^".to_string()),
                        ))
                    }
                };
                op_stack.push(op);
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unexpected rule in power expression: {:?}", item.as_rule()),
                    Some(convert_to_ast_location(&get_location(&item))),
                    Some("Expected primary expression or power operator".to_string()),
                ))
            }
        }
    }

    // Build the expression tree (right-associative for power operator)
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty power expression".to_string(),
            None,
            Some("Power expression must contain at least one primary expression".to_string()),
        ));
    }

    // For right-associativity of power operator, we build from right to left
    let mut result = expr_stack.pop().unwrap();

    while let (Some(left), Some(op)) = (expr_stack.pop(), op_stack.pop()) {
        result = Expression::Binary(Box::new(left), op, Box::new(result));
    }

    Ok(result)
}

pub fn parse_primary(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let inner = pair.clone().into_inner().next().ok_or_else(|| {
        CompilerError::parse_error(
            "Empty primary expression".to_string(),
            Some(convert_to_ast_location(&location)),
            Some("Expected a value inside the primary expression".to_string()),
        )
    })?;

    match inner.as_rule() {
        Rule::number => parse_number_literal(inner),
        Rule::hex_integer | Rule::binary_integer | Rule::octal_integer | Rule::decimal_integer => {
            // Handle specific integer types directly
            parse_integer_literal(inner, &location)
        }
        Rule::integer => {
            // Handle legacy integer rule (fallback)
            let integer_inner = inner.into_inner().next().unwrap();
            parse_integer_literal(integer_inner, &location)
        }
        Rule::float => {
            let num_str = inner.as_str();
            num_str
                .parse::<f64>()
                .map(Value::Number)
                .map(Expression::Literal)
                .map_err(|_| {
                    CompilerError::parse_error(
                        format!("Invalid float: {num_str}"),
                        Some(convert_to_ast_location(&location)),
                        Some("Check that the float is in a valid format".to_string()),
                    )
                })
        }
        Rule::boolean => {
            let value = match inner.as_str() {
                "true" => true,
                "false" => false,
                _ => {
                    return Err(CompilerError::parse_error(
                        format!("Invalid boolean: {}", inner.as_str()),
                        Some(convert_to_ast_location(&location)),
                        Some("Boolean values must be 'true' or 'false'".to_string()),
                    ))
                }
            };
            Ok(Expression::Literal(Value::Boolean(value)))
        }
        Rule::string => parse_string(inner),
        Rule::list_literal => parse_list_literal(inner),
        Rule::matrix_literal => parse_matrix_literal(inner),
        Rule::function_call => parse_function_call(inner),
        Rule::property_method_call => parse_property_method_call(inner),
        Rule::method_call => parse_method_call(inner),
        Rule::chained_method_call => parse_chained_method_call(inner),
        Rule::three_level_method_call => parse_three_level_method_call(inner),
        Rule::static_method_call => parse_static_method_call(inner),
        Rule::property_access => parse_property_access(inner),
        Rule::list_access => parse_list_access(inner),
        Rule::error_variable => {
            // Parse error variable
            Ok(Expression::ErrorVariable {
                location: convert_to_ast_location(&location),
            })
        }
        Rule::identifier => {
            let identifier = inner.as_str();
            Ok(Expression::Variable(identifier.to_string()))
        }
        Rule::expression => {
            // Handle parenthesized expressions: (expression)
            parse_expression(inner)
        }
        Rule::logical_expression => {
            // Handle parenthesized logical expressions: (logical_expression)
            parse_logical_expression(inner)
        }
        Rule::parenthesized_expr => {
            // Handle parenthesized expressions: (parenthesized_expr)
            let inner_expr = inner.into_inner().next().unwrap();
            // parenthesized_expr contains additive_expression, not logical_expression
            parse_additive_expression(inner_expr)
        }
        Rule::conditional_expr => {
            // Handle conditional expressions: if condition then value else value
            parse_conditional_expression(inner)
        }
        Rule::base_call => {
            // Handle base constructor calls: base(args...)
            parse_base_call(inner)
        }
        Rule::start_expr => {
            // Handle start expressions: start expression
            parse_start_expression(inner)
        }
        _ => Err(CompilerError::parse_error(
            format!("Unexpected primary expression: {}", inner.as_str()),
            Some(convert_to_ast_location(&location)),
            Some("Expected a literal, identifier, or function call".to_string()),
        )),
    }
}

fn parse_number_literal(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let num_str = pair.as_str();

    if num_str.contains('.') || num_str.contains('e') || num_str.contains('E') {
        // Float
        num_str
            .parse::<f64>()
            .map(Value::Number)
            .map(Expression::Literal)
            .map_err(|_| {
                CompilerError::parse_error(
                    format!("Invalid float: {num_str}"),
                    None,
                    Some("Check that the float is in a valid format".to_string()),
                )
            })
    } else {
        // Integer
        num_str
            .parse::<i64>()
            .map(Value::Integer)
            .map(Expression::Literal)
            .map_err(|_| {
                CompilerError::parse_error(
                    format!("Invalid integer: {num_str}"),
                    None,
                    Some("Check that the integer is in a valid format".to_string()),
                )
            })
    }
}

pub fn parse_string(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut parts = Vec::new();

    for part in pair.into_inner() {
        match part.as_rule() {
            Rule::string_part => {
                // Handle string_part which contains either string_content or string_interpolation
                for inner_part in part.into_inner() {
                    match inner_part.as_rule() {
                        Rule::string_content => {
                            parts.push(StringPart::Text(inner_part.as_str().to_string()));
                        }
                        Rule::string_interpolation => {
                            // Handle {variable} or {object.property}
                            let mut inner = inner_part.into_inner();
                            let expr_str = inner.next().unwrap().as_str();

                            // Parse the interpolation expression properly
                            // Instead of treating as simple variable, parse as full expression
                            match parse_interpolation_expression(expr_str) {
                                Ok(expr) => {
                                    parts.push(StringPart::Interpolation(expr));
                                }
                                Err(_) => {
                                    // Fallback to simple variable parsing if expression parsing fails
                                    if expr_str.contains('.') {
                                        let parts_split: Vec<&str> = expr_str.split('.').collect();
                                        let object =
                                            Expression::Variable(parts_split[0].to_string());
                                        let property = parts_split[1].to_string();

                                        let location = crate::ast::SourceLocation::default();
                                        let property_access = Expression::PropertyAccess {
                                            object: Box::new(object),
                                            property,
                                            location,
                                        };
                                        parts.push(StringPart::Interpolation(property_access));
                                    } else {
                                        // Simple variable
                                        let variable = Expression::Variable(expr_str.to_string());
                                        parts.push(StringPart::Interpolation(variable));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Rule::string_content => {
                // Direct string_content (shouldn't happen with current grammar, but keeping for safety)
                parts.push(StringPart::Text(part.as_str().to_string()));
            }
            Rule::string_interpolation => {
                // Direct string_interpolation (shouldn't happen with current grammar, but keeping for safety)
                let mut inner = part.into_inner();
                let expr_str = inner.next().unwrap().as_str();

                // Parse the interpolation expression properly
                match parse_interpolation_expression(expr_str) {
                    Ok(expr) => {
                        parts.push(StringPart::Interpolation(expr));
                    }
                    Err(_) => {
                        // Fallback to simple parsing if expression parsing fails
                        if expr_str.contains('.') {
                            let parts_split: Vec<&str> = expr_str.split('.').collect();
                            let object = Expression::Variable(parts_split[0].to_string());
                            let property = parts_split[1].to_string();

                            let location = crate::ast::SourceLocation::default();
                            let property_access = Expression::PropertyAccess {
                                object: Box::new(object),
                                property,
                                location,
                            };
                            parts.push(StringPart::Interpolation(property_access));
                        } else {
                            // Simple variable
                            let variable = Expression::Variable(expr_str.to_string());
                            parts.push(StringPart::Interpolation(variable));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Check if this is a simple string (no interpolation)
    if parts.len() == 1 {
        if let StringPart::Text(text) = &parts[0] {
            // This is a simple string literal, return it as a literal value
            return Ok(Expression::Literal(Value::String(text.clone())));
        }
    } else if parts.is_empty() {
        // Empty string
        return Ok(Expression::Literal(Value::String(String::new())));
    }

    // This has interpolation parts, return as StringInterpolation
    Ok(Expression::StringInterpolation(parts))
}

/// Parse expression within string interpolation braces
/// This handles arithmetic expressions, function calls, property access, etc.
fn parse_interpolation_expression(expr_str: &str) -> Result<Expression, CompilerError> {
    use crate::parser::grammar::Rule;
    use crate::CleanParser;
    use pest::Parser;

    // Parse the expression string as a standalone expression
    let parsed = CleanParser::parse(Rule::expression, expr_str).map_err(|e| {
        CompilerError::parse_error(
            format!(
                "Failed to parse interpolation expression '{}': {}",
                expr_str, e
            ),
            None,
            None,
        )
    })?;

    let expression_pair = parsed.into_iter().next().ok_or_else(|| {
        CompilerError::parse_error(format!("No expression found in '{}'", expr_str), None, None)
    })?;

    parse_expression(expression_pair)
}

pub fn parse_list_literal(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut elements = Vec::new();

    for element in pair.into_inner() {
        if let Rule::expression = element.as_rule() {
            elements.push(parse_expression(element)?);
        }
    }

    // Convert to array values
    let values: Result<Vec<Value>, _> = elements
        .into_iter()
        .map(|expr| match expr {
            Expression::Literal(value) => Ok(value),
            _ => Err(CompilerError::parse_error(
                "List literals can only contain literal values".to_string(),
                None,
                Some("Use variables or function calls outside of list literals".to_string()),
            )),
        })
        .collect();

    Ok(Expression::Literal(Value::List(values?)))
}

pub fn parse_matrix_literal(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut rows = Vec::new();

    for matrix_row_pair in pair.into_inner() {
        if let Rule::matrix_row = matrix_row_pair.as_rule() {
            let mut row = Vec::new();

            for element in matrix_row_pair.into_inner() {
                if let Rule::expression = element.as_rule() {
                    let expr = parse_expression(element)?;
                    match expr {
                        Expression::Literal(Value::Number(f)) => row.push(f),
                        Expression::Literal(Value::Integer(i)) => row.push(i as f64),
                        _ => {
                            return Err(CompilerError::parse_error(
                                "Matrix literals can only contain numeric values".to_string(),
                                None,
                                Some("Use numeric literals in matrix definitions".to_string()),
                            ))
                        }
                    }
                }
            }

            rows.push(row);
        }
    }

    Ok(Expression::Literal(Value::Matrix(rows)))
}

pub fn parse_function_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = convert_to_ast_location(&get_location(&pair));
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();
    let mut arguments = Vec::new();

    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        // argument_expression contains base_expression
                        let inner_expr = arg_expr.into_inner().next().unwrap();
                        arguments.push(parse_base_expression(inner_expr)?);
                    }
                }
            }
            Rule::logical_expression => {
                // Fallback for direct logical expressions (if any)
                arguments.push(parse_logical_expression(arg)?);
            }
            _ => {
                // Skip other rules (like identifier, type_arguments, parentheses)
            }
        }
    }

    // Check if this is a namespace call (contains a dot)
    if let Some(dot_pos) = name.find('.') {
        let namespace = name[..dot_pos].to_string();
        let function = name[dot_pos + 1..].to_string();

        // println!("DEBUG: Parser converting function call '{}' to namespace call: namespace='{}', function='{}'", name, namespace, function);

        Ok(Expression::NamespaceCall {
            namespace,
            function,
            arguments,
            location,
        })
    } else {
        Ok(Expression::Call(name, arguments))
    }
}

pub fn parse_method_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut inner = pair.into_inner();

    // Parse method_call_base
    let base_pair = inner.next().unwrap();
    let object_expr = match base_pair.as_rule() {
        Rule::method_call_base => {
            let mut base_inner = base_pair.into_inner();
            let first = base_inner.next().unwrap();
            match first.as_rule() {
                Rule::identifier => Expression::Variable(first.as_str().to_string()),
                Rule::builtin_class_name => Expression::Variable(first.as_str().to_string()),
                Rule::string => parse_string(first)?,
                // Handle all number variants (because number is a silent rule)
                Rule::decimal_integer
                | Rule::hex_integer
                | Rule::binary_integer
                | Rule::octal_integer
                | Rule::float => parse_number_literal(first)?,
                Rule::boolean => Expression::Literal(Value::Boolean(first.as_str() == "true")),
                Rule::logical_expression => parse_expression(first)?, // Handle parenthesized expressions
                Rule::additive_expression => parse_additive_expression(first)?, // Handle parenthesized additive expressions
                _ => {
                    return Err(CompilerError::parse_error(
                        "Invalid method call base".to_string(),
                        None,
                        None,
                    ))
                }
            }
        }
        _ => {
            return Err(CompilerError::parse_error(
                "Expected method_call_base".to_string(),
                None,
                None,
            ))
        }
    };

    let mut current_expr = object_expr;

    for segment in inner {
        if let Rule::method_call_segment = segment.as_rule() {
            let mut seg_inner = segment.into_inner();
            let first_child = seg_inner.next().unwrap();

            let (method_name, arguments) = match first_child.as_rule() {
                Rule::method_name => {
                    // Method call with mandatory parentheses
                    let method_name = first_child.as_str().to_string();
                    let mut arguments = Vec::new();

                    // Parse arguments from the remaining segments
                    for arg in seg_inner {
                        match arg.as_rule() {
                            Rule::argument_list => {
                                // Parse argument list - contains argument_expression items
                                for arg_expr in arg.into_inner() {
                                    if let Rule::argument_expression = arg_expr.as_rule() {
                                        // argument_expression contains base_expression
                                        let base_expr = arg_expr.into_inner().next().unwrap();
                                        arguments.push(parse_base_expression(base_expr)?);
                                    }
                                }
                            }
                            Rule::logical_expression => {
                                // Fallback for direct logical expressions (if any)
                                arguments.push(parse_logical_expression(arg)?);
                            }
                            _ => {
                                // Skip other rules
                            }
                        }
                    }

                    (method_name, arguments)
                }
                _ => {
                    return Err(CompilerError::parse_error(
                        format!(
                            "Unexpected method call segment: {:?}",
                            first_child.as_rule()
                        ),
                        None,
                        None,
                    ))
                }
            };

            let location = crate::ast::SourceLocation::default();
            current_expr = Expression::MethodCall {
                object: Box::new(current_expr),
                method: method_name,
                arguments,
                location,
            };
        }
    }

    Ok(current_expr)
}

pub fn parse_property_access(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut inner = pair.into_inner();
    let object_name = inner.next().unwrap().as_str().to_string();
    let mut current_expr = Expression::Variable(object_name);

    for segment in inner {
        let property_name = segment.as_str().to_string();
        let location = crate::ast::SourceLocation::default();
        current_expr = Expression::PropertyAccess {
            object: Box::new(current_expr),
            property: property_name,
            location,
        };
    }

    Ok(current_expr)
}

pub fn parse_property_method_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let inner = pair.into_inner();
    let all_children: Vec<_> = inner.collect();

    // First element should be the base identifier
    let base_identifier = all_children[0].as_str().to_string();
    let mut current_expr = Expression::Variable(base_identifier);

    // Build property access chain until we hit the method_name
    let mut segments = Vec::new();
    let mut method_name = String::new();
    let mut arguments = Vec::new();

    for child in &all_children[1..] {
        match child.as_rule() {
            Rule::identifier => segments.push(child.as_str().to_string()),
            Rule::method_name => method_name = child.as_str().to_string(),
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in child.clone().into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        // argument_expression contains base_expression
                        let inner_expr = arg_expr.into_inner().next().unwrap();
                        arguments.push(parse_base_expression(inner_expr)?);
                    }
                }
            }
            Rule::logical_expression => arguments.push(parse_logical_expression(child.clone())?),
            _ => {}
        }
    }

    // Build property access expression for the chain before the method
    for property in segments {
        let location = crate::ast::SourceLocation::default();
        current_expr = Expression::PropertyAccess {
            object: Box::new(current_expr),
            property,
            location,
        };
    }

    let location = crate::ast::SourceLocation::default();
    Ok(Expression::MethodCall {
        object: Box::new(current_expr),
        method: method_name,
        arguments,
        location,
    })
}

pub fn parse_list_access(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut inner = pair.into_inner();

    // First element is the list identifier
    let list_name = inner.next().unwrap().as_str().to_string();
    let list_expr = Expression::Variable(list_name);

    // Second element is the index expression
    let index_pair = inner.next().unwrap();
    let index_expr = parse_expression(index_pair)?;

    Ok(Expression::ListAccess(
        Box::new(list_expr),
        Box::new(index_expr),
    ))
}

pub fn parse_conditional_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = convert_to_ast_location(&get_location(&pair));
    let mut inner = pair.into_inner();

    // Parse: if condition then value else value
    // The grammar gives us: expression, expression, expression (condition, then_expr, else_expr)
    let condition_pair = inner.next().ok_or_else(|| {
        CompilerError::parse_error(
            "Missing condition in conditional expression".to_string(),
            Some(location.clone()),
            Some("Conditional expressions require: if condition then value else value".to_string()),
        )
    })?;

    let then_pair = inner.next().ok_or_else(|| {
        CompilerError::parse_error(
            "Missing then expression in conditional expression".to_string(),
            Some(location.clone()),
            Some("Conditional expressions require: if condition then value else value".to_string()),
        )
    })?;

    let else_pair = inner.next().ok_or_else(|| {
        CompilerError::parse_error(
            "Missing else expression in conditional expression".to_string(),
            Some(location.clone()),
            Some("Conditional expressions require: if condition then value else value".to_string()),
        )
    })?;

    let condition = parse_expression(condition_pair)?;
    let then_expr = parse_expression(then_pair)?;
    let else_expr = parse_expression(else_pair)?;

    Ok(Expression::Conditional {
        condition: Box::new(condition),
        then_expr: Box::new(then_expr),
        else_expr: Box::new(else_expr),
        location,
    })
}

pub fn parse_base_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut arguments = Vec::new();

    for arg in pair.into_inner() {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        // argument_expression contains base_expression
                        let inner_expr = arg_expr.into_inner().next().unwrap();
                        arguments.push(parse_base_expression(inner_expr)?);
                    }
                }
            }
            Rule::logical_expression => {
                arguments.push(parse_logical_expression(arg)?);
            }
            Rule::expression => {
                arguments.push(parse_expression(arg)?);
            }
            _ => {
                // Skip other rules (like "base", parentheses)
            }
        }
    }

    Ok(Expression::BaseCall {
        arguments,
        location: convert_to_ast_location(&location),
    })
}

pub fn parse_static_method_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut inner = pair.into_inner();

    // Parse: ClassName.method(args...) or namespace.subnamespace.method(args...)
    let static_class_name_pair = inner.next().unwrap();
    let class_name = static_class_name_pair.as_str().to_string();
    let method_name = inner.next().unwrap().as_str().to_string();
    let mut arguments = Vec::new();

    // Parse arguments
    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        // argument_expression contains base_expression
                        let inner_expr = arg_expr.into_inner().next().unwrap();
                        arguments.push(parse_base_expression(inner_expr)?);
                    }
                }
            }
            Rule::logical_expression => {
                // Fallback for direct logical expressions (if any)
                arguments.push(parse_logical_expression(arg)?);
            }
            _ => {
                // Skip other rules (like static_class_name, method_name, parentheses)
            }
        }
    }

    Ok(Expression::StaticMethodCall {
        class_name,
        method: method_name,
        arguments,
        location: convert_to_ast_location(&location),
    })
}

pub fn parse_three_level_method_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut inner = pair.into_inner();

    // Parse: namespace.subnamespace.method(args...) like compare.integer.greaterThan(a, b)
    let namespace = inner.next().unwrap().as_str().to_string();
    let subnamespace = inner.next().unwrap().as_str().to_string();
    let method_name = inner.next().unwrap().as_str().to_string();
    let mut arguments = Vec::new();

    // Parse arguments
    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        // argument_expression contains base_expression
                        let inner_expr = arg_expr.into_inner().next().unwrap();
                        arguments.push(parse_base_expression(inner_expr)?);
                    }
                }
            }
            Rule::logical_expression => {
                // Fallback for direct logical expressions (if any)
                arguments.push(parse_logical_expression(arg)?);
            }
            _ => {
                // Skip other rules (like identifiers, method_name, parentheses)
            }
        }
    }

    // Combine namespace and subnamespace into class name like "compare.integer"
    let class_name = format!("{}.{}", namespace, subnamespace);

    Ok(Expression::StaticMethodCall {
        class_name,
        method: method_name,
        arguments,
        location: convert_to_ast_location(&location),
    })
}

pub fn parse_chained_method_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut inner = pair.into_inner();

    // First element is either static_method_call or function_call
    let base_pair = inner.next().unwrap();
    let mut current_expr = match base_pair.as_rule() {
        Rule::static_method_call => parse_static_method_call(base_pair)?,
        Rule::function_call => parse_function_call(base_pair)?,
        _ => {
            return Err(CompilerError::parse_error(
                "Invalid base for chained method call".to_string(),
                None,
                Some("Expected static method call or function call".to_string()),
            ))
        }
    };

    // Now process all the method_call_segment rules
    for segment in inner {
        if let Rule::method_call_segment = segment.as_rule() {
            let mut seg_inner = segment.into_inner();
            let first_child = seg_inner.next().unwrap();

            let (method_name, arguments) = match first_child.as_rule() {
                Rule::method_name => {
                    // Method call with mandatory parentheses
                    let method_name = first_child.as_str().to_string();
                    let mut arguments = Vec::new();

                    // Parse arguments from the remaining segments
                    for arg in seg_inner {
                        match arg.as_rule() {
                            Rule::argument_list => {
                                // Parse argument list - contains argument_expression items
                                for arg_expr in arg.into_inner() {
                                    if let Rule::argument_expression = arg_expr.as_rule() {
                                        // argument_expression contains base_expression
                                        let base_expr = arg_expr.into_inner().next().unwrap();
                                        arguments.push(parse_base_expression(base_expr)?);
                                    }
                                }
                            }
                            Rule::logical_expression => {
                                // Fallback for direct logical expressions (if any)
                                arguments.push(parse_logical_expression(arg)?);
                            }
                            _ => {
                                // Skip other rules
                            }
                        }
                    }

                    (method_name, arguments)
                }
                _ => {
                    return Err(CompilerError::parse_error(
                        format!(
                            "Unexpected chained method call segment: {:?}",
                            first_child.as_rule()
                        ),
                        None,
                        None,
                    ))
                }
            };

            let location = crate::ast::SourceLocation::default();
            current_expr = Expression::MethodCall {
                object: Box::new(current_expr),
                method: method_name,
                arguments,
                location,
            };
        }
    }

    Ok(current_expr)
}

pub fn parse_start_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut inner = pair.into_inner();

    // Parse: start expression
    let expression = parse_expression(inner.next().unwrap())?;

    Ok(Expression::StartExpression {
        expression: Box::new(expression),
        location: convert_to_ast_location(&location),
    })
}
