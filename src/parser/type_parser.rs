use pest::iterators::Pair;
use crate::ast::Type;
use crate::error::CompilerError;
use super::{get_location, convert_to_ast_location};
use super::Rule;

pub fn parse_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let parser_location = get_location(&pair);
    let ast_location = convert_to_ast_location(&parser_location);
    
    // Handle both direct type rules and type_ wrapper
    let type_rule = if pair.as_rule() == Rule::type_ {
        pair.clone().into_inner().next().ok_or_else(|| CompilerError::parse_error(
            "Empty type declaration".to_string(),
            Some(convert_to_ast_location(&parser_location)),
            Some("Type declarations must specify a type".to_string())
        ))?
    } else {
        pair.clone()
    };
    
    match type_rule.as_rule() {
        Rule::input_type => {
            // input_type is a wrapper, extract the inner type
            let inner_type = type_rule.into_inner().next().ok_or_else(|| CompilerError::parse_error(
                "Empty input type declaration".to_string(),
                Some(ast_location.clone()),
                Some("Input type declarations must specify a type".to_string())
            ))?;
            parse_type(inner_type)
        },
        Rule::core_type => {
            let type_str = type_rule.as_str();
            match type_str {
                "boolean" => Ok(Type::Boolean),
                "integer" => Ok(Type::Integer),
                "number" => Ok(Type::Number),
                "string" => Ok(Type::String),
                "void" => Ok(Type::Void),
                "any" => Ok(Type::Any),
                _ => Err(CompilerError::parse_error(
                    format!("Unknown core type: {}", type_str),
                    Some(ast_location),
                    Some("Valid core types are: boolean, integer, number, string, void, any".to_string())
                ))
            }
        },
        Rule::sized_type => parse_sized_type(type_rule),
        Rule::generic_type => parse_generic_type(type_rule),
        Rule::type_parameter => {
            let type_name = type_rule.as_str().to_string();
            // Handle all type parameters uniformly
            Ok(Type::TypeParameter(type_name))
        },
        Rule::identifier => parse_basic_type(type_rule),
        Rule::matrix_type => parse_matrix_type(type_rule),
        Rule::list_type => parse_list_type(type_rule),
        Rule::pairs_type => parse_pairs_type(type_rule),
        Rule::function_type => {
            // function_type contains one of: sized_type, core_type, matrix_type, array_type, pairs_type, generic_type, type_parameter
            let inner_type = type_rule.into_inner().next().ok_or_else(|| CompilerError::parse_error(
                "Empty function type declaration".to_string(),
                Some(convert_to_ast_location(&parser_location)),
                Some("Function type declarations must specify a type".to_string())
            ))?;
            
            match inner_type.as_rule() {
                Rule::core_type => {
                    let type_str = inner_type.as_str();
                    match type_str {
                        "boolean" => Ok(Type::Boolean),
                        "integer" => Ok(Type::Integer),
                        "number" => Ok(Type::Number),
                        "string" => Ok(Type::String),
                        "void" => Ok(Type::Void),
                        "any" => Ok(Type::Any),
                        _ => Err(CompilerError::parse_error(
                            format!("Unknown core type: {type_str}"),
                            None,
                            Some("Valid core types are: boolean, integer, number, string, void, any".to_string())
                        ))
                    }
                },
                Rule::sized_type => {
                    let mut inner_parts = inner_type.into_inner();
                    let core_type_pair = inner_parts.next().unwrap();
                    let size_spec = inner_parts.next().unwrap().as_str();
                    
                    let base_type = match core_type_pair.as_str() {
                        "boolean" => Type::Boolean,
                        "integer" => Type::Integer,
                        "number" => Type::Number,
                        "string" => Type::String,
                        "void" => Type::Void,
                        "any" => Type::Any,
                        _ => return Err(CompilerError::parse_error(
                            format!("Unknown core type in sized type: {}", core_type_pair.as_str()),
                            None,
                            Some("Valid core types are: boolean, integer, number, string, void, any".to_string())
                        ))
                    };
                    
                    let size_str = &size_spec[1..].trim();
                    let (bits, unsigned) = if let Some(bits_str) = size_str.strip_suffix('u') {
                        (bits_str.parse::<u8>().unwrap_or(32), true)
                    } else {
                        (size_str.parse::<u8>().unwrap_or(32), false)
                    };
                    
                    match base_type {
                        Type::Integer => Ok(Type::IntegerSized { bits, unsigned }),
                        Type::Number => Ok(Type::NumberSized { bits }),
                        _ => Err(CompilerError::parse_error(
                            "Size specifiers can only be used with integer and number types".to_string(),
                            None,
                            Some("Use size specifiers like :8, :16, :32, :64, or :8u for unsigned".to_string())
                        ))
                    }
                },
                Rule::matrix_type => parse_matrix_type(inner_type),
                Rule::list_type => parse_list_type(inner_type),
                Rule::pairs_type => parse_pairs_type(inner_type),
                Rule::generic_type => parse_generic_type(inner_type),
                Rule::type_parameter => Ok(Type::TypeParameter(inner_type.as_str().to_string())),
                _ => Err(CompilerError::parse_error(
                    format!("Unexpected function type: {:?}", inner_type.as_rule()),
                    Some(ast_location),
                    Some("Function type declarations must specify a valid type".to_string())
                ))
            }
        },
        Rule::core_type => {
            let type_str = type_rule.as_str();
            match type_str {
                "boolean" => Ok(Type::Boolean),
                "integer" => Ok(Type::Integer),
                "number" => Ok(Type::Number),
                "string" => Ok(Type::String),
                "void" => Ok(Type::Void),
                "any" => Ok(Type::Any),
                _ => Err(CompilerError::parse_error(
                    format!("Unknown core type: {type_str}"),
                    None,
                    Some("Valid core types are: boolean, integer, number, string, void, any".to_string())
                ))
            }
        },
        Rule::sized_type => {
            let mut inner_parts = type_rule.into_inner();
            let core_type_pair = inner_parts.next().unwrap();
            let size_spec = inner_parts.next().unwrap().as_str();
            
            // Parse the core type directly (it's not a type_ rule, so we can't use parse_type)
            let base_type = match core_type_pair.as_str() {
                "boolean" => Type::Boolean,
                "integer" => Type::Integer,
                "number" => Type::Number,
                "float" => Type::Number,
                "string" => Type::String,
                "void" => Type::Void,
                "any" => Type::Any,
                _ => return Err(CompilerError::parse_error(
                    format!("Unknown core type in sized type: {}", core_type_pair.as_str()),
                    None,
                    Some("Valid core types are: boolean, integer, number, float, string, void, any".to_string())
                ))
            };
            
            // Parse size specifier like ":8" or ":8u"
            let size_str = &size_spec[1..].trim(); // Remove the ':' and trim whitespace
            let (bits, unsigned) = if size_str.ends_with('u') {
                let bits_str = size_str.strip_suffix('u').unwrap();
                (bits_str.parse::<u8>().unwrap_or(32), true)
            } else {
                (size_str.parse::<u8>().unwrap_or(32), false)
            };
            
            match base_type {
                Type::Integer => Ok(Type::IntegerSized { bits, unsigned }),
                Type::Number => Ok(Type::NumberSized { bits }),
                _ => Err(CompilerError::parse_error(
                    "Size specifiers can only be used with integer and number types".to_string(),
                    None,
                    Some("Use size specifiers like :8, :16, :32, :64, or :8u for unsigned".to_string())
                ))
            }
        },
        _ => Err(CompilerError::parse_error(
            format!("Unexpected type: {:?}", type_rule.as_rule()),
            Some(ast_location),
            Some("Type declarations must specify a valid type".to_string())
        ))
    }
}

