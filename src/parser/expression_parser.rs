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
        Rule::namespace_method_chain => {
            // Parse namespace method chain
            parse_namespace_method_chain(pair)
        }
        Rule::namespace_function_call => {
            // Parse namespace function call
            parse_namespace_function_call(pair)
        }
        Rule::method_call | Rule::multiple_method_call => {
            // Parse method call or multiple method call directly
            parse_method_call(pair)
        }
        Rule::parenthesized_method_call => {
            // Parse parenthesized expression with method call(s)
            parse_parenthesized_method_call(pair)
        }
        Rule::chained_method_call => {
            // Parse chained method call directly
            parse_chained_method_call(pair)
        }
        Rule::three_level_method_call => {
            // Parse three level method call directly
            parse_three_level_method_call(pair)
        }
        Rule::property_method_call => {
            // Parse property method call directly
            parse_property_method_call(pair)
        }
        Rule::static_method_call => {
            // Parse static method call directly
            parse_static_method_call(pair)
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
        Rule::argument_expression => {
            // Parse argument value directly
            parse_argument_expression(pair)
        }
        // Argument-specific expression rules (supports logical, comparison, arithmetic)
        Rule::argument_logical => {
            parse_argument_logical(pair)
        }
        Rule::argument_comparison => {
            parse_argument_comparison(pair)
        }
        Rule::argument_additive => {
            parse_argument_additive(pair)
        }
        Rule::argument_multiplicative => {
            parse_argument_multiplicative(pair)
        }
        Rule::argument_power => {
            parse_argument_power(pair)
        }
        Rule::argument_unary => {
            parse_argument_unary(pair)
        }
        Rule::argument_primary => {
            parse_argument_primary(pair)
        }
        // argument_term rule removed - now using argument_expression directly
        Rule::list_element => {
            // Parse list element directly
            parse_list_element(pair)
        }
        Rule::single_line_expression => {
            // Parse single line expression directly
            parse_single_line_expression(pair)
        }
        Rule::base_call => {
            // Parse base constructor call directly
            parse_base_call(pair)
        }
        Rule::pairs_literal => {
            // Parse pairs literal directly
            parse_pairs_literal(pair)
        }
        _ => {
            Err(CompilerError::parse_error(
                format!("Unsupported expression rule: {:?}", pair.as_rule()),
                Some(convert_to_ast_location(&get_location(&pair))),
                Some("Expected expression, on_error_expr, on_error_block, base_expression, start_expr, conditional_expr, argument_expression, list_element, or error_variable".to_string())
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
        Rule::pairs_literal => parse_pairs_literal(inner),
        Rule::function_call => parse_function_call(inner),
        Rule::namespace_method_chain => parse_namespace_method_chain(inner),
        Rule::namespace_function_call => parse_namespace_function_call(inner),
        Rule::property_method_call => parse_property_method_call(inner),
        Rule::method_call | Rule::multiple_method_call => parse_method_call(inner),
        Rule::parenthesized_method_call => parse_parenthesized_method_call(inner),
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
        Rule::identifier | Rule::base_identifier => {
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
            // parenthesized_expr contains multiline_logical_expression per grammar
            parse_multiline_logical_expression(inner_expr)
        }
        Rule::conditional_expr => {
            // Handle conditional expressions: if condition then value else value
            parse_conditional_expression(inner)
        }
        Rule::base_call => {
            // Handle base constructor calls: base(args...)
            parse_base_call(inner)
        }
        Rule::constructor_call => {
            // Handle constructor calls: ClassName(args...)
            parse_constructor_call(inner)
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
                        Rule::escaped_char => {
                            // Handle escaped characters like \", \\, \{, \}, \n, \r, \t
                            let escaped_text = inner_part.as_str();
                            let unescaped = match escaped_text {
                                "\\\"" => "\"",
                                "\\\\" => "\\",
                                "\\{" => "{",
                                "\\}" => "}",
                                "\\n" => "\n",
                                "\\r" => "\r",
                                "\\t" => "\t",
                                _ => escaped_text, // fallback to original if not recognized
                            };
                            parts.push(StringPart::Text(unescaped.to_string()));
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
    use super::CleanParser;
    use crate::parser::grammar::Rule;
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
        if let Rule::list_element = element.as_rule() {
            elements.push(parse_list_element(element)?);
        }
    }

    // Convert to array values
    let values: Result<Vec<Value>, _> = elements
        .into_iter()
        .map(|expr| match expr {
            Expression::Literal(value) => Ok(value),
            Expression::Variable(name) => {
                // Allow variables in list literals for now
                // TODO: Evaluate variables during compilation
                Err(CompilerError::parse_error(
                    format!("Variable '{}' cannot be used in list literal", name),
                    None,
                    Some("List literals must contain constant values only".to_string()),
                ))
            }
            _ => Err(CompilerError::parse_error(
                "List literals can only contain literal values".to_string(),
                None,
                Some("Use variables or function calls outside of list literals".to_string()),
            )),
        })
        .collect();

    Ok(Expression::Literal(Value::List(values?)))
}

pub fn parse_list_element(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let inner = pair.into_inner().next().ok_or_else(|| {
        CompilerError::parse_error(
            "Empty list element".to_string(),
            Some(convert_to_ast_location(&location)),
            Some("Expected a value in the list element".to_string()),
        )
    })?;

    match inner.as_rule() {
        Rule::decimal_integer => parse_integer_literal(inner, &location),
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
        Rule::identifier | Rule::base_identifier => {
            let identifier = inner.as_str();
            Ok(Expression::Variable(identifier.to_string()))
        }
        Rule::expression => {
            // Parenthesized expression: (expression)
            parse_expression(inner)
        }
        Rule::pairs_literal => {
            // Delegate to main expression parser for pairs literal
            parse_expression(inner)
        }
        _ => Err(CompilerError::parse_error(
            format!("Unexpected list element: {:?}", inner.as_rule()),
            Some(convert_to_ast_location(&location)),
            Some(
                "Expected number, boolean, string, identifier, pairs literal, or parenthesized expression"
                    .to_string(),
            ),
        )),
    }
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

pub fn parse_pairs_literal(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut pairs = Vec::new();

    for pair_element in pair.into_inner() {
        if let Rule::pair_element = pair_element.as_rule() {
            let mut pair_parts = pair_element.into_inner();

            // Parse the key (string, identifier, or decimal_integer)
            let key_part = pair_parts.next().unwrap();
            let key_value = match key_part.as_rule() {
                Rule::string => {
                    // Parse string and extract the inner value
                    match parse_string(key_part)? {
                        Expression::Literal(Value::String(s)) => Value::String(s),
                        _ => {
                            return Err(CompilerError::parse_error(
                                "Invalid string key in pairs literal".to_string(),
                                None,
                                None,
                            ))
                        }
                    }
                }
                Rule::identifier => Value::String(key_part.as_str().to_string()),
                Rule::decimal_integer => {
                    let num_str = key_part.as_str();
                    let num = num_str.parse::<i64>().map_err(|_| {
                        CompilerError::parse_error(
                            format!("Invalid integer key: {num_str}"),
                            None,
                            None,
                        )
                    })?;
                    Value::Integer(num)
                }
                _ => {
                    return Err(CompilerError::parse_error(
                        format!(
                            "Unexpected key type in pairs literal: {:?}",
                            key_part.as_rule()
                        ),
                        None,
                        None,
                    ))
                }
            };

            // Parse the value (single_line_expression)
            let value_part = pair_parts.next().unwrap();
            let value_expr = parse_expression(value_part)?;
            let value_value = match value_expr {
                Expression::Literal(v) => v,
                _ => {
                    // For non-literal expressions, we'll need to handle them differently
                    // For now, convert to a string representation
                    Value::String(format!("{:?}", value_expr))
                }
            };

            // Create a 2-element list representing the key-value pair
            let pair_list = Value::List(vec![key_value, value_value]);
            pairs.push(pair_list);
        }
    }

    // Return the pairs as a list of 2-element lists
    Ok(Expression::Literal(Value::List(pairs)))
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
                        // Parse argument_expression directly
                        arguments.push(parse_argument_expression(arg_expr)?);
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

pub fn parse_namespace_function_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = convert_to_ast_location(&get_location(&pair));
    let mut inner = pair.into_inner();

    // First child is namespace_identifier
    let namespace = inner.next().unwrap().as_str().to_string();

    // Second child is the function identifier
    let function = inner.next().unwrap().as_str().to_string();

    let mut arguments = Vec::new();

    // Parse remaining arguments
    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                for arg_expr in arg.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        arguments.push(parse_argument_expression(arg_expr)?);
                    }
                }
            }
            _ => {
                // Skip other rules (like parentheses)
            }
        }
    }

    Ok(Expression::NamespaceCall {
        namespace,
        function,
        arguments,
        location,
    })
}

