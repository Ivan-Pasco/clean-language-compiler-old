//! Plugin enforcement rules validation
//!
//! Checks plugin-defined project structure conventions during compilation:
//! - Restricted functions (raw bridge calls that should use DSL blocks instead)
//! - Required blocks (blocks that must exist in files under certain folders)
//! - Block folder rules (blocks that should only appear in specific folders)
//! - Permission enforcement (plugins may only call bridge functions they declared)

use crate::ast::{
    AssignmentTarget, Capability, Class, Expression, ExternalFunction, Function, HttpTestAssertion,
    Parameter, Program, StateBlock, Statement, StringPart, TestCase, TestCaseKind, ValidateBlock,
    ValidateConstraint,
};
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

    let mut call_names = Vec::new();
    collect_call_names_from_program(program, &mut call_names);

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

// ============================================================================
// Call-name collection
// ============================================================================
//
// Every walker below matches its node type exhaustively, without a catch-all
// arm: a new AST variant that can contain a call is a compile error here
// until it is walked, so no call can hide from the permission check or the
// restricted-function rules.

/// Every function name called (or declared as a WASM import) anywhere in
/// `program`.
fn collect_call_names_from_program(program: &Program, names: &mut Vec<String>) {
    let Program {
        imports: _,
        plugins: _,
        statements,
        functions,
        classes,
        capabilities,
        start_function,
        tests,
        state,
        watch_blocks,
        screen_blocks,
        externals,
        source_block,
        location: _,
    } = program;
    collect_call_names_from_statements(statements, names);
    for func in functions {
        collect_call_names_from_function(func, names);
    }
    for class in classes {
        collect_call_names_from_class(class, names);
    }
    for capability in capabilities {
        collect_call_names_from_capability(capability, names);
    }
    if let Some(start) = start_function {
        collect_call_names_from_function(start, names);
    }
    for test in tests {
        collect_call_names_from_test_case(test, names);
    }
    if let Some(state) = state {
        collect_call_names_from_state(state, names);
    }
    for watch in watch_blocks {
        collect_call_names_from_statements(&watch.body, names);
    }
    collect_call_names_from_statements(screen_blocks, names);
    for external in externals {
        collect_call_names_from_external(external, names);
    }
    if let Some(source_block) = source_block {
        collect_call_names_from_statement(source_block, names);
    }
}

/// Every function name called (or declared as a WASM import) anywhere in a
/// plugin expansion: inline statements, the start function, functions,
/// classes, capabilities, externals and the `state:` block.
fn collect_call_names_from_expansion(
    expansion: &crate::plugins::PluginExpansion,
    names: &mut Vec<String>,
) {
    let crate::plugins::PluginExpansion {
        statements,
        start_function,
        functions,
        classes,
        capabilities,
        externals,
        state,
    } = expansion;
    collect_call_names_from_statements(statements, names);
    if let Some(start) = start_function {
        collect_call_names_from_function(start, names);
    }
    for func in functions {
        collect_call_names_from_function(func, names);
    }
    for class in classes {
        collect_call_names_from_class(class, names);
    }
    for capability in capabilities {
        collect_call_names_from_capability(capability, names);
    }
    for external in externals {
        collect_call_names_from_external(external, names);
    }
    if let Some(state) = state {
        collect_call_names_from_state(state, names);
    }
}

/// An `external:` declaration imports `name` from the host directly, which
/// grants the same access as calling it.
fn collect_call_names_from_external(external: &ExternalFunction, names: &mut Vec<String>) {
    names.push(external.name.clone());
    collect_call_names_from_parameters(&external.parameters, names);
}

fn collect_call_names_from_parameters(parameters: &[Parameter], names: &mut Vec<String>) {
    for parameter in parameters {
        if let Some(default) = &parameter.default_value {
            collect_call_names_from_expr(default, names);
        }
    }
}

