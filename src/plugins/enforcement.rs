//! Plugin enforcement rules validation
//!
//! Checks plugin-defined project structure conventions during compilation:
//! - Restricted functions (raw bridge calls that should use DSL blocks instead)
//! - Required blocks (blocks that must exist in files under certain folders)
//! - Block folder rules (blocks that should only appear in specific folders)
//! - Permission enforcement (plugins may only call bridge functions they declared)

use crate::ast::{Expression, Program, Statement};
use crate::error::CompilerError;
use crate::plugins::plugin_abi::PluginEnforcement;
use crate::plugins::PluginRegistry;

/// Result of running enforcement checks
pub struct EnforcementResult {
    pub warnings: Vec<EnforcementDiagnostic>,
    pub errors: Vec<EnforcementDiagnostic>,
}

/// A single enforcement diagnostic (warning or error)
pub struct EnforcementDiagnostic {
    pub message: String,
    pub suggestion: String,
    pub file_path: String,
    pub line: Option<usize>,
    pub plugin: String,
}

/// Run all enforcement rules from loaded plugins against a parsed program
pub fn enforce_rules(
    program: &Program,
    file_path: &str,
    enforcement_rules: &[(String, PluginEnforcement)],
) -> EnforcementResult {
    let mut result = EnforcementResult {
        warnings: vec![],
        errors: vec![],
    };

    for (plugin_name, rules) in enforcement_rules {
        check_restricted_functions(program, file_path, plugin_name, rules, &mut result);
        check_required_blocks(program, file_path, plugin_name, rules, &mut result);
        check_block_folder_rules(program, file_path, plugin_name, rules, &mut result);
    }

    result
}

/// Check for calls to restricted functions in the AST
fn check_restricted_functions(
    program: &Program,
    file_path: &str,
    plugin_name: &str,
    rules: &PluginEnforcement,
    result: &mut EnforcementResult,
) {
    if rules.restricted_functions.is_empty() {
        return;
    }

    // Collect all function call names from the program
    let mut call_names = Vec::new();
    collect_call_names_from_statements(&program.statements, &mut call_names);
    for func in &program.functions {
        collect_call_names_from_statements(&func.body, &mut call_names);
    }
    if let Some(start) = &program.start_function {
        collect_call_names_from_statements(&start.body, &mut call_names);
    }
    for class in &program.classes {
        for method in &class.methods {
            collect_call_names_from_statements(&method.body, &mut call_names);
        }
    }

    for restricted in &rules.restricted_functions {
        if call_names.iter().any(|name| name == &restricted.name) {
            let diagnostic = EnforcementDiagnostic {
                message: if restricted.message.is_empty() {
                    format!(
                        "Direct call to '{}' is restricted by plugin '{}'",
                        restricted.name, plugin_name
                    )
                } else {
                    restricted.message.clone()
                },
                suggestion: format!("Use '{}' instead", restricted.use_instead),
                file_path: file_path.to_string(),
                line: None,
                plugin: plugin_name.to_string(),
            };
            if rules.severity == "error" {
                result.errors.push(diagnostic);
            } else {
                result.warnings.push(diagnostic);
            }
        }
    }
}

/// Check that required blocks exist when a file is in a specific folder
fn check_required_blocks(
    program: &Program,
    file_path: &str,
    plugin_name: &str,
    rules: &PluginEnforcement,
    result: &mut EnforcementResult,
) {
    for required in &rules.required_blocks {
        if file_path.contains(&required.folder) {
            let has_block = program.statements.iter().any(|stmt| {
                matches!(stmt, Statement::FrameworkBlock { name, .. } if name == &required.block)
            });
            if !has_block {
                let diagnostic = EnforcementDiagnostic {
                    message: if required.message.is_empty() {
                        format!(
                            "Files in '{}' should contain a '{}:' block (plugin: {})",
                            required.folder, required.block, plugin_name
                        )
                    } else {
                        required.message.clone()
                    },
                    suggestion: format!("Add a '{}:' block to this file", required.block),
                    file_path: file_path.to_string(),
                    line: None,
                    plugin: plugin_name.to_string(),
                };
                if rules.severity == "error" {
                    result.errors.push(diagnostic);
                } else {
                    result.warnings.push(diagnostic);
                }
            }
        }
    }
}