pub fn parse_namespace_method_chain(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = convert_to_ast_location(&get_location(&pair));
    let mut inner = pair.into_inner();

    // First child is namespace_identifier
    let namespace = inner.next().unwrap().as_str().to_string();

    // Second child is the function identifier
    let function = inner.next().unwrap().as_str().to_string();

    let mut arguments = Vec::new();

    // Collect all children to distinguish arguments from method_call_segments
    let remaining: Vec<_> = inner.collect();
    let mut i = 0;

    // Parse arguments (before method_call_segment)
    while i < remaining.len() {
        match remaining[i].as_rule() {
            Rule::argument_list => {
                for arg_expr in remaining[i].clone().into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        arguments.push(parse_argument_expression(arg_expr)?);
                    }
                }
                i += 1;
            }
            Rule::method_call_segment => {
                break; // Stop when we hit method call segments
            }
            _ => {
                i += 1; // Skip other rules (like parentheses)
            }
        }
    }

    // Start with the namespace call as base expression
    let mut current_expr = Expression::NamespaceCall {
        namespace,
        function,
        arguments,
        location: location.clone(),
    };

    // Now process all the method_call_segment rules
    while i < remaining.len() {
        if let Rule::method_call_segment = remaining[i].as_rule() {
            let segment = remaining[i].clone();
            let mut seg_inner = segment.into_inner();
            let first_child = seg_inner.next().unwrap();

            let (method_name, method_arguments) = match first_child.as_rule() {
                Rule::method_name => {
                    let method_name = first_child.as_str().to_string();
                    let mut method_arguments = Vec::new();

                    // Parse arguments from the remaining segments
                    for arg in seg_inner {
                        match arg.as_rule() {
                            Rule::argument_list => {
                                for arg_expr in arg.into_inner() {
                                    if let Rule::argument_expression = arg_expr.as_rule() {
                                        method_arguments.push(parse_argument_expression(arg_expr)?);
                                    }
                                }
                            }
                            Rule::logical_expression => {
                                method_arguments.push(parse_logical_expression(arg)?);
                            }
                            _ => {}
                        }
                    }

                    (method_name, method_arguments)
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

            current_expr = Expression::MethodCall {
                object: Box::new(current_expr),
                method: method_name,
                arguments: method_arguments,
                location: location.clone(),
            };
        }
        i += 1;
    }

    Ok(current_expr)
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
                Rule::identifier | Rule::base_identifier => {
                    Expression::Variable(first.as_str().to_string())
                }
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
                                        // Parse argument_expression directly
                                        arguments.push(parse_argument_expression(arg_expr)?);
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

pub fn parse_parenthesized_method_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut inner = pair.into_inner();

    // First should be the parenthesized_expr
    let paren_expr_pair = inner.next().unwrap();
    // parenthesized_expr contains multiline_logical_expression, so extract it
    let expr_inside = paren_expr_pair.into_inner().next().unwrap();
    let mut current_expr = match expr_inside.as_rule() {
        Rule::multiline_logical_expression => parse_multiline_logical_expression(expr_inside)?,
        _ => parse_expression(expr_inside)?,
    };

    // Then comes one or more method_call_segment
    for segment in inner {
        if segment.as_rule() != Rule::method_call_segment {
            continue;
        }

        let mut segment_inner = segment.into_inner();
        let first_child = segment_inner.next().unwrap();

        let method_name = match first_child.as_rule() {
            Rule::method_name => first_child.as_str().to_string(),
            Rule::identifier => first_child.as_str().to_string(),
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Expected method name, got {:?}", first_child.as_rule()),
                    None,
                    None,
                ))
            }
        };

        let mut arguments = Vec::new();
        if let Some(arg_list) = segment_inner.next() {
            if arg_list.as_rule() == Rule::argument_list {
                for arg_expr in arg_list.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        arguments.push(parse_argument_expression(arg_expr)?);
                    }
                }
            }
        }

        let location = crate::ast::SourceLocation::default();
        current_expr = Expression::MethodCall {
            object: Box::new(current_expr),
            method: method_name,
            arguments,
            location,
        };
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
                        // Parse argument_expression directly
                        arguments.push(parse_argument_expression(arg_expr)?);
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
    let mut current_expr = Expression::Variable(list_name);

    // Grammar allows multiple chained [index] accesses: identifier ~ ("[" ~ additive_expression ~ "]")+
    // Parse all index expressions and chain them
    for index_pair in inner {
        let index_expr = parse_expression(index_pair)?;
        current_expr = Expression::ListAccess(Box::new(current_expr), Box::new(index_expr));
    }

    Ok(current_expr)
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
    eprintln!("DEBUG: parse_base_call called");
    let location = get_location(&pair);
    let mut arguments = Vec::new();

    for arg in pair.into_inner() {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        // Parse argument_expression directly
                        arguments.push(parse_argument_expression(arg_expr)?);
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

    eprintln!(
        "DEBUG: parse_base_call returning BaseCall with {} arguments",
        arguments.len()
    );
    Ok(Expression::BaseCall {
        arguments,
        location: convert_to_ast_location(&location),
    })
}

