/*!
 * Clean Frame Web Plugin - HTTP Endpoints DSL
 *
 * This plugin transforms the `endpoints:` DSL block into standard Clean
 * Language code that registers HTTP routes with the framework router.
 *
 * ## DSL Syntax
 *
 * ```clean
 * endpoints:
 *     GET "/users" -> listUsers
 *     POST "/users" -> createUser
 *     GET "/users/{id}" -> getUser
 *     PUT "/users/{id}" -> updateUser
 *     DELETE "/users/{id}" -> deleteUser
 * ```
 *
 * ## Expansion
 *
 * The above expands to:
 *
 * ```clean
 * functions:
 *     void __frame_register_routes()
 *         router.get("/users", listUsers)
 *         router.post("/users", createUser)
 *         router.get("/users/{id}", getUser)
 *         router.put("/users/{id}", updateUser)
 *         router.delete("/users/{id}", deleteUser)
 * ```
 */

use crate::ast::{
    Expression, Function, FunctionModifier, FunctionSyntax, SourceLocation, Statement, Type, Value,
    Visibility,
};
use crate::plugins::{FrameworkBlock, FrameworkPlugin, PluginError, PluginResult};

/// HTTP method enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(HttpMethod::Get),
            "POST" => Some(HttpMethod::Post),
            "PUT" => Some(HttpMethod::Put),
            "PATCH" => Some(HttpMethod::Patch),
            "DELETE" => Some(HttpMethod::Delete),
            "HEAD" => Some(HttpMethod::Head),
            "OPTIONS" => Some(HttpMethod::Options),
            _ => None,
        }
    }

    fn to_router_method(&self) -> &'static str {
        match self {
            HttpMethod::Get => "get",
            HttpMethod::Post => "post",
            HttpMethod::Put => "put",
            HttpMethod::Patch => "patch",
            HttpMethod::Delete => "delete",
            HttpMethod::Head => "head",
            HttpMethod::Options => "options",
        }
    }
}

/// Parsed endpoint definition
#[derive(Debug, Clone)]
pub struct EndpointDef {
    pub method: HttpMethod,
    pub path: String,
    pub handler: String,
    pub line: usize,
}

/// Web plugin for Clean Frame
pub struct WebPlugin;

impl WebPlugin {
    pub fn new() -> Self {
        Self
    }

    /// Parse the endpoints block content
    fn parse_endpoints(
        &self,
        content: &str,
        base_location: &Option<SourceLocation>,
    ) -> PluginResult<Vec<EndpointDef>> {
        let mut endpoints = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            // Parse: METHOD "path" -> handler
            let endpoint = self.parse_endpoint_line(trimmed, line_num + 1, base_location)?;
            endpoints.push(endpoint);
        }

        Ok(endpoints)
    }

    /// Parse a single endpoint line
    fn parse_endpoint_line(
        &self,
        line: &str,
        line_num: usize,
        base_location: &Option<SourceLocation>,
    ) -> PluginResult<EndpointDef> {
        // Expected format: METHOD "path" -> handler
        // Or: METHOD "/path" -> handler (without quotes for simple paths)

        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return Err(PluginError::ParseError {
                message: format!("Invalid endpoint syntax: '{}'", line),
                line: line_num,
                column: 1,
                location: base_location.clone(),
            });
        }

        let method_str = parts[0];
        let rest = parts[1].trim();

        // Parse HTTP method
        let method = HttpMethod::from_str(method_str).ok_or_else(|| PluginError::ParseError {
            message: format!("Unknown HTTP method: '{}'", method_str),
            line: line_num,
            column: 1,
            location: base_location.clone(),
        })?;

        // Parse path and handler: "path" -> handler
        let arrow_pos = rest.find("->").ok_or_else(|| PluginError::ParseError {
            message: format!("Missing '->' in endpoint definition: '{}'", line),
            line: line_num,
            column: method_str.len() + 2,
            location: base_location.clone(),
        })?;

        let path_part = rest[..arrow_pos].trim();
        let handler = rest[arrow_pos + 2..].trim().to_string();

        // Extract path (remove quotes if present)
        let path = if path_part.starts_with('"') && path_part.ends_with('"') {
            path_part[1..path_part.len() - 1].to_string()
        } else if path_part.starts_with('/') {
            path_part.to_string()
        } else {
            return Err(PluginError::ParseError {
                message: format!("Path must start with '/' or be quoted: '{}'", path_part),
                line: line_num,
                column: method_str.len() + 2,
                location: base_location.clone(),
            });
        };

        if handler.is_empty() {
            return Err(PluginError::ParseError {
                message: "Handler function name is required".to_string(),
                line: line_num,
                column: arrow_pos + method_str.len() + 4,
                location: base_location.clone(),
            });
        }

        Ok(EndpointDef {
            method,
            path,
            handler,
            line: line_num,
        })
    }

    /// Generate router registration statements
    fn generate_router_calls(
        &self,
        endpoints: &[EndpointDef],
        location: &Option<SourceLocation>,
    ) -> Vec<Statement> {
        endpoints
            .iter()
            .map(|ep| {
                // Generate: router.METHOD(path, handler)
                Statement::Expression {
                    expr: Expression::MethodCall {
                        object: Box::new(Expression::Variable("router".to_string())),
                        method: ep.method.to_router_method().to_string(),
                        arguments: vec![
                            Expression::Literal(Value::String(ep.path.clone())),
                            Expression::Variable(ep.handler.clone()),
                        ],
                        location: location.clone().unwrap_or_default(),
                    },
                    location: location.clone(),
                }
            })
            .collect()
    }

    /// Generate the __frame_register_routes function
    fn generate_registration_function(
        &self,
        endpoints: &[EndpointDef],
        location: &Option<SourceLocation>,
    ) -> Function {
        let body = self.generate_router_calls(endpoints, location);

        Function {
            name: "__frame_register_routes".to_string(),
            type_parameters: vec![],
            type_constraints: vec![],
            parameters: vec![],
            return_type: Type::Void,
            body,
            description: Some(
                "Auto-generated route registration from endpoints: block".to_string(),
            ),
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Private,
            modifier: FunctionModifier::None,
            location: location.clone(),
        }
    }
}