/// Check that blocks only appear in allowed folders
fn check_block_folder_rules(
    program: &Program,
    file_path: &str,
    plugin_name: &str,
    rules: &PluginEnforcement,
    result: &mut EnforcementResult,
) {
    for rule in &rules.block_folder_rules {
        let has_block = program.statements.iter().any(
            |stmt| matches!(stmt, Statement::FrameworkBlock { name, .. } if name == &rule.block),
        );
        if has_block {
            let in_allowed = rule.allowed_in.iter().any(|path| file_path.contains(path));
            if !in_allowed {
                let diagnostic = EnforcementDiagnostic {
                    message: if rule.message.is_empty() {
                        format!(
                            "'{}:' block should only appear in: {} (plugin: {})",
                            rule.block,
                            rule.allowed_in.join(", "),
                            plugin_name
                        )
                    } else {
                        rule.message.clone()
                    },
                    suggestion: format!("Move this file to one of: {}", rule.allowed_in.join(", ")),
                    file_path: file_path.to_string(),
                    line: None,
                    plugin: plugin_name.to_string(),
                };
                if rules.severity == "error" {
                    result.errors.push(diagnostic);
                } else {
                    result.warnings.push(diagnostic);
                }
            }
        }
    }
}

/// Recursively collect function call names from statements
fn collect_call_names_from_statements(stmts: &[Statement], names: &mut Vec<String>) {
    for stmt in stmts {
        collect_call_names_from_statement(stmt, names);
    }
}

/// Collect function call names from a single statement
fn collect_call_names_from_statement(stmt: &Statement, names: &mut Vec<String>) {
    match stmt {
        Statement::Expression { expr, .. } => collect_call_names_from_expr(expr, names),
        Statement::VariableDecl {
            initializer: Some(v),
            ..
        } => collect_call_names_from_expr(v, names),
        Statement::VariableDecl {
            initializer: None, ..
        } => {}
        Statement::Assignment { value, .. } => collect_call_names_from_expr(value, names),
        Statement::Return {
            value: Some(expr), ..
        } => collect_call_names_from_expr(expr, names),
        Statement::Print { expression, .. } => collect_call_names_from_expr(expression, names),
        Statement::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_call_names_from_expr(condition, names);
            collect_call_names_from_statements(then_branch, names);
            if let Some(eb) = else_branch {
                collect_call_names_from_statements(eb, names);
            }
        }
        Statement::While {
            condition, body, ..
        } => {
            collect_call_names_from_expr(condition, names);
            collect_call_names_from_statements(body, names);
        }
        Statement::Iterate {
            collection, body, ..
        } => {
            collect_call_names_from_expr(collection, names);
            collect_call_names_from_statements(body, names);
        }
        Statement::RangeIterate {
            start, end, body, ..
        } => {
            collect_call_names_from_expr(start, names);
            collect_call_names_from_expr(end, names);
            collect_call_names_from_statements(body, names);
        }
        _ => {}
    }
}

// ============================================================================
// Permission Enforcement
// ============================================================================