pub fn parse_constructor_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut class_name = String::new();
    let mut arguments = Vec::new();

    // Parse the class name and argument list
    for arg in pair.into_inner() {
        match arg.as_rule() {
            Rule::class_name => {
                class_name = arg.as_str().to_string();
            }
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        // Parse argument_expression directly
                        arguments.push(parse_argument_expression(arg_expr)?);
                    }
                }
            }
            _ => {
                // Skip other rules (like parentheses)
            }
        }
    }

    if class_name.is_empty() {
        return Err(CompilerError::parse_error(
            "Constructor call missing class name".to_string(),
            Some(convert_to_ast_location(&location)),
            Some("Constructor calls must have a class name".to_string()),
        ));
    }

    Ok(Expression::ObjectCreation {
        class_name,
        arguments,
        location: convert_to_ast_location(&location),
    })
}

pub fn parse_static_method_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut inner = pair.into_inner();

    // Parse: ClassName.method(args...) or namespace.subnamespace.method(args...)
    let static_class_name_pair = inner.next().unwrap();
    let full_class_name = static_class_name_pair.as_str().to_string();
    let method_name = inner.next().unwrap().as_str().to_string();
    let mut arguments = Vec::new();

    // Parse arguments
    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if let Rule::argument_expression = arg_expr.as_rule() {
                        // Parse argument_expression directly
                        arguments.push(parse_argument_expression(arg_expr)?);
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

    // Split the full_class_name to handle multi-level namespaces
    // e.g., "compare.integer" -> namespace=["compare"], class_name="integer"
    //       "Math" -> namespace=[], class_name="Math"
    let parts: Vec<&str> = full_class_name.split('.').collect();
    let (namespace, class_name) = if parts.len() > 1 {
        // Multi-level: all but last part is namespace, last part is class
        let namespace_parts: Vec<String> = parts[..parts.len() - 1]
            .iter()
            .map(|s| s.to_string())
            .collect();
        (namespace_parts, parts[parts.len() - 1].to_string())
    } else {
        // Single level: no namespace, just class name
        (vec![], full_class_name)
    };

    Ok(Expression::StaticMethodCall {
        namespace,
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
                        // Parse argument_expression directly
                        arguments.push(parse_argument_expression(arg_expr)?);
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

    // Keep namespace separate from class name for proper type resolution
    Ok(Expression::StaticMethodCall {
        namespace: vec![namespace], // Namespace hierarchy
        class_name: subnamespace,   // Actual class name
        method: method_name,
        arguments,
        location: convert_to_ast_location(&location),
    })
}

pub fn parse_chained_method_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut inner = pair.into_inner();

    // First element can be static_method_call, function_call, property_access, or identifier
    let base_pair = inner.next().unwrap();
    let mut current_expr =
        match base_pair.as_rule() {
            Rule::static_method_call => parse_static_method_call(base_pair)?,
            Rule::function_call => parse_function_call(base_pair)?,
            Rule::property_access => parse_property_access(base_pair)?,
            Rule::identifier | Rule::base_identifier => {
                // Identifier base - treat as variable reference or namespace
                Expression::Variable(base_pair.as_str().to_string())
            }
            _ => return Err(CompilerError::parse_error(
                format!(
                    "Invalid base for chained method call: {:?}",
                    base_pair.as_rule()
                ),
                None,
                Some(
                    "Expected static method call, function call, property access, or identifier"
                        .to_string(),
                ),
            )),
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
                                        // Parse argument_expression directly
                                        arguments.push(parse_argument_expression(arg_expr)?);
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

pub fn parse_argument_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    // argument_expression now contains argument_logical (supports full logical expressions)
    let location = get_location(&pair);
    let inner = pair.into_inner().next().ok_or_else(|| {
        CompilerError::parse_error(
            "Empty argument expression".to_string(),
            Some(convert_to_ast_location(&location)),
            Some("Argument expression should contain an argument_logical".to_string()),
        )
    })?;

    match inner.as_rule() {
        Rule::argument_logical => parse_argument_logical(inner),
        _ => {
            let location = get_location(&inner);
            Err(CompilerError::parse_error(
                format!(
                    "Unexpected rule in argument expression: {:?}",
                    inner.as_rule()
                ),
                Some(convert_to_ast_location(&location)),
                Some("Expected argument_logical".to_string()),
            ))
        }
    }
}

// Argument-specific expression parsing functions (supports logical, comparison, arithmetic)
pub fn parse_argument_logical(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut pairs = pair.into_inner();
    let first = pairs.next().unwrap();

    let mut left = parse_argument_comparison(first)?;

    while let (Some(op_pair), Some(right_pair)) = (pairs.next(), pairs.next()) {
        let op = op_pair.as_str();
        let right = parse_argument_comparison(right_pair)?;

        let binary_op = match op {
            "&&" | "and" => BinaryOperator::And,
            "||" | "or" => BinaryOperator::Or,
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unknown logical operator: {}", op),
                    Some(convert_to_ast_location(&location)),
                    Some("Expected && or ||".to_string()),
                ))
            }
        };
        left = Expression::Binary(Box::new(left), binary_op, Box::new(right));
    }

    Ok(left)
}