impl Default for WebPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameworkPlugin for WebPlugin {
    fn name(&self) -> &'static str {
        "frame.web"
    }

    fn handles(&self) -> &'static [&'static str] {
        &["endpoints"]
    }

    fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        // Parse the endpoints DSL
        let endpoints = self.parse_endpoints(&block.content, &block.location)?;

        if endpoints.is_empty() {
            // Empty block - generate nothing
            return Ok(vec![]);
        }

        // Generate the registration function
        let func = self.generate_registration_function(&endpoints, &block.location);

        // Wrap in a FunctionsBlock statement
        Ok(vec![Statement::FunctionsBlock {
            functions: vec![func],
            location: block.location.clone(),
        }])
    }

    fn validate(&self, block: &FrameworkBlock) -> PluginResult<()> {
        // Basic validation - just check it's not malformed
        for (line_num, line) in block.content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            // Check for basic structure
            if !trimmed.contains("->") {
                return Err(PluginError::ValidationFailed {
                    plugin_name: self.name().to_string(),
                    message: format!("Line {} missing '->' operator: '{}'", line_num + 1, trimmed),
                    location: block.location.clone(),
                });
            }
        }

        Ok(())
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_endpoint() {
        let plugin = WebPlugin::new();
        let content = r#"GET "/users" -> listUsers"#;

        let endpoints = plugin.parse_endpoints(content, &None).unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].method, HttpMethod::Get);
        assert_eq!(endpoints[0].path, "/users");
        assert_eq!(endpoints[0].handler, "listUsers");
    }

    #[test]
    fn test_parse_multiple_endpoints() {
        let plugin = WebPlugin::new();
        let content = r#"
            GET "/users" -> listUsers
            POST "/users" -> createUser
            DELETE "/users/{id}" -> deleteUser
        "#;

        let endpoints = plugin.parse_endpoints(content, &None).unwrap();
        assert_eq!(endpoints.len(), 3);
    }

    #[test]
    fn test_parse_unquoted_path() {
        let plugin = WebPlugin::new();
        let content = r#"GET /api/v1/users -> getUsers"#;

        let endpoints = plugin.parse_endpoints(content, &None).unwrap();
        assert_eq!(endpoints[0].path, "/api/v1/users");
    }

    #[test]
    fn test_expand_endpoints() {
        let plugin = WebPlugin::new();
        let block = FrameworkBlock {
            name: "endpoints".to_string(),
            content: r#"GET "/test" -> testHandler"#.to_string(),
            attributes: vec![],
            location: None,
        };

        let expanded = plugin.expand(&block).unwrap();
        assert_eq!(expanded.len(), 1);
        assert!(matches!(expanded[0], Statement::FunctionsBlock { .. }));
    }

    #[test]
    fn test_invalid_method() {
        let plugin = WebPlugin::new();
        let content = r#"INVALID "/users" -> handler"#;

        let result = plugin.parse_endpoints(content, &None);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_arrow() {
        let plugin = WebPlugin::new();
        let block = FrameworkBlock {
            name: "endpoints".to_string(),
            content: r#"GET "/users" handler"#.to_string(),
            attributes: vec![],
            location: None,
        };

        let result = plugin.validate(&block);
        assert!(result.is_err());
    }
}