/// Validate that plugin-expanded code only calls bridge functions the plugin
/// has declared in its `[bridge]` section.
///
/// Bridge functions are identified by names that start with `_` (the conventional
/// prefix used throughout the Clean Language runtime bridge).
///
/// # Arguments
/// * `registry`             - The plugin registry that owns permission allowlists.
/// * `plugin_name`          - The name of the plugin whose expanded code is being checked.
/// * `expanded_statements`  - The AST statements produced by the plugin's expansion.
///
/// # Returns
/// A (possibly empty) list of `CompilerError::Validation` errors.  Callers should
/// collect these and emit them as compilation diagnostics rather than aborting
/// immediately, so that all violations are reported in one pass.
pub fn validate_plugin_permissions(
    registry: &PluginRegistry,
    plugin_name: &str,
    expanded_statements: &[Statement],
) -> Vec<CompilerError> {
    // Get the plugin's allowed bridge functions. If no manifest, the plugin has zero permissions.
    let empty = std::collections::HashSet::new();
    let allowed = registry
        .plugin_allowed_functions(plugin_name)
        .unwrap_or(&empty);

    let mut call_names: Vec<String> = Vec::new();
    collect_call_names_from_statements(expanded_statements, &mut call_names);

    let mut errors: Vec<CompilerError> = Vec::new();

    // Deduplicate before reporting so that a repeated call only produces one error.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for name in call_names {
        // Only check names that look like bridge functions (leading underscore).
        if !name.starts_with('_') {
            continue;
        }

        if !allowed.contains(&name) && seen.insert(name.clone()) {
            // Determine whether another plugin owns this function (cross-plugin call).
            let owner_hint = match registry.bridge_function_owner(&name) {
                Some(owner) if owner != plugin_name => format!(
                    " (function is declared by plugin '{}' — add it as a dependency or request cross-plugin access)",
                    owner
                ),
                _ => String::new(),
            };

            let message = format!(
                "Security: Plugin '{}' calls '{}' which is not in its [bridge] functions.{} \
                 Add it to {}/plugin.toml [bridge] section or remove the call.",
                plugin_name, name, owner_hint, plugin_name
            );

            errors.push(CompilerError::PluginError {
                message,
                location: None,
            });
        }
    }

    errors
}