pub fn parse_argument_comparison(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut pairs = pair.into_inner();
    let first = pairs.next().unwrap();

    let mut left = parse_argument_unary(first)?;

    while let (Some(op_pair), Some(right_pair)) = (pairs.next(), pairs.next()) {
        let op = op_pair.as_str();
        let right = parse_argument_unary(right_pair)?;

        let binary_op = match op {
            "==" => BinaryOperator::Equal,
            "!=" => BinaryOperator::NotEqual,
            "<" => BinaryOperator::Less,
            "<=" => BinaryOperator::LessEqual,
            ">" => BinaryOperator::Greater,
            ">=" => BinaryOperator::GreaterEqual,
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unknown comparison operator: {}", op),
                    Some(convert_to_ast_location(&location)),
                    Some("Expected ==, !=, <, <=, >, or >=".to_string()),
                ))
            }
        };
        left = Expression::Binary(Box::new(left), binary_op, Box::new(right));
    }

    Ok(left)
}

pub fn parse_argument_additive(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut pairs = pair.into_inner();
    let first = pairs.next().unwrap();

    let mut left = parse_argument_multiplicative(first)?;

    while let (Some(op_pair), Some(right_pair)) = (pairs.next(), pairs.next()) {
        let op = op_pair.as_str();
        let right = parse_argument_multiplicative(right_pair)?;

        let binary_op = match op {
            "+" => BinaryOperator::Add,
            "-" => BinaryOperator::Subtract,
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unknown additive operator: {}", op),
                    Some(convert_to_ast_location(&location)),
                    Some("Expected + or -".to_string()),
                ))
            }
        };
        left = Expression::Binary(Box::new(left), binary_op, Box::new(right));
    }

    Ok(left)
}

