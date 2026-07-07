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
            let inner = pair.into_inner().next().expect("invariant: expression grammar child");
            parse_expression(inner)
        }
        Rule::on_error_expr => {
            // Handle onError expression
            let location = convert_to_ast_location(&get_location(&pair));
            let mut inner = pair.into_inner();
            let expression = parse_expression(inner.next().expect("invariant: on_error_expr first child"))?;
            let fallback = parse_expression(inner.next().expect("invariant: on_error_expr second child"))?;

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
            let expression = parse_expression(inner.next().expect("invariant: on_error_block expression child"))?;

            // Parse the indented block
            let block_pair = inner.next().expect("invariant: on_error_block block child");
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
        Rule::multiline_default_expression => {
            parse_multiline_default_expression(pair)
        }
        Rule::single_line_default_expression => {
            parse_single_line_default_expression(pair)
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
            let inner = pair.into_inner().next().expect("invariant: start_expr grammar child");
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
            let inner = pair.into_inner().next().expect("invariant: parenthesized_expr grammar child");
            parse_expression(inner)
        }
        Rule::argument_expression => {
            // Parse argument value directly
            parse_argument_expression(pair)
        }
        Rule::argument_item => {
            // Parse argument item (named or positional)
            parse_argument_item(pair)
        }
        Rule::named_argument => {
            // Dispatch named_argument when encountered as a top-level expression rule
            parse_argument_item(pair)
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
                    Some("Expected logical expression, or conditional expression".to_string()),
                ))
            }
        }
    }

    Err(CompilerError::parse_error(
        "Empty base expression".to_string(),
        None,
        Some(
            "Base expression must contain a default, logical, or conditional expression"
                .to_string(),
        ),
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
    let op_count = op_stack.len();
    let mut i = 0;

    // BUG (fp 7793fbeec120): the old condition was
    //   while i < op_stack.len() && i < expr_stack.len()
    // which is wrong because `expr_stack.remove(0)` inside the loop shrinks
    // `expr_stack.len()`. After the initial `remove(0)`, the invariant is
    // `expr_stack.len() = op_stack.len() - i`, so the condition failed one
    // iteration early — dropping the last operand of chains with 4+ operands
    // (e.g. `a + b + c + d`). The fix iterates against the stable op_count
    // captured before the loop.
    while i < op_count && !expr_stack.is_empty() {
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
    let op_count = op_stack.len();
    let mut i = 0;

    // BUG (fp 7793fbeec120): the old condition was
    //   while i < op_stack.len() && i < expr_stack.len()
    // which is wrong because `expr_stack.remove(0)` inside the loop shrinks
    // `expr_stack.len()`. After the initial `remove(0)`, the invariant is
    // `expr_stack.len() = op_stack.len() - i`, so the condition failed one
    // iteration early — dropping the last operand of chains with 4+ operands
    // (e.g. `a + b + c + d`). The fix iterates against the stable op_count
    // captured before the loop.
    while i < op_count && !expr_stack.is_empty() {
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
    let op_count = op_stack.len();
    let mut i = 0;

    // BUG (fp 7793fbeec120): the old condition was
    //   while i < op_stack.len() && i < expr_stack.len()
    // which is wrong because `expr_stack.remove(0)` inside the loop shrinks
    // `expr_stack.len()`. After the initial `remove(0)`, the invariant is
    // `expr_stack.len() = op_stack.len() - i`, so the condition failed one
    // iteration early — dropping the last operand of chains with 4+ operands
    // (e.g. `a + b + c + d`). The fix iterates against the stable op_count
    // captured before the loop.
    while i < op_count && !expr_stack.is_empty() {
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
    let op_count = op_stack.len();
    let mut i = 0;

    // BUG (fp 7793fbeec120): the old condition was
    //   while i < op_stack.len() && i < expr_stack.len()
    // which is wrong because `expr_stack.remove(0)` inside the loop shrinks
    // `expr_stack.len()`. After the initial `remove(0)`, the invariant is
    // `expr_stack.len() = op_stack.len() - i`, so the condition failed one
    // iteration early — dropping the last operand of chains with 4+ operands
    // (e.g. `a + b + c + d`). The fix iterates against the stable op_count
    // captured before the loop.
    while i < op_count && !expr_stack.is_empty() {
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
    let mut result = expr_stack
        .pop()
        .expect("invariant: non-empty stack checked above");

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
            let integer_inner = inner
                .into_inner()
                .next()
                .expect("invariant: integer grammar child");
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
        // BOOK: null-support - Parse null literal
        Rule::none_literal => Ok(Expression::Literal(Value::None)),
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
        Rule::identifier | Rule::base_identifier | Rule::soft_keyword_identifier => {
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
            let inner_expr = inner
                .into_inner()
                .next()
                .expect("invariant: parenthesized_expr inner grammar child");
            // BOOK: null-coalescing - parenthesized_expr now contains multiline_default_expression
            match inner_expr.as_rule() {
                Rule::multiline_default_expression => {
                    parse_multiline_default_expression(inner_expr)
                }
                Rule::multiline_logical_expression => {
                    parse_multiline_logical_expression(inner_expr)
                }
                _ => parse_multiline_logical_expression(inner_expr), // fallback for compatibility
            }
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
                            let expr_str = inner
                                .next()
                                .expect("invariant: string_interpolation grammar child")
                                .as_str();

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
                let expr_str = inner
                    .next()
                    .expect("invariant: string_interpolation grammar child")
                    .as_str();

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
                // Variables in list literals require runtime evaluation
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
        // BOOK: null-support - Parse null literal
        Rule::none_literal => Ok(Expression::Literal(Value::None)),
        Rule::string => parse_string(inner),
        Rule::identifier | Rule::base_identifier | Rule::soft_keyword_identifier => {
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
    let mut rows: Vec<Vec<Value>> = Vec::new();

    for matrix_row_pair in pair.into_inner() {
        if let Rule::matrix_row = matrix_row_pair.as_rule() {
            let mut row: Vec<Value> = Vec::new();

            for element in matrix_row_pair.into_inner() {
                if let Rule::expression = element.as_rule() {
                    let expr = parse_expression(element)?;
                    match expr {
                        Expression::Literal(Value::Number(f)) => row.push(Value::Number(f)),
                        Expression::Literal(Value::Integer(i)) => row.push(Value::Integer(i)),
                        Expression::Literal(v) => row.push(v),
                        _ => {
                            return Err(CompilerError::parse_error(
                                "Matrix literals can only contain literal values".to_string(),
                                None,
                                Some("Use literal values in matrix definitions".to_string()),
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
    let location = convert_to_ast_location(&get_location(&pair));
    let mut fields = Vec::new();

    for pair_element in pair.into_inner() {
        if let Rule::pair_element = pair_element.as_rule() {
            let mut pair_parts = pair_element.into_inner();

            // Parse the key (string, identifier, or decimal_integer)
            let key_part = pair_parts
                .next()
                .expect("invariant: pair_element key child");
            let key_value = match key_part.as_rule() {
                Rule::string => match parse_string(key_part)? {
                    Expression::Literal(Value::String(s)) => Value::String(s),
                    _ => {
                        return Err(CompilerError::parse_error(
                            "Invalid string key in pairs literal".to_string(),
                            None,
                            None,
                        ))
                    }
                },
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

            // Parse the value as a full single_line_expression — variables,
            // calls, binary ops, etc. are preserved as Expression nodes
            // (not stringified). See ANON-OBJ-LITERAL-NOT-EVALUATED.
            let value_part = pair_parts
                .next()
                .expect("invariant: pair_element value child");
            let value_expr = parse_expression(value_part)?;

            fields.push((key_value, value_expr));
        }
    }

    Ok(Expression::ObjectLiteral { fields, location })
}

pub fn parse_function_call(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = convert_to_ast_location(&get_location(&pair));
    let mut inner = pair.into_inner();
    let name = inner
        .next()
        .expect("invariant: function_call name child")
        .as_str()
        .to_string();
    let mut arguments = Vec::new();

    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        // Parse argument_item (named or positional) or legacy argument_expression
                        arguments.push(parse_argument_item(arg_expr)?);
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
    let namespace = inner
        .next()
        .expect("invariant: namespace_function_call namespace child")
        .as_str()
        .to_string();

    // Second child is the function identifier
    let function = inner
        .next()
        .expect("invariant: namespace_function_call function child")
        .as_str()
        .to_string();

    let mut arguments = Vec::new();

    // Parse remaining arguments
    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                for arg_expr in arg.into_inner() {
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        arguments.push(parse_argument_item(arg_expr)?);
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
    let namespace = inner
        .next()
        .expect("invariant: namespace_method_chain namespace child")
        .as_str()
        .to_string();

    // Second child is the function identifier
    let function = inner
        .next()
        .expect("invariant: namespace_method_chain function child")
        .as_str()
        .to_string();

    let mut arguments = Vec::new();

    // Collect all children to distinguish arguments from method_call_segments
    let remaining: Vec<_> = inner.collect();
    let mut i = 0;

    // Parse arguments (before method_call_segment)
    while i < remaining.len() {
        match remaining[i].as_rule() {
            Rule::argument_list => {
                for arg_expr in remaining[i].clone().into_inner() {
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        arguments.push(parse_argument_item(arg_expr)?);
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
            let first_child = seg_inner
                .next()
                .expect("invariant: method_call_segment grammar child");

            let (method_name, method_arguments) = match first_child.as_rule() {
                Rule::method_name => {
                    let method_name = first_child.as_str().to_string();
                    let mut method_arguments = Vec::new();

                    // Parse arguments from the remaining segments
                    for arg in seg_inner {
                        match arg.as_rule() {
                            Rule::argument_list => {
                                for arg_expr in arg.into_inner() {
                                    if matches!(
                                        arg_expr.as_rule(),
                                        Rule::argument_item | Rule::argument_expression
                                    ) {
                                        method_arguments.push(parse_argument_item(arg_expr)?);
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
    let base_pair = inner.next().expect("invariant: method_call base child");
    let object_expr = match base_pair.as_rule() {
        Rule::method_call_base => {
            let mut base_inner = base_pair.into_inner();
            let first = base_inner
                .next()
                .expect("invariant: method_call_base inner child");
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
                // null-support - Parse none literal in method call base
                Rule::none_literal => Expression::Literal(Value::None),
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
            let first_child = seg_inner
                .next()
                .expect("invariant: method_call_segment inner child");

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
                                    if matches!(
                                        arg_expr.as_rule(),
                                        Rule::argument_item | Rule::argument_expression
                                    ) {
                                        // Parse argument_item (named or positional) or legacy argument_expression
                                        arguments.push(parse_argument_item(arg_expr)?);
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
    let paren_expr_pair = inner
        .next()
        .expect("invariant: parenthesized_method_call paren child");
    // parenthesized_expr contains multiline_logical_expression, so extract it
    let expr_inside = paren_expr_pair
        .into_inner()
        .next()
        .expect("invariant: parenthesized_expr inner child");
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
        let first_child = segment_inner
            .next()
            .expect("invariant: parenthesized_method_call segment child");

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
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        arguments.push(parse_argument_item(arg_expr)?);
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
    let object_name = inner
        .next()
        .expect("invariant: property_access object child")
        .as_str()
        .to_string();
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
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        // Parse argument_item (named or positional) or legacy argument_expression
                        arguments.push(parse_argument_item(arg_expr)?);
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
    let list_name = inner
        .next()
        .expect("invariant: list_access identifier child")
        .as_str()
        .to_string();
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
    tracing::trace!("DEBUG: parse_base_call called");
    let location = get_location(&pair);
    let mut arguments = Vec::new();

    for arg in pair.into_inner() {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        // Parse argument_item (named or positional) or legacy argument_expression
                        arguments.push(parse_argument_item(arg_expr)?);
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
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        // Parse argument_item (named or positional) or legacy argument_expression
                        arguments.push(parse_argument_item(arg_expr)?);
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
    let static_class_name_pair = inner
        .next()
        .expect("invariant: static_method_call class name child");
    let full_class_name = static_class_name_pair.as_str().to_string();
    let method_name = inner
        .next()
        .expect("invariant: static_method_call method name child")
        .as_str()
        .to_string();
    let mut arguments = Vec::new();

    // Parse arguments
    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        // Parse argument_item (named or positional) or legacy argument_expression
                        arguments.push(parse_argument_item(arg_expr)?);
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
    // e.g., "math.abs" -> namespace=["math"], class_name="abs"
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
    let namespace = inner
        .next()
        .expect("invariant: three_level_method_call namespace child")
        .as_str()
        .to_string();
    let subnamespace = inner
        .next()
        .expect("invariant: three_level_method_call subnamespace child")
        .as_str()
        .to_string();
    let method_name = inner
        .next()
        .expect("invariant: three_level_method_call method child")
        .as_str()
        .to_string();
    let mut arguments = Vec::new();

    // Parse arguments
    for arg in inner {
        match arg.as_rule() {
            Rule::argument_list => {
                // Parse argument list - contains argument_expression items
                for arg_expr in arg.into_inner() {
                    if matches!(
                        arg_expr.as_rule(),
                        Rule::argument_item | Rule::argument_expression
                    ) {
                        // Parse argument_item (named or positional) or legacy argument_expression
                        arguments.push(parse_argument_item(arg_expr)?);
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
    let base_pair = inner
        .next()
        .expect("invariant: chained_method_call base child");
    let mut current_expr =
        match base_pair.as_rule() {
            Rule::static_method_call => parse_static_method_call(base_pair)?,
            Rule::function_call => parse_function_call(base_pair)?,
            // Fingerprint 0f628d47cad7: `Foo().bar()` — the constructor_call
            // arm of chained_method_call handed us a class instantiation whose
            // result should feed the trailing method_call_segment+.
            Rule::constructor_call => parse_constructor_call(base_pair)?,
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
                    "Expected static method call, function call, constructor call, property access, or identifier"
                        .to_string(),
                ),
            )),
        };

    // Now process all the method_call_segment rules
    for segment in inner {
        if let Rule::method_call_segment = segment.as_rule() {
            let mut seg_inner = segment.into_inner();
            let first_child = seg_inner
                .next()
                .expect("invariant: method_call_segment inner child");

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
                                    if matches!(
                                        arg_expr.as_rule(),
                                        Rule::argument_item | Rule::argument_expression
                                    ) {
                                        // Parse argument_item (named or positional) or legacy argument_expression
                                        arguments.push(parse_argument_item(arg_expr)?);
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
    let expression = parse_expression(
        inner
            .next()
            .expect("invariant: start_expression grammar child"),
    )?;

    Ok(Expression::StartExpression {
        expression: Box::new(expression),
        location: convert_to_ast_location(&location),
    })
}

pub fn parse_argument_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
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

/// Parse an `argument_item` — either a `named_argument` or a plain `argument_expression`.
///
/// grammar.pest `argument_item = { named_argument | argument_expression }`
///
/// This is the adapter for the legacy Pest-based parser to handle named argument syntax
/// introduced by grammar.ebnf FUNC008–FUNC011.  Named arguments produce an
/// `Expression::NamedArgBinding`; plain arguments delegate to `parse_argument_expression`.
pub fn parse_argument_item(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    match pair.as_rule() {
        Rule::argument_item => {
            let inner = pair.into_inner().next().ok_or_else(|| {
                CompilerError::parse_error(
                    "Empty argument item".to_string(),
                    Some(convert_to_ast_location(&location)),
                    None,
                )
            })?;
            parse_argument_item(inner)
        }
        Rule::named_argument => {
            // named_argument = { parameter_name ~ ":" ~ argument_expression }
            let mut inner = pair.into_inner();
            let label_pair = inner.next().ok_or_else(|| {
                CompilerError::parse_error(
                    "Missing label in named argument".to_string(),
                    Some(convert_to_ast_location(&location)),
                    None,
                )
            })?;
            let label = label_pair.as_str().to_string();
            let loc = convert_to_ast_location(&get_location(&label_pair));

            // Skip colon (implicit in the rule structure — it's a separator, not a node)
            let value_pair = inner.next().ok_or_else(|| {
                CompilerError::parse_error(
                    "Missing value in named argument".to_string(),
                    Some(convert_to_ast_location(&location)),
                    None,
                )
            })?;
            let value = parse_argument_expression(value_pair)?;

            Ok(Expression::NamedArgBinding {
                label,
                value: Box::new(value),
                location: loc,
            })
        }
        Rule::argument_expression => parse_argument_expression(pair),
        _ => Err(CompilerError::parse_error(
            format!("Unexpected rule in argument item: {:?}", pair.as_rule()),
            Some(convert_to_ast_location(&location)),
            Some("Expected named_argument or argument_expression".to_string()),
        )),
    }
}

// Argument-specific expression parsing functions (supports logical, comparison, arithmetic)

pub fn parse_argument_logical(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    let location = get_location(&pair);
    let mut pairs = pair.into_inner();
    let first = pairs
        .next()
        .expect("invariant: argument_logical left operand");

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
    let first = pairs
        .next()
        .expect("invariant: argument_comparison left operand");

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
    let first = pairs
        .next()
        .expect("invariant: argument_additive left operand");

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
    let first = pairs
        .next()
        .expect("invariant: argument_multiplicative left operand");

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
    let first = pairs
        .next()
        .expect("invariant: argument_power left operand");

    let mut left = match first.as_rule() {
        Rule::argument_primary => parse_argument_primary(first)?,
        _ => parse_argument_primary(first)?,
    };

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
        // BOOK: null-coalescing - single_line_expression now uses single_line_default_expression
        Rule::single_line_default_expression => parse_single_line_default_expression(inner),
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

// single_line_default_expression is now a pass-through wrapper for single_line_logical_expression
pub fn parse_single_line_default_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    if let Some(inner) = pair.into_inner().next() {
        match inner.as_rule() {
            Rule::single_line_logical_expression => parse_single_line_logical_expression(inner),
            _ => Err(CompilerError::parse_error(
                format!(
                    "Unexpected rule in single_line_default_expression: {:?}",
                    inner.as_rule()
                ),
                Some(convert_to_ast_location(&get_location(&inner))),
                None,
            )),
        }
    } else {
        Err(CompilerError::parse_error(
            "Empty single_line_default_expression".to_string(),
            None,
            None,
        ))
    }
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

// multiline_default_expression is now a pass-through wrapper for multiline_logical_expression
pub fn parse_multiline_default_expression(pair: Pair<Rule>) -> Result<Expression, CompilerError> {
    if let Some(inner) = pair.into_inner().next() {
        match inner.as_rule() {
            Rule::multiline_logical_expression => parse_multiline_logical_expression(inner),
            _ => Err(CompilerError::parse_error(
                format!(
                    "Unexpected rule in multiline_default_expression: {:?}",
                    inner.as_rule()
                ),
                Some(convert_to_ast_location(&get_location(&inner))),
                None,
            )),
        }
    } else {
        Err(CompilerError::parse_error(
            "Empty multiline_default_expression".to_string(),
            None,
            None,
        ))
    }
}

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
    let op_count = op_stack.len();
    let mut i = 0;

    // BUG (fp 7793fbeec120): the old condition was
    //   while i < op_stack.len() && i < expr_stack.len()
    // which is wrong because `expr_stack.remove(0)` inside the loop shrinks
    // `expr_stack.len()`. After the initial `remove(0)`, the invariant is
    // `expr_stack.len() = op_stack.len() - i`, so the condition failed one
    // iteration early — dropping the last operand of chains with 4+ operands
    // (e.g. `a + b + c + d`). The fix iterates against the stable op_count
    // captured before the loop.
    while i < op_count && !expr_stack.is_empty() {
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
    let op_count = op_stack.len();
    let mut i = 0;

    // BUG (fp 7793fbeec120): the old condition was
    //   while i < op_stack.len() && i < expr_stack.len()
    // which is wrong because `expr_stack.remove(0)` inside the loop shrinks
    // `expr_stack.len()`. After the initial `remove(0)`, the invariant is
    // `expr_stack.len() = op_stack.len() - i`, so the condition failed one
    // iteration early — dropping the last operand of chains with 4+ operands
    // (e.g. `a + b + c + d`). The fix iterates against the stable op_count
    // captured before the loop.
    while i < op_count && !expr_stack.is_empty() {
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
    let op_count = op_stack.len();
    let mut i = 0;

    // BUG (fp 7793fbeec120): the old condition was
    //   while i < op_stack.len() && i < expr_stack.len()
    // which is wrong because `expr_stack.remove(0)` inside the loop shrinks
    // `expr_stack.len()`. After the initial `remove(0)`, the invariant is
    // `expr_stack.len() = op_stack.len() - i`, so the condition failed one
    // iteration early — dropping the last operand of chains with 4+ operands
    // (e.g. `a + b + c + d`). The fix iterates against the stable op_count
    // captured before the loop.
    while i < op_count && !expr_stack.is_empty() {
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
    let op_count = op_stack.len();
    let mut i = 0;

    // BUG (fp 7793fbeec120): the old condition was
    //   while i < op_stack.len() && i < expr_stack.len()
    // which is wrong because `expr_stack.remove(0)` inside the loop shrinks
    // `expr_stack.len()`. After the initial `remove(0)`, the invariant is
    // `expr_stack.len() = op_stack.len() - i`, so the condition failed one
    // iteration early — dropping the last operand of chains with 4+ operands
    // (e.g. `a + b + c + d`). The fix iterates against the stable op_count
    // captured before the loop.
    while i < op_count && !expr_stack.is_empty() {
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

    let mut result = expr_stack
        .pop()
        .expect("invariant: non-empty stack checked above");

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::CleanParser;
    use pest::Parser;

    #[test]
    fn test_none_literal_parsing() {
        // Test that "none" is parsed as none_literal rule
        let result = CleanParser::parse(Rule::none_literal, "none");
        assert!(
            result.is_ok(),
            "none should parse as none_literal: {:?}",
            result.err()
        );

        let pairs: Vec<_> = result.expect("test: parse succeeded above").collect();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].as_rule(), Rule::none_literal);
        assert_eq!(pairs[0].as_str(), "none");
    }

    #[test]
    fn test_none_in_primary() {
        // Test that "none" is matched in primary rule
        let result = CleanParser::parse(Rule::primary, "none");
        assert!(
            result.is_ok(),
            "none should parse in primary: {:?}",
            result.err()
        );

        let mut pairs = result.expect("test: parse succeeded above");
        let primary = pairs.next().expect("test: primary pair exists");
        assert_eq!(primary.as_rule(), Rule::primary);

        // The inner rule should be none_literal
        let inner = primary
            .into_inner()
            .next()
            .expect("test: none_literal inner child");
        assert_eq!(
            inner.as_rule(),
            Rule::none_literal,
            "Expected none_literal, got {:?}",
            inner.as_rule()
        );
    }

    #[test]
    fn test_none_not_identifier() {
        // Test that "none" is NOT parsed as an identifier
        let result = CleanParser::parse(Rule::identifier, "none");
        assert!(result.is_err(), "none should NOT parse as identifier");
    }

    #[test]
    fn test_none_expression_parsing() {
        // Test that "none" becomes Expression::Literal(Value::None)
        let result = CleanParser::parse(Rule::expression, "none");
        assert!(
            result.is_ok(),
            "none should parse as expression: {:?}",
            result.err()
        );

        let mut pairs = result.expect("test: parse succeeded above");
        let expr_pair = pairs.next().expect("test: expression pair exists");
        let expr = parse_expression(expr_pair).expect("Should parse expression");

        match expr {
            Expression::Literal(Value::None) => {
                // Success!
            }
            Expression::Variable(name) => {
                panic!(
                    "none was parsed as Variable('{}') instead of Literal(None)",
                    name
                );
            }
            other => {
                panic!("none was parsed as {:?} instead of Literal(None)", other);
            }
        }
    }

    #[test]
    fn test_none_in_argument() {
        // Test that "none" parses correctly when used as a function argument
        let result = CleanParser::parse(Rule::argument_expression, "none");
        assert!(
            result.is_ok(),
            "none should parse as argument_expression: {:?}",
            result.err()
        );

        let mut pairs = result.expect("test: parse succeeded above");
        let expr_pair = pairs.next().expect("test: argument_expression pair exists");
        let expr = parse_argument_expression(expr_pair).expect("Should parse argument");

        match expr {
            Expression::Literal(Value::None) => {
                // Success!
            }
            Expression::Variable(name) => {
                panic!(
                    "none in argument was parsed as Variable('{}') instead of Literal(None)",
                    name
                );
            }
            other => {
                panic!(
                    "none in argument was parsed as {:?} instead of Literal(None)",
                    other
                );
            }
        }
    }

    #[test]
    fn test_none_full_program() {
        // Test parsing a full program with none
        let program_src = "start:\n\tprint(none)";

        let result = CleanParser::parse_program(program_src);
        assert!(
            result.is_ok(),
            "Program with none should parse: {:?}",
            result.err()
        );

        let program = result.expect("test: parse succeeded above");
        assert!(
            program.start_function.is_some(),
            "Program should have start function"
        );
        let start_fn = program
            .start_function
            .expect("test: start function present per assert above");
        assert!(!start_fn.body.is_empty(), "Start function should have body");
    }

    #[test]
    fn test_none_print_statement() {
        // Test parsing just the print statement
        let stmt = "print(none)";
        let result = CleanParser::parse(Rule::print_parenthesized_stmt, stmt);
        assert!(
            result.is_ok(),
            "print(none) should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_none_parse_and_convert() {
        use crate::parser::statement_parser::parse_statement;

        // Parse a statement with none
        let stmt = "print(none)";
        let result = CleanParser::parse(Rule::statement, stmt);
        assert!(result.is_ok(), "statement should parse: {:?}", result.err());

        let mut pairs = result.expect("test: parse succeeded above");
        let stmt_pair = pairs.next().expect("test: statement pair exists");

        // Convert to AST
        let ast_stmt = parse_statement(stmt_pair);
        assert!(
            ast_stmt.is_ok(),
            "Statement parsing should succeed: {:?}",
            ast_stmt.err()
        );

        let stmt = ast_stmt.expect("test: statement parse succeeded per assert above");

        // Check that the expression is Literal(None)
        if let crate::ast::Statement::Print { expression, .. } = stmt {
            match expression {
                Expression::Literal(Value::None) => {
                    // Success!
                }
                Expression::Variable(name) => {
                    panic!(
                        "none was parsed as Variable('{}') instead of Literal(None)",
                        name
                    );
                }
                other => {
                    panic!("none was parsed as {:?} instead of Literal(None)", other);
                }
            }
        } else {
            panic!("Expected Print statement, got {:?}", stmt);
        }
    }

    #[test]
    fn test_none_compile_pipeline() {
        // Test the full compilation pipeline with none
        let source = "start:\n\tprint(none)";

        // Parse
        let program = CleanParser::parse_program(source);
        assert!(program.is_ok(), "Program should parse: {:?}", program.err());
        let program = program.expect("test: parse succeeded per assert above");

        // Build HIR - build_hir takes owned Program
        let mut hir_builder = crate::hir::hir_builder::HirBuilder::new();
        let hir_result = hir_builder.build_hir(program);
        assert!(
            hir_result.is_ok(),
            "HIR should build: {:?}",
            hir_result.err()
        );
        let hir_result = hir_result.expect("test: HIR build succeeded per assert above");

        // Check that none is in the HIR as a literal
        if let Some(start_fn) = &hir_result.hir.start_function {
            if let Some(crate::hir::HirStatement::Print { expression, .. }) =
                start_fn.body.statements.first()
            {
                match expression {
                    crate::hir::HirExpression::Literal { value, .. } => {
                        assert!(
                            matches!(value, Value::None),
                            "Expected Value::None, got {:?}",
                            value
                        );
                    }
                    other => {
                        panic!("none was converted to {:?} instead of Literal(None)", other);
                    }
                }
            } else {
                panic!(
                    "Expected Print statement in start function, got {:?}",
                    start_fn.body.statements
                );
            }
        } else {
            panic!("Expected start function in HirProgram");
        }
    }
}
