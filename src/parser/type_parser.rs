use super::get_location;
use super::Rule;
use crate::ast::{ListBehavior, Type};
use crate::error::CompilerError;
use pest::iterators::Pair;

pub fn parse_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let ast_location = get_location(&pair);

    match pair.as_rule() {
        // Wrapper rules around a single concrete type.
        Rule::type_ | Rule::variable_type | Rule::function_return_type | Rule::input_type => {
            let inner_type = pair.into_inner().next().ok_or_else(|| {
                CompilerError::parse_error(
                    "Empty type declaration".to_string(),
                    Some(ast_location),
                    Some("Type declarations must specify a type".to_string()),
                )
            })?;
            parse_type(inner_type)
        }
        Rule::core_type => {
            let type_str = pair.as_str();
            match type_str {
                "boolean" => Ok(Type::Boolean),
                "integer" => Ok(Type::Integer),
                "number" => Ok(Type::Number),
                "string" => Ok(Type::String),
                "void" => Ok(Type::Void),
                "any" => Ok(Type::Any),
                _ => Err(CompilerError::parse_error(
                    format!("Unknown core type: {type_str}"),
                    Some(ast_location),
                    Some(
                        "Valid core types are: boolean, integer, number, string, void, any"
                            .to_string(),
                    ),
                )),
            }
        }
        Rule::sized_type => parse_sized_type(pair),
        Rule::generic_type => parse_generic_type(pair),
        Rule::type_parameter => Ok(Type::TypeParameter(pair.as_str().to_string())),
        // A named type (grammar `identifier` excludes the type keywords): a
        // class or other user-defined type, as the token parser builds it.
        Rule::identifier => Ok(Type::Object(pair.as_str().to_string())),
        Rule::matrix_type => parse_matrix_type(pair),
        Rule::list_type => parse_list_type(pair),
        Rule::pairs_type => parse_pairs_type(pair),
        _ => Err(CompilerError::parse_error(
            format!("Unexpected type: {:?}", pair.as_rule()),
            Some(ast_location),
            Some("Type declarations must specify a valid type".to_string()),
        )),
    }
}

fn parse_generic_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    let parser_location = get_location(&pair);
    let ast_location = parser_location.clone();

    let mut base_type = String::new();
    let mut type_arguments = Vec::new();

    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::identifier => base_type = inner.as_str().to_string(),
            Rule::type_arguments => {
                for type_arg in inner.into_inner() {
                    type_arguments.push(parse_type(type_arg)?);
                }
            }
            _ => {
                return Err(CompilerError::parse_error(
                    format!("Unexpected rule in generic type: {:?}", inner.as_rule()),
                    Some(ast_location),
                    Some(
                        "Generic types must have a base type and optional type arguments"
                            .to_string(),
                    ),
                ))
            }
        }
    }

    let boxed_base_type = Box::new(Type::Object(base_type));
    Ok(Type::Generic(boxed_base_type, type_arguments))
}

fn parse_matrix_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    // Bare `matrix` is shorthand for `matrix<any>` per root spec
    // 04-type-system.md §"Bare Generic Type Shorthand". When the grammar
    // matches `matrix` without a `<T>` type argument, `into_inner()` yields
    // no children — fall through to the Any default.
    let element_type = match pair.into_inner().next() {
        Some(inner) => parse_type(inner)?,
        None => Type::Any,
    };
    Ok(Type::Matrix(Box::new(element_type)))
}

fn parse_list_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    // Bare `list` is shorthand for `list<any>` per root spec
    // 04-type-system.md §"Bare Generic Type Shorthand". The list_behavior
    // rule (if present) is not a `type_`, so we only take the first `type_`
    // child if any exists.
    let mut element_type = Type::Any;
    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::type_ {
            element_type = parse_type(inner)?;
            break;
        }
    }
    // Legacy parser path: no behaviour modifier handled here.
    // The token-based parser (declarations.rs) is the production path and
    // captures `.line` / `.pile` / `.unique` from the source.
    Ok(Type::List(Box::new(element_type), ListBehavior::Default))
}

fn parse_pairs_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    // Bare `pairs` is shorthand for `pairs<string, any>` per root spec
    // 04-type-system.md §"Bare Generic Type Shorthand". The grammar rule
    // `pairs_type = "pairs" ~ ("<" ... ">")? ...` guarantees the `<K,V>`
    // block is atomic — either both types are present (explicit form) or
    // both are absent (bare form). A single-typed `pairs<T>` is a grammar
    // failure, not something this function needs to defend against.
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

    let (key_type, value_type) = match (key_type, value_type) {
        (Some(k), Some(v)) => (k, v),
        _ => (Type::String, Type::Any),
    };

    Ok(Type::Pairs(Box::new(key_type), Box::new(value_type)))
}

fn parse_sized_type(pair: Pair<Rule>) -> Result<Type, CompilerError> {
    // Since sized_type is atomic (@), parse the entire string manually
    let full_string = pair.as_str();

    // Split on ':' to get the core type and size specifier
    let parts: Vec<&str> = full_string.split(':').collect();
    if parts.len() != 2 {
        return Err(CompilerError::parse_error(
            format!("Invalid sized type format: {}", full_string),
            None,
            Some("Sized types should be in format 'type:size' (e.g., 'number:64')".to_string()),
        ));
    }

    let core_type_str = parts[0];
    let size_spec = parts[1];

    let base_type = match core_type_str {
        "boolean" => Type::Boolean,
        "integer" => Type::Integer,
        "number" => Type::Number,
        "float" => Type::Number,
        "string" => Type::String,
        "void" => Type::Void,
        "any" => Type::Any,
        _ => {
            return Err(CompilerError::parse_error(
                format!("Unknown core type in sized type: {}", core_type_str),
                None,
                Some(
                    "Valid core types are: boolean, integer, number, float, string, void, any"
                        .to_string(),
                ),
            ))
        }
    };

    // Parse size specifier like "8" or "8u"
    let size_str = size_spec.trim();
    let (bits_str, unsigned) = match size_str.strip_suffix('u') {
        Some(bits_str) => (bits_str, true),
        None => (size_str, false),
    };
    // Widths from foundation/spec/type-system.md §2 "Precision Modifiers".
    let bits = match (&base_type, bits_str.parse::<u8>()) {
        (Type::Integer, Ok(bits @ (8 | 16 | 32 | 64))) => bits,
        (Type::Number, Ok(bits @ (32 | 64))) if !unsigned => bits,
        (Type::Integer | Type::Number, _) => {
            return Err(CompilerError::parse_error(
                format!("Invalid precision '{}'", full_string),
                None,
                Some(
                    "integer supports :8, :16, :32 and :64 (optionally followed by u); number supports :32 and :64"
                        .to_string(),
                ),
            ))
        }
        _ => 0,
    };

    match base_type {
        Type::Integer => Ok(Type::IntegerSized { bits, unsigned }),
        Type::Number => Ok(Type::NumberSized { bits }),
        _ => Err(CompilerError::parse_error(
            "Size specifiers can only be used with integer and number types".to_string(),
            None,
            Some("Use size specifiers like :8, :16, :32, :64, or :8u for unsigned".to_string()),
        )),
    }
}