pub fn parse_argument_multiplicative(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut pairs = pair.into_inner();
    let first = pairs.next().unwrap();

    let mut left = parse_argument_power(first)?;

    while let (Some(op_pair), Some(right_pair)) = (pairs.next(), pairs.next()) {
        let op = op_pair.as_str();
        let right = parse_argument_power(right_pair)?;

        let binary_op = match op {
            "*" => BinaryOperator::Multiply,
            "/" => BinaryOperator::Divide,
            "%" => BinaryOperator::Modulo,
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unknown multiplicative operator: {}", op),
                    Some(convert_to_ast_location(&location)),
                    Some("Expected *, /, or %".to_string()),
                ))
            }
        };
        left = Expression::Binary(Box::new(left), binary_op, Box::new(right));
    }

    Ok(left)
}

pub fn parse_argument_power(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut pairs = pair.into_inner();
    let first = pairs.next().unwrap();

    let mut left = parse_argument_primary(first)?;

    while let (Some(op_pair), Some(right_pair)) = (pairs.next(), pairs.next()) {
        let op = op_pair.as_str();
        let right = parse_argument_primary(right_pair)?;

        let binary_op = match op {
            "^" => BinaryOperator::Power,
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unknown power operator: {}", op),
                    Some(convert_to_ast_location(&location)),
                    Some("Expected ^".to_string()),
                ))
            }
        };
        left = Expression::Binary(Box::new(left), binary_op, Box::new(right));
    }

    Ok(left)
}