fn collect_call_names_from_function(func: &Function, names: &mut Vec<String>) {
    collect_call_names_from_parameters(&func.parameters, names);
    collect_call_names_from_statements(&func.body, names);
}

fn collect_call_names_from_class(class: &Class, names: &mut Vec<String>) {
    for field in &class.fields {
        if let Some(default) = &field.default_value {
            collect_call_names_from_expr(default, names);
        }
    }
    for method in &class.methods {
        collect_call_names_from_function(method, names);
    }
    if let Some(constructor) = &class.constructor {
        collect_call_names_from_parameters(&constructor.parameters, names);
        collect_call_names_from_statements(&constructor.body, names);
    }
    for invariant in &class.invariants {
        collect_call_names_from_expr(invariant, names);
    }
}

fn collect_call_names_from_capability(capability: &Capability, names: &mut Vec<String>) {
    for method in &capability.methods {
        collect_call_names_from_parameters(&method.parameters, names);
        if let Some(body) = &method.default_body {
            collect_call_names_from_statements(body, names);
        }
    }
}

fn collect_call_names_from_state(state: &StateBlock, names: &mut Vec<String>) {
    for declaration in &state.declarations {
        collect_call_names_from_expr(&declaration.initializer, names);
        if let Some(guard) = &declaration.guard {
            collect_call_names_from_expr(&guard.condition, names);
        }
    }
    for computed in &state.computed {
        collect_call_names_from_statements(&computed.body, names);
    }
    if let Some(rules) = &state.rules {
        for rule in &rules.rules {
            collect_call_names_from_expr(rule, names);
        }
    }
}

fn collect_call_names_from_test_case(test: &TestCase, names: &mut Vec<String>) {
    match &test.kind {
        TestCaseKind::Expression {
            test_expression,
            expected_value,
        } => {
            collect_call_names_from_expr(test_expression, names);
            collect_call_names_from_expr(expected_value, names);
        }
        TestCaseKind::Endpoint(endpoint) => {
            if let Some(body) = &endpoint.request.body {
                for (_, value) in body {
                    collect_call_names_from_expr(value, names);
                }
            }
            for assertion in &endpoint.assertions {
                match assertion {
                    HttpTestAssertion::JsonField { value, .. } => {
                        collect_call_names_from_expr(value, names)
                    }
                    HttpTestAssertion::Status { .. }
                    | HttpTestAssertion::JsonFieldNotNull { .. } => {}
                }
            }
        }
    }
}