fn parse_generic_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let parser_location = get_location(&pair);
    let ast_location = convert_to_ast_location(&parser_location);
    
    let mut base_type = String::new();
    let mut type_arguments = Vec::new();
    
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => base_type = inner.as_str().to_string(),
            Rule::type_arguments => {
                for type_arg in inner.into_inner() {
                    type_arguments.push(parse_type(type_arg)?);
                }
            },
            _ => return Err(CompilerError::parse_error(
                format!("Unexpected rule in generic type: {:?}", inner.as_rule()),
                Some(ast_location),
                Some("Generic types must have a base type and optional type arguments".to_string())
            ))
        }
    }
    
    let boxed_base_type = Box::new(Type::Object(base_type));
    Ok(Type::Generic(boxed_base_type, type_arguments))
}

fn parse_basic_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let parser_location = get_location(&pair);
    let _ast_location = convert_to_ast_location(&parser_location);
    
    let type_name = pair.as_str();
    match type_name {
        "int" => Ok(Type::Integer),
        "number" => Ok(Type::Number),  // number maps to Number type
        "bool" => Ok(Type::Boolean),
        "string" => Ok(Type::String),
        "void" => Ok(Type::Void),
        _ => Ok(Type::Object(type_name.to_string()))
    }
}

fn parse_matrix_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let parser_location = get_location(&pair);
    let ast_location = convert_to_ast_location(&parser_location);
    
    let element_type = pair.into_inner().next()
        .ok_or_else(|| CompilerError::parse_error(
            "Matrix type must specify element type".to_string(),
            Some(ast_location),
            Some("Matrix types must be in the form matrix<T>".to_string())
        ))?;
    
    let element_type = parse_type(element_type)?;
    Ok(Type::Matrix(Box::new(element_type)))
}