pub fn parse_argument_unary(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let inner = pair.into_inner();

    // Collect unary operators
    let mut operators = Vec::new();
    let mut additive_pair = None;

    for child in inner {
        match child.as_rule() {
            Rule::unary_op => operators.push(child.as_str().to_string()),
            Rule::argument_additive => {
                additive_pair = Some(child);
                break;
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unexpected rule in argument unary: {:?}", child.as_rule()),
                    Some(convert_to_ast_location(&location)),
                    Some("Expected unary_op or argument_additive".to_string()),
                ));
            }
        }
    }

    let additive_pair = additive_pair.ok_or_else(|| {
        CompilerError::parse_error(
            "Missing additive expression in argument unary expression".to_string(),
            Some(convert_to_ast_location(&location)),
            Some("Argument unary should contain argument_additive".to_string()),
        )
    })?;

    let mut expr = parse_argument_additive(additive_pair)?;

    // Apply unary operators from right to left
    for op in operators.into_iter().rev() {
        let unary_op = match op.as_str() {
            "-" => UnaryOperator::Negate,
            "!" | "not" => UnaryOperator::Not,
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unknown unary operator: {}", op),
                    Some(convert_to_ast_location(&location)),
                    Some("Expected -, !, or not".to_string()),
                ))
            }
        };
        expr = Expression::Unary(unary_op, Box::new(expr));
    }

    Ok(expr)
}

pub fn parse_argument_primary(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    // argument_primary just wraps primary
    let location = get_location(&pair);
    let inner = pair.into_inner().next().ok_or_else(|| {
        CompilerError::parse_error(
            "Empty argument_primary".to_string(),
            Some(convert_to_ast_location(&location)),
            Some("Expected primary expression".to_string()),
        )
    })?;

    parse_primary(inner)
}

/// Parse single-line expressions (no newlines/indentation allowed)
pub fn parse_single_line_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let inner = pair.into_inner().next().ok_or_else(|| {
        CompilerError::parse_error(
            "Empty single_line_expression".to_string(),
            Some(convert_to_ast_location(&location)),
            None,
        )
    })?;

    match inner.as_rule() {
        Rule::single_line_logical_expression => parse_single_line_logical_expression(inner),
        _ => Err(CompilerError::parse_error(
            format!(
                "Unexpected rule in single_line_expression: {:?}",
                inner.as_rule()
            ),
            Some(convert_to_ast_location(&get_location(&inner))),
            None,
        )),
    }
}

/// Parse single-line logical expressions (logical operations without newlines)
pub fn parse_single_line_logical_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::single_line_comparison_expression => {
                expr_stack.push(parse_single_line_comparison_expression(item)?);
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
                        "Unexpected element in logical expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    None,
                ))
            }
        }
    }

    // Build the expression tree from left to right
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty logical expression".to_string(),
            None,
            None,
        ));
    }

    let mut result = expr_stack.remove(0);
    for (i, op) in op_stack.into_iter().enumerate() {
        if i < expr_stack.len() {
            let right = expr_stack.remove(0);
            result = Expression::Binary(Box::new(result), op, Box::new(right));
        }
    }

    Ok(result)
}

/// Parse single-line comparison expressions (comparisons without newlines)
pub fn parse_single_line_comparison_expression(
    pair: Pair<Rule>,
) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::single_line_unary_expression => {
                expr_stack.push(parse_single_line_unary_expression(item)?);
            }
            Rule::comparison_op => {
                let op = match item.as_str() {
                    "=" => BinaryOperator::Equal,
                    "!=" => BinaryOperator::NotEqual,
                    "<" => BinaryOperator::Less,
                    ">" => BinaryOperator::Greater,
                    "<=" => BinaryOperator::LessEqual,
                    ">=" => BinaryOperator::GreaterEqual,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid comparison operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some("Valid comparison operators are: =, !=, <, >, <=, >=".to_string()),
                        ))
                    }
                };
                op_stack.push(op);
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!(
                        "Unexpected element in comparison expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    None,
                ))
            }
        }
    }

    // Build the expression tree from left to right
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty comparison expression".to_string(),
            None,
            None,
        ));
    }

    let mut result = expr_stack.remove(0);
    for (i, op) in op_stack.into_iter().enumerate() {
        if i < expr_stack.len() {
            let right = expr_stack.remove(0);
            result = Expression::Binary(Box::new(result), op, Box::new(right));
        }
    }

    Ok(result)
}