/// Collect function call names from an expression
fn collect_call_names_from_expr(expr: &Expression, names: &mut Vec<String>) {
    match expr {
        Expression::Call(name, args) => {
            names.push(name.clone());
            for arg in args {
                collect_call_names_from_expr(arg, names);
            }
        }
        Expression::NamespaceCall {
            namespace,
            function,
            arguments,
            ..
        } => {
            names.push(format!("{}.{}", namespace, function));
            for arg in arguments {
                collect_call_names_from_expr(arg, names);
            }
        }
        Expression::MethodCall {
            object, arguments, ..
        } => {
            collect_call_names_from_expr(object, names);
            for arg in arguments {
                collect_call_names_from_expr(arg, names);
            }
        }
        Expression::Binary(left, _, right) => {
            collect_call_names_from_expr(left, names);
            collect_call_names_from_expr(right, names);
        }
        Expression::Unary(_, inner) => collect_call_names_from_expr(inner, names),
        Expression::OnError {
            expression,
            fallback,
            ..
        } => {
            collect_call_names_from_expr(expression, names);
            collect_call_names_from_expr(fallback, names);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::plugin_abi::{
        BlockFolderRule, PluginEnforcement, RequiredBlock, RestrictedFunction,
    };

    fn empty_program() -> Program {
        Program {
            imports: vec![],
            plugins: vec![],
            statements: vec![],
            functions: vec![],
            classes: vec![],
            start_function: None,
            tests: vec![],
            screens: vec![],
            state: None,
            watch_blocks: vec![],
            screen_blocks: vec![],
            externals: vec![],
            capabilities: vec![],
            source_block: None,
            location: None,
        }
    }

    #[test]
    fn test_no_rules_no_diagnostics() {
        let program = empty_program();
        let rules: Vec<(String, PluginEnforcement)> = vec![];
        let result = enforce_rules(&program, "test.cln", &rules);
        assert!(result.warnings.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_restricted_function_warning() {
        let mut program = empty_program();
        program.statements.push(Statement::Expression {
            expr: Expression::Call("_http_route".to_string(), vec![]),
            location: None,
        });

        let rules = vec![(
            "frame.httpserver".to_string(),
            PluginEnforcement {
                severity: "warn".to_string(),
                restricted_functions: vec![RestrictedFunction {
                    name: "_http_route".to_string(),
                    use_instead: "endpoints:".to_string(),
                    message: "Use endpoints: block".to_string(),
                }],
                required_blocks: vec![],
                block_folder_rules: vec![],
            },
        )];

        let result = enforce_rules(&program, "test.cln", &rules);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.errors.is_empty());
        assert_eq!(result.warnings[0].message, "Use endpoints: block");
    }

    #[test]
    fn test_restricted_function_error() {
        let mut program = empty_program();
        program.statements.push(Statement::Expression {
            expr: Expression::Call("_http_route".to_string(), vec![]),
            location: None,
        });

        let rules = vec![(
            "frame.httpserver".to_string(),
            PluginEnforcement {
                severity: "error".to_string(),
                restricted_functions: vec![RestrictedFunction {
                    name: "_http_route".to_string(),
                    use_instead: "endpoints:".to_string(),
                    message: "".to_string(),
                }],
                required_blocks: vec![],
                block_folder_rules: vec![],
            },
        )];

        let result = enforce_rules(&program, "test.cln", &rules);
        assert!(result.warnings.is_empty());
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_required_block_missing() {
        let program = empty_program();
        let rules = vec![(
            "frame.httpserver".to_string(),
            PluginEnforcement {
                severity: "warn".to_string(),
                restricted_functions: vec![],
                required_blocks: vec![RequiredBlock {
                    folder: "app/backend/api/".to_string(),
                    block: "endpoints".to_string(),
                    message: "API files should use endpoints: block".to_string(),
                }],
                block_folder_rules: vec![],
            },
        )];

        let result = enforce_rules(&program, "app/backend/api/users.cln", &rules);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(
            result.warnings[0].message,
            "API files should use endpoints: block"
        );
    }

    #[test]
    fn test_required_block_present() {
        let mut program = empty_program();
        program.statements.push(Statement::FrameworkBlock {
            name: "endpoints".to_string(),
            content: "GET /users".to_string(),
            attributes: vec![],
            location: None,
        });

        let rules = vec![(
            "frame.httpserver".to_string(),
            PluginEnforcement {
                severity: "warn".to_string(),
                restricted_functions: vec![],
                required_blocks: vec![RequiredBlock {
                    folder: "app/backend/api/".to_string(),
                    block: "endpoints".to_string(),
                    message: "".to_string(),
                }],
                block_folder_rules: vec![],
            },
        )];

        let result = enforce_rules(&program, "app/backend/api/users.cln", &rules);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_block_folder_rule_violation() {
        let mut program = empty_program();
        program.statements.push(Statement::FrameworkBlock {
            name: "endpoints".to_string(),
            content: "GET /users".to_string(),
            attributes: vec![],
            location: None,
        });

        let rules = vec![(
            "frame.httpserver".to_string(),
            PluginEnforcement {
                severity: "warn".to_string(),
                restricted_functions: vec![],
                required_blocks: vec![],
                block_folder_rules: vec![BlockFolderRule {
                    block: "endpoints".to_string(),
                    allowed_in: vec!["app/backend/".to_string()],
                    message: "endpoints: should be in app/backend/".to_string(),
                }],
            },
        )];

        let result = enforce_rules(&program, "app/frontend/page.cln", &rules);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn test_block_folder_rule_ok() {
        let mut program = empty_program();
        program.statements.push(Statement::FrameworkBlock {
            name: "endpoints".to_string(),
            content: "GET /users".to_string(),
            attributes: vec![],
            location: None,
        });

        let rules = vec![(
            "frame.httpserver".to_string(),
            PluginEnforcement {
                severity: "warn".to_string(),
                restricted_functions: vec![],
                required_blocks: vec![],
                block_folder_rules: vec![BlockFolderRule {
                    block: "endpoints".to_string(),
                    allowed_in: vec!["app/backend/".to_string()],
                    message: "".to_string(),
                }],
            },
        )];

        let result = enforce_rules(&program, "app/backend/api/users.cln", &rules);
        assert!(result.warnings.is_empty());
    }
}