fn parse_list_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let parser_location = get_location(&pair);
    let ast_location = convert_to_ast_location(&parser_location);
    
    let element_type = pair.into_inner().next()
        .ok_or_else(|| CompilerError::parse_error(
            "List type must specify element type".to_string(),
            Some(ast_location),
            Some("List types must be in the form list<T>".to_string())
        ))?;
    
    let element_type = parse_type(element_type)?;
    Ok(Type::List(Box::new(element_type)))
}

fn parse_pairs_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let parser_location = get_location(&pair);
    let ast_location = convert_to_ast_location(&parser_location);
    
    let mut key_type = None;
    let mut value_type = None;
    
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::type_ {
            if key_type.is_none() {
                key_type = Some(parse_type(inner)?);
            } else if value_type.is_none() {
                value_type = Some(parse_type(inner)?);
            }
        }
    }
    
    let key_type = key_type.ok_or_else(|| CompilerError::parse_error(
        "pairs type must specify key type".to_string(),
        Some(ast_location.clone()),
        Some("pairs types must be in the form pairs<K, V>".to_string())
    ))?;
    
    let value_type = value_type.ok_or_else(|| CompilerError::parse_error(
        "pairs type must specify value type".to_string(),
        Some(ast_location),
        Some("pairs types must be in the form pairs<K, V>".to_string())
    ))?;
    
    Ok(Type::Pairs(Box::new(key_type), Box::new(value_type)))
}

fn parse_sized_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let mut inner_parts = pair.into_inner();
    let core_type_pair = inner_parts.next().unwrap();
    let size_spec = inner_parts.next().unwrap().as_str();
    
    let base_type = match core_type_pair.as_str() {
        "boolean" => Type::Boolean,
        "integer" => Type::Integer,
        "number" => Type::Number,
        "float" => Type::Number,
        "string" => Type::String,
        "void" => Type::Void,
        "any" => Type::Any,
        _ => return Err(CompilerError::parse_error(
            format!("Unknown core type in sized type: {}", core_type_pair.as_str()),
            None,
            Some("Valid core types are: boolean, integer, number, float, string, void, any".to_string())
        ))
    };
    
    // Parse size specifier like ":8" or ":8u"
    let size_str = &size_spec[1..].trim(); // Remove the ':' and trim whitespace
    let (bits, unsigned) = if size_str.ends_with('u') {
        let bits_str = size_str.strip_suffix('u').unwrap();
        (bits_str.parse::<u8>().unwrap_or(32), true)
    } else {
        (size_str.parse::<u8>().unwrap_or(32), false)
    };
    
    match base_type {
        Type::Integer => Ok(Type::IntegerSized { bits, unsigned }),
        Type::Number => Ok(Type::NumberSized { bits }),
        _ => Err(CompilerError::parse_error(
            "Size specifiers can only be used with integer and number types".to_string(),
            None,
            Some("Use size specifiers like :8, :16, :32, :64, or :8u for unsigned".to_string())
        ))
    }
} 