/// Parse single-line unary expressions (unary operations without newlines)
pub fn parse_single_line_unary_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut inner_iter = pair.into_inner();
    let inner = inner_iter.next().ok_or_else(|| {
        CompilerError::parse_error(
            "Empty single_line_unary_expression".to_string(),
            Some(convert_to_ast_location(&location)),
            None,
        )
    })?;

    match inner.as_rule() {
        Rule::single_line_additive_expression => parse_single_line_additive_expression(inner),
        Rule::unary_op => {
            let op = match inner.as_str() {
                "not" => UnaryOperator::Not,
                "-" => UnaryOperator::Negate,
                _ => {
                    return Err(CompilerError::parse_error(
                        format!("Invalid unary operator: {}", inner.as_str()),
                        Some(convert_to_ast_location(&get_location(&inner))),
                        Some("Valid unary operators are: not, -".to_string()),
                    ))
                }
            };

            // Get the next item which should be the expression
            let expr_inner = inner_iter.next().ok_or_else(|| {
                CompilerError::parse_error(
                    "Missing expression after unary operator".to_string(),
                    Some(convert_to_ast_location(&location)),
                    None,
                )
            })?;

            let operand = parse_single_line_unary_expression(expr_inner)?;
            Ok(Expression::Unary(op, Box::new(operand)))
        }
        _ => Err(CompilerError::parse_error(
            format!(
                "Unexpected rule in single_line_unary_expression: {:?}",
                inner.as_rule()
            ),
            Some(convert_to_ast_location(&get_location(&inner))),
            None,
        )),
    }
}

/// Parse single-line additive expressions (addition/subtraction without newlines)
pub fn parse_single_line_additive_expression(
    pair: Pair<Rule>,
) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::single_line_multiplicative_expression => {
                expr_stack.push(parse_single_line_multiplicative_expression(item)?);
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
                        "Unexpected element in additive expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    None,
                ))
            }
        }
    }

    // Build the expression tree from left to right
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty additive expression".to_string(),
            None,
            None,
        ));
    }

    let mut result = expr_stack.remove(0);
    for (i, op) in op_stack.into_iter().enumerate() {
        if i < expr_stack.len() {
            let right = expr_stack.remove(0);
            result = Expression::Binary(Box::new(result), op, Box::new(right));
        }
    }

    Ok(result)
}

/// Parse single-line multiplicative expressions (multiplication/division without newlines)
pub fn parse_single_line_multiplicative_expression(
    pair: Pair<Rule>,
) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::single_line_power_expression => {
                expr_stack.push(parse_single_line_power_expression(item)?);
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
                        "Unexpected element in multiplicative expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    None,
                ))
            }
        }
    }

    // Build the expression tree from left to right
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty multiplicative expression".to_string(),
            None,
            None,
        ));
    }

    let mut result = expr_stack.remove(0);
    for (i, op) in op_stack.into_iter().enumerate() {
        if i < expr_stack.len() {
            let right = expr_stack.remove(0);
            result = Expression::Binary(Box::new(result), op, Box::new(right));
        }
    }

    Ok(result)
}

/// Parse single-line power expressions (exponentiation without newlines)
pub fn parse_single_line_power_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::single_line_primary => {
                expr_stack.push(parse_single_line_primary(item)?);
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
                    format!(
                        "Unexpected element in power expression: {:?}",
                        item.as_rule()
                    ),
                    Some(convert_to_ast_location(&get_location(&item))),
                    None,
                ))
            }
        }
    }

    // Build the expression tree from left to right
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty power expression".to_string(),
            None,
            None,
        ));
    }

    let mut result = expr_stack.remove(0);
    for (i, op) in op_stack.into_iter().enumerate() {
        if i < expr_stack.len() {
            let right = expr_stack.remove(0);
            result = Expression::Binary(Box::new(result), op, Box::new(right));
        }
    }

    Ok(result)
}

/// Parse single-line primary expressions (base values and parenthesized expressions without newlines)
pub fn parse_single_line_primary(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    // For single-line primary expressions, delegate to existing primary expression parser
    // since primary expressions (literals, identifiers, etc.) are inherently single-line
    parse_primary(pair)
}

// parse_argument_term function removed - now using single_line_expression in argument_expression