fn collect_call_names_from_validate_schema(schema: &ValidateBlock, names: &mut Vec<String>) {
    for field in &schema.fields {
        for constraint in &field.constraints {
            match constraint {
                ValidateConstraint::Length { min, max } => {
                    collect_call_names_from_expr(min, names);
                    collect_call_names_from_expr(max, names);
                }
                ValidateConstraint::Min(e) | ValidateConstraint::Max(e) => {
                    collect_call_names_from_expr(e, names)
                }
                ValidateConstraint::OneOf(values) => {
                    for value in values {
                        collect_call_names_from_expr(value, names);
                    }
                }
                // `custom: fn` names a validator function the schema calls.
                ValidateConstraint::Custom(function) => names.push(function.clone()),
                ValidateConstraint::Required
                | ValidateConstraint::Trim
                | ValidateConstraint::Match(_) => {}
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
        Statement::Require { condition, .. } | Statement::Ensure { condition, .. } => {
            collect_call_names_from_expr(condition, names)
        }
        Statement::VariableDecl { initializer, .. } => {
            if let Some(v) = initializer {
                collect_call_names_from_expr(v, names);
            }
        }
        Statement::FunctionsBlock { functions, .. } => {
            for func in functions {
                collect_call_names_from_function(func, names);
            }
        }
        Statement::TypeApplyBlock { assignments, .. } => {
            for assignment in assignments {
                if let Some(v) = &assignment.initializer {
                    collect_call_names_from_expr(v, names);
                }
            }
        }
        // `name:` with one argument per line calls `name` once per line.
        Statement::FunctionApplyBlock {
            function_name,
            expressions,
            ..
        } => {
            names.push(function_name.clone());
            for e in expressions {
                collect_call_names_from_expr(e, names);
            }
        }
        Statement::MethodApplyBlock { expressions, .. }
        | Statement::PrintBlock { expressions, .. } => {
            for e in expressions {
                collect_call_names_from_expr(e, names);
            }
        }
        Statement::ConstantApplyBlock { constants, .. } => {
            for constant in constants {
                collect_call_names_from_expr(&constant.value, names);
            }
        }
        Statement::Assignment { target, value, .. } => {
            match target {
                AssignmentTarget::Index { index, .. } => collect_call_names_from_expr(index, names),
                AssignmentTarget::Variable(_) | AssignmentTarget::Property { .. } => {}
            }
            collect_call_names_from_expr(value, names);
        }
        Statement::Print { expression, .. }
        | Statement::Error {
            message: expression,
            ..
        }
        | Statement::LaterAssignment { expression, .. }
        | Statement::Background { expression, .. }
        | Statement::BuildBlock {
            rules_enabled: expression,
            ..
        } => collect_call_names_from_expr(expression, names),
        Statement::Return { value, .. } => {
            if let Some(v) = value {
                collect_call_names_from_expr(v, names);
            }
        }
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
            start,
            end,
            step,
            body,
            ..
        } => {
            collect_call_names_from_expr(start, names);
            collect_call_names_from_expr(end, names);
            if let Some(step) = step {
                collect_call_names_from_expr(step, names);
            }
            collect_call_names_from_statements(body, names);
        }
        Statement::Test { body, .. } | Statement::StandaloneErrorHandler { body, .. } => {
            collect_call_names_from_statements(body, names)
        }
        Statement::Expression { expr, .. } => collect_call_names_from_expr(expr, names),
        Statement::OnErrorBlock {
            expression,
            error_block,
            ..
        } => {
            collect_call_names_from_expr(expression, names);
            collect_call_names_from_statements(error_block, names);
        }
        Statement::ClassDefinition { class, .. } => collect_call_names_from_class(class, names),
        Statement::ScreenBlockStmt {
            state,
            watch_blocks,
            functions,
            ..
        } => {
            if let Some(state) = state {
                collect_call_names_from_state(state, names);
            }
            for watch in watch_blocks {
                collect_call_names_from_statements(&watch.body, names);
            }
            for func in functions {
                collect_call_names_from_function(func, names);
            }
        }
        Statement::ValidateDeclaration { schema, .. } => {
            collect_call_names_from_validate_schema(schema, names)
        }
        Statement::ValidateCheck { check, .. } => {
            collect_call_names_from_expr(&check.input, names);
            collect_call_names_from_statements(&check.ok_branch, names);
            collect_call_names_from_statements(&check.error_branch, names);
        }
        // A nested framework block is raw DSL text; whatever it expands to is
        // checked when its own plugin expands it.
        Statement::FrameworkBlock { .. }
        | Statement::Import { .. }
        | Statement::Break { .. }
        | Statement::Continue { .. }
        | Statement::Description { .. }
        | Statement::Spec { .. }
        | Statement::Intent { .. }
        | Statement::SourceBlock { .. }
        | Statement::ResetStmt { .. } => {}
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
/// * `expansion`            - Everything the plugin emitted: inline statements, the
///   start function, functions, classes, capabilities, externals and state.
/// * `user_source`          - The DSL text the user wrote for the block that was
///   expanded, when there is one. Plugins pass user-authored code through
///   (handler bodies, `_emit_stmt_from_source`), and the user may call any
///   loaded bridge; a bridge whose name the user wrote in the block is the
///   user's call, not the plugin's. Lifecycle slots and helper output have no
///   user text and are checked in full.
///
/// Only calls to *bridge functions* are checked: names some plugin declares
/// in its `[bridge]` section or that the canonical function registry
/// defines. Other underscore-prefixed names are ordinary functions — e.g.
/// the `__Model_count` helpers a plugin emits for an ORM model, possibly in
/// a different module of the build — and name resolution reports them if
/// they do not exist.
///
/// # Returns
/// A (possibly empty) list of `CompilerError::Validation` errors.  Callers should
/// collect these and emit them as compilation diagnostics rather than aborting
/// immediately, so that all violations are reported in one pass.
pub fn validate_plugin_permissions(
    registry: &PluginRegistry,
    plugin_name: &str,
    expansion: &crate::plugins::PluginExpansion,
    user_source: Option<&str>,
) -> Vec<CompilerError> {
    // Get the plugin's allowed bridge functions. If no manifest, the plugin has zero permissions.
    let empty = std::collections::HashSet::new();
    let allowed = registry
        .plugin_allowed_functions(plugin_name)
        .unwrap_or(&empty);

    let mut call_names: Vec<String> = Vec::new();
    collect_call_names_from_expansion(expansion, &mut call_names);

    let mut errors: Vec<CompilerError> = Vec::new();

    // Deduplicate before reporting so that a repeated call only produces one error.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for name in call_names {
        // Only check names that look like bridge functions (leading underscore).
        if !name.starts_with('_') || !is_bridge_function(registry, &name) {
            continue;
        }
        if user_source.is_some_and(|text| contains_identifier(text, &name)) {
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

/// True when `ident` occurs in `text` as a whole identifier (not as part of
/// a longer one).
fn contains_identifier(text: &str, ident: &str) -> bool {
    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = text.as_bytes();
    text.match_indices(ident).any(|(start, _)| {
        let end = start + ident.len();
        (start == 0 || !is_ident_byte(bytes[start - 1]))
            && (end == bytes.len() || !is_ident_byte(bytes[end]))
    })
}

/// True when `name` is a bridge function: declared by some loaded plugin's
/// `[bridge]` section or defined by the canonical function registry.
fn is_bridge_function(registry: &PluginRegistry, name: &str) -> bool {
    static CANONICAL: std::sync::OnceLock<Option<crate::plugins::registry_loader::RegistryIndex>> =
        std::sync::OnceLock::new();
    registry.bridge_function_owner(name).is_some()
        || CANONICAL
            .get_or_init(|| crate::plugins::registry_loader::RegistryIndex::load().ok())
            .as_ref()
            .is_some_and(|idx| idx.lookup(name).is_some())
}

/// Collect function call names from an expression
fn collect_call_names_from_expr(expr: &Expression, names: &mut Vec<String>) {
    match expr {
        Expression::Call(name, args) => {
            names.push(name.clone());
            collect_call_names_from_exprs(args, names);
        }
        Expression::NamespaceCall {
            namespace,
            function,
            arguments,
            ..
        } => {
            names.push(format!("{}.{}", namespace, function));
            collect_call_names_from_exprs(arguments, names);
        }
        Expression::MethodCall {
            object, arguments, ..
        } => {
            collect_call_names_from_expr(object, names);
            collect_call_names_from_exprs(arguments, names);
        }
        Expression::StaticMethodCall { arguments, .. }
        | Expression::ObjectCreation { arguments, .. }
        | Expression::BaseCall { arguments, .. } => collect_call_names_from_exprs(arguments, names),
        Expression::ListLiteral(elements) => collect_call_names_from_exprs(elements, names),
        Expression::Binary(left, _, right) | Expression::ListAccess(left, right) => {
            collect_call_names_from_expr(left, names);
            collect_call_names_from_expr(right, names);
        }
        Expression::Unary(_, inner) => collect_call_names_from_expr(inner, names),
        Expression::PropertyAccess { object, .. } => collect_call_names_from_expr(object, names),
        Expression::StringInterpolation(parts) => {
            for part in parts {
                match part {
                    StringPart::Interpolation(e) => collect_call_names_from_expr(e, names),
                    StringPart::Text(_) => {}
                }
            }
        }
        Expression::OnError {
            expression,
            fallback,
            ..
        } => {
            collect_call_names_from_expr(expression, names);
            collect_call_names_from_expr(fallback, names);
        }
        Expression::OnErrorBlock {
            expression,
            error_handler,
            ..
        } => {
            collect_call_names_from_expr(expression, names);
            collect_call_names_from_statements(error_handler, names);
        }
        Expression::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            collect_call_names_from_expr(condition, names);
            collect_call_names_from_expr(then_expr, names);
            collect_call_names_from_expr(else_expr, names);
        }
        Expression::StartExpression { expression, .. } => {
            collect_call_names_from_expr(expression, names)
        }
        Expression::NamedArgBinding { value, .. } => collect_call_names_from_expr(value, names),
        Expression::ObjectLiteral { fields, .. } => {
            for (_, value) in fields {
                collect_call_names_from_expr(value, names);
            }
        }
        // Raw `Model.verb:` query text; its expansion is checked when the
        // owning plugin expands it.
        Expression::OrmQuery { .. }
        | Expression::Literal(_)
        | Expression::Variable(_)
        | Expression::ErrorVariable { .. } => {}
    }
}

fn collect_call_names_from_exprs(exprs: &[Expression], names: &mut Vec<String>) {
    for e in exprs {
        collect_call_names_from_expr(e, names);
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

    // ------------------------------------------------------------------
    // Permission enforcement covers everything a plugin emits
    // ------------------------------------------------------------------

    fn manifest_declaring(name: &str, bridges: &[&str]) -> crate::plugins::PluginManifest {
        let functions = bridges
            .iter()
            .map(|b| format!("{{ name = \"{}\", params = [], returns = \"string\" }}", b))
            .collect::<Vec<_>>()
            .join(", ");
        toml::from_str(&format!(
            "[plugin]\nname = \"{name}\"\nversion = \"1.0.0\"\n[handles]\nblocks = []\n\
             [bridge]\nfunctions = [{functions}]\n"
        ))
        .unwrap()
    }

    /// `owner` declares `_req_ip`; `intruder` declares nothing.
    fn registry_with_owner() -> PluginRegistry {
        PluginRegistry::builder()
            .add_manifest(
                "owner".to_string(),
                manifest_declaring("owner", &["_req_ip"]),
            )
            .add_manifest("intruder".to_string(), manifest_declaring("intruder", &[]))
            .build()
            .unwrap()
    }

    fn secret_call() -> Expression {
        Expression::Call("_req_ip".to_string(), vec![])
    }

    fn loc() -> crate::ast::SourceLocation {
        crate::ast::SourceLocation::new(0, 0, "test")
    }

    fn expr_stmt(expr: Expression) -> Statement {
        Statement::Expression {
            expr,
            location: None,
        }
    }

    fn func_with_body(name: &str, body: Vec<Statement>) -> crate::ast::Function {
        crate::ast::Function::new(name.to_string(), vec![], crate::ast::Type::Void, body, None)
    }

    fn violations_for(expansion: &crate::plugins::PluginExpansion) -> usize {
        validate_plugin_permissions(&registry_with_owner(), "intruder", expansion, None).len()
    }

    /// Calls hidden in every kind of emitted item and in every nested
    /// expression shape are reported.
    #[test]
    fn permission_check_sees_calls_everywhere_in_an_expansion() {
        use crate::ast::{Class, ExternalFunction, StateBlock, StateDeclaration, StateScope, Type};
        use crate::plugins::PluginExpansion;

        let nested_shapes: Vec<Expression> = vec![
            Expression::ListLiteral(vec![secret_call()]),
            Expression::StringInterpolation(vec![StringPart::Interpolation(secret_call())]),
            Expression::OnError {
                expression: Box::new(Expression::Variable("x".to_string())),
                fallback: Box::new(secret_call()),
                location: loc(),
            },
            Expression::PropertyAccess {
                object: Box::new(Expression::PropertyAccess {
                    object: Box::new(secret_call()),
                    property: "a".to_string(),
                    location: loc(),
                }),
                property: "b".to_string(),
                location: loc(),
            },
            Expression::Conditional {
                condition: Box::new(Expression::Literal(crate::ast::Value::Boolean(true))),
                then_expr: Box::new(Expression::Literal(crate::ast::Value::Integer(1))),
                else_expr: Box::new(secret_call()),
                location: loc(),
            },
            Expression::ObjectLiteral {
                fields: vec![(crate::ast::Value::String("k".to_string()), secret_call())],
                location: loc(),
            },
            Expression::ListAccess(
                Box::new(Expression::Variable("xs".to_string())),
                Box::new(secret_call()),
            ),
        ];
        for shape in nested_shapes {
            let expansion = PluginExpansion {
                statements: vec![expr_stmt(shape.clone())],
                ..Default::default()
            };
            assert_eq!(violations_for(&expansion), 1, "missed call in {:?}", shape);
        }

        let in_function = PluginExpansion {
            functions: vec![func_with_body("helper", vec![expr_stmt(secret_call())])],
            ..Default::default()
        };
        assert_eq!(violations_for(&in_function), 1, "function body");

        let in_start = PluginExpansion {
            start_function: Some(func_with_body("start", vec![expr_stmt(secret_call())])),
            ..Default::default()
        };
        assert_eq!(violations_for(&in_start), 1, "start function");

        let mut class = Class::new("Model".to_string(), None);
        class
            .methods
            .push(func_with_body("save", vec![expr_stmt(secret_call())]));
        let in_class = PluginExpansion {
            classes: vec![class],
            ..Default::default()
        };
        assert_eq!(violations_for(&in_class), 1, "class method");

        let in_external = PluginExpansion {
            externals: vec![ExternalFunction::new(
                "_req_ip".to_string(),
                vec![],
                Type::Integer,
                None,
            )],
            ..Default::default()
        };
        assert_eq!(violations_for(&in_external), 1, "external import");

        let mut state = StateBlock::new(StateScope::App, None);
        state.declarations.push(StateDeclaration::new(
            "s".to_string(),
            Type::Integer,
            secret_call(),
            None,
        ));
        let in_state = PluginExpansion {
            state: Some(state),
            ..Default::default()
        };
        assert_eq!(violations_for(&in_state), 1, "state initializer");
    }

    /// A plugin calling its own declared bridge is not a violation.
    #[test]
    fn permission_check_allows_declared_bridges() {
        let expansion = crate::plugins::PluginExpansion {
            functions: vec![func_with_body("helper", vec![expr_stmt(secret_call())])],
            ..Default::default()
        };
        assert!(
            validate_plugin_permissions(&registry_with_owner(), "owner", &expansion, None)
                .is_empty()
        );
    }

    /// A bridge call the user wrote inside the block (a handler body the
    /// plugin passes through) is the user's call; the same call with no
    /// user text behind it is the plugin's and is reported.
    #[test]
    fn user_written_bridge_calls_in_block_bodies_are_not_plugin_violations() {
        let expansion = crate::plugins::PluginExpansion {
            functions: vec![func_with_body("handler", vec![expr_stmt(secret_call())])],
            ..Default::default()
        };
        let registry = registry_with_owner();
        let user_text = "GET /page:\n\tstring body = _req_ip()\n";
        assert!(
            validate_plugin_permissions(&registry, "intruder", &expansion, Some(user_text))
                .is_empty()
        );
        // A longer identifier is not the same name.
        assert_eq!(
            validate_plugin_permissions(&registry, "intruder", &expansion, Some("x = _req_ip2()"))
                .len(),
            1
        );
        assert_eq!(
            validate_plugin_permissions(&registry, "intruder", &expansion, None).len(),
            1
        );
    }
}