/// Parse multiline logical expression (supports expressions with whitespace/newlines)
pub fn parse_multiline_logical_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::multiline_comparison_expression => {
                expr_stack.push(parse_multiline_comparison_expression(item)?);
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
            _ => {} // Skip whitespace/newlines
        }
    }

    // Build the expression tree (left-associative)
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty multiline logical expression".to_string(),
            None,
            Some(
                "Multiline logical expression must contain at least one comparison expression"
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

/// Parse multiline comparison expression
pub fn parse_multiline_comparison_expression(
    pair: Pair<Rule>,
) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::multiline_unary_expression => {
                expr_stack.push(parse_multiline_unary_expression(item)?);
            }
            Rule::comparison_op => {
                let op = match item.as_str() {
                    "==" => BinaryOperator::Equal,
                    "!=" => BinaryOperator::NotEqual,
                    "<" => BinaryOperator::Less,
                    ">" => BinaryOperator::Greater,
                    "<=" => BinaryOperator::LessEqual,
                    ">=" => BinaryOperator::GreaterEqual,
                    s if s.starts_with("is") => BinaryOperator::Is,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid comparison operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some(
                                "Valid comparison operators are: ==, !=, <, >, <=, >=, is"
                                    .to_string(),
                            ),
                        ))
                    }
                };
                op_stack.push(op);
            }
            _ => {} // Skip whitespace/newlines
        }
    }

    // Build the expression tree (left-associative)
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty multiline comparison expression".to_string(),
            None,
            Some(
                "Multiline comparison expression must contain at least one unary expression"
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

/// Parse multiline unary expression
pub fn parse_multiline_unary_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut unary_ops = Vec::new();
    let mut additive_expr = None;

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::unary_op => {
                unary_ops.push(match item.as_str() {
                    "not" => UnaryOperator::Not,
                    "-" => UnaryOperator::Negate,
                    _ => {
                        return Err(CompilerError::parse_error(
                            format!("Invalid unary operator: {}", item.as_str()),
                            Some(convert_to_ast_location(&get_location(&item))),
                            Some("Valid unary operators are: not, -".to_string()),
                        ))
                    }
                });
            }
            Rule::multiline_additive_expression => {
                additive_expr = Some(parse_multiline_additive_expression(item)?);
                break;
            }
            _ => {} // Skip whitespace/newlines
        }
    }

    let mut result = additive_expr.ok_or_else(|| {
        CompilerError::parse_error(
            "Missing additive expression in multiline unary expression".to_string(),
            None,
            Some("Multiline unary expression must contain an additive expression".to_string()),
        )
    })?;

    // Apply unary operators from right to left (since we parsed left to right)
    for op in unary_ops.into_iter().rev() {
        result = Expression::Unary(op, Box::new(result));
    }

    Ok(result)
}

/// Parse multiline additive expression
pub fn parse_multiline_additive_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::multiline_multiplicative_expression => {
                expr_stack.push(parse_multiline_multiplicative_expression(item)?);
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
            _ => {} // Skip whitespace/newlines
        }
    }

    // Build the expression tree (left-associative)
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty multiline additive expression".to_string(),
            None,
            Some(
                "Multiline additive expression must contain at least one multiplicative expression"
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

/// Parse multiline multiplicative expression
pub fn parse_multiline_multiplicative_expression(
    pair: Pair<Rule>,
) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::multiline_power_expression => {
                expr_stack.push(parse_multiline_power_expression(item)?);
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
            _ => {} // Skip whitespace/newlines
        }
    }

    // Build the expression tree (left-associative)
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty multiline multiplicative expression".to_string(),
            None,
            Some(
                "Multiline multiplicative expression must contain at least one power expression"
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

/// Parse multiline power expression
pub fn parse_multiline_power_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let mut expr_stack = Vec::new();
    let mut op_stack = Vec::new();

    for item in pair.into_inner() {
        match item.as_rule() {
            Rule::multiline_primary => {
                expr_stack.push(parse_multiline_primary(item)?);
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
            _ => {} // Skip whitespace/newlines
        }
    }

    // Build the expression tree (right-associative for power)
    if expr_stack.is_empty() {
        return Err(CompilerError::parse_error(
            "Empty multiline power expression".to_string(),
            None,
            Some(
                "Multiline power expression must contain at least one primary expression"
                    .to_string(),
            ),
        ));
    }

    let mut result = expr_stack.pop().unwrap();

    while let Some(op) = op_stack.pop() {
        if let Some(left) = expr_stack.pop() {
            result = Expression::Binary(Box::new(left), op, Box::new(result));
        }
    }

    Ok(result)
}

/// Parse multiline primary expression
pub fn parse_multiline_primary(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    // multiline_primary just wraps a primary expression
    let inner = pair.into_inner().next().ok_or_else(|| {
        CompilerError::parse_error(
            "Empty multiline primary expression".to_string(),
            None,
            Some("Multiline primary expression must contain a primary expression".to_string()),
        )
    })?;
    parse_primary(inner)
}
