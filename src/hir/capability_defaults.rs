//! Capability default bodies (15-classes-and-objects.md §Capabilities).
//!
//! Both run on the whole program, after every module's HIR is merged, so a
//! class may claim a capability declared in another module.

use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::*;

/// SEM014: a capability's default body may name members of the claiming
/// class only to call the capability's own methods. Any other bare name
/// that is not a parameter, a local or a top-level name (`is_top_level`)
/// would be a field or method of the claiming class, which is not
/// guaranteed to exist on every class that claims the capability.
pub fn check_defaults(
    capabilities: &[HirCapability],
    is_top_level: &dyn Fn(&str) -> bool,
) -> Result<(), CompilerError> {
    for cap in capabilities {
        let declared: std::collections::HashSet<&str> =
            cap.methods.iter().map(|m| m.name.as_str()).collect();
        for method in &cap.methods {
            let Some(body) = &method.default_body else {
                continue;
            };
            let mut locals: std::collections::HashSet<String> =
                method.parameters.iter().map(|p| p.name.clone()).collect();
            collect_locals(body, &mut locals);
            let mut used = Vec::new();
            collect_bare_names_in_block(body, &mut used);
            if let Some((name, location)) = used.into_iter().find(|(name, _)| {
                !declared.contains(name.as_str()) && !locals.contains(name) && !is_top_level(name)
            }) {
                return Err(CompilerError::semantic_error_with_code(
                    format!(
                        "The default body of '{}.{}' uses '{name}', but '{name}' is not \
                         a method of capability '{}'",
                        cap.name, method.name, cap.name
                    ),
                    Some(format!(
                        "A default body can only call methods declared in '{}'",
                        cap.name
                    )),
                    Some(location),
                    "SEM014",
                ));
            }
        }
    }
    Ok(())
}

/// Every name `block` declares (variables, loop variables, deferred values).
fn collect_locals(block: &HirBlock, out: &mut std::collections::HashSet<String>) {
    for statement in &block.statements {
        match statement {
            HirStatement::VariableDeclaration { name, .. } => {
                out.insert(name.clone());
            }
            HirStatement::LaterAssignment { variable, .. } => {
                out.insert(variable.clone());
            }
            HirStatement::For { variable, body, .. } => {
                out.insert(variable.clone());
                collect_locals(body, out);
            }
            HirStatement::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_locals(then_branch, out);
                if let Some(else_branch) = else_branch {
                    collect_locals(else_branch, out);
                }
            }
            HirStatement::While { body, .. } => collect_locals(body, out),
            _ => {}
        }
    }
}

/// Every bare name (variable read or write, call of an unqualified
/// function) in `block`, with where it appears.
fn collect_bare_names_in_block(block: &HirBlock, out: &mut Vec<(String, SourceLocation)>) {
    for statement in &block.statements {
        match statement {
            HirStatement::VariableDeclaration { initializer, .. } => {
                if let Some(init) = initializer {
                    collect_bare_names(init, out);
                }
            }
            HirStatement::Assignment {
                target,
                value,
                location,
            } => {
                match target {
                    HirLValue::Variable { name, .. } => out.push((name.clone(), location.clone())),
                    HirLValue::FieldAccess { object, .. } => collect_bare_names(object, out),
                    HirLValue::Index { array, index, .. } => {
                        collect_bare_names(array, out);
                        collect_bare_names(index, out);
                    }
                }
                collect_bare_names(value, out);
            }
            HirStatement::Expression { expression, .. }
            | HirStatement::Print { expression, .. }
            | HirStatement::LaterAssignment { expression, .. }
            | HirStatement::Background { expression, .. }
            | HirStatement::Require {
                condition: expression,
                ..
            }
            | HirStatement::Ensure {
                condition: expression,
                ..
            } => collect_bare_names(expression, out),
            HirStatement::Return { value, .. } => {
                if let Some(value) = value {
                    collect_bare_names(value, out);
                }
            }
            HirStatement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                collect_bare_names(condition, out);
                collect_bare_names_in_block(then_branch, out);
                if let Some(else_branch) = else_branch {
                    collect_bare_names_in_block(else_branch, out);
                }
            }
            HirStatement::For { iterable, body, .. } => {
                collect_bare_names(iterable, out);
                collect_bare_names_in_block(body, out);
            }
            HirStatement::While {
                condition, body, ..
            } => {
                collect_bare_names(condition, out);
                collect_bare_names_in_block(body, out);
            }
            HirStatement::Break { .. } | HirStatement::Continue { .. } => {}
        }
    }
}

fn collect_bare_names(expr: &HirExpression, out: &mut Vec<(String, SourceLocation)>) {
    match expr {
        HirExpression::Literal { .. } => {}
        HirExpression::Variable { name, location } => out.push((name.clone(), location.clone())),
        HirExpression::Call {
            function,
            arguments,
            location,
        } => {
            if !function.contains('.') {
                out.push((function.clone(), location.clone()));
            }
            for arg in arguments {
                collect_bare_names(arg, out);
            }
        }
        HirExpression::MethodCall {
            receiver,
            arguments,
            ..
        } => {
            collect_bare_names(receiver, out);
            for arg in arguments {
                collect_bare_names(arg, out);
            }
        }
        HirExpression::FieldAccess { object, .. } => collect_bare_names(object, out),
        HirExpression::BinaryOp { left, right, .. } => {
            collect_bare_names(left, out);
            collect_bare_names(right, out);
        }
        HirExpression::UnaryOp { operand, .. } => collect_bare_names(operand, out),
        HirExpression::Constructor { arguments, .. }
        | HirExpression::NamespaceCall { arguments, .. }
        | HirExpression::StaticMethodCall { arguments, .. }
        | HirExpression::BaseCall { arguments, .. } => {
            for arg in arguments {
                collect_bare_names(arg, out);
            }
        }
        HirExpression::Index { array, index, .. } => {
            collect_bare_names(array, out);
            collect_bare_names(index, out);
        }
        HirExpression::Array { elements, .. } => {
            for element in elements {
                collect_bare_names(element, out);
            }
        }
        HirExpression::Cast { expression, .. } => collect_bare_names(expression, out),
        HirExpression::OnError {
            expression,
            fallback,
            ..
        } => {
            collect_bare_names(expression, out);
            collect_bare_names(fallback, out);
        }
        HirExpression::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            collect_bare_names(condition, out);
            collect_bare_names(then_expr, out);
            collect_bare_names(else_expr, out);
        }
        HirExpression::Range {
            start, end, step, ..
        } => {
            collect_bare_names(start, out);
            collect_bare_names(end, out);
            if let Some(step) = step {
                collect_bare_names(step, out);
            }
        }
        HirExpression::ObjectLiteral { fields, .. } => {
            for (_, value) in fields {
                collect_bare_names(value, out);
            }
        }
    }
}

/// The (method, default body) pairs of the capability named `cap_name`.
fn defaults_of<'a>(
    capabilities: &'a [HirCapability],
    cap_name: &'a str,
) -> impl Iterator<Item = (&'a HirCapabilityMethod, &'a HirBlock)> {
    capabilities
        .iter()
        .filter(move |cap| cap.name == cap_name)
        .flat_map(|cap| cap.methods.iter())
        .filter_map(|m| m.default_body.as_ref().map(|body| (m, body)))
}

/// A class that claims a capability and neither declares nor inherits a
/// method the capability gives a default body gets that body as a public
/// method of its own (15-classes-and-objects.md: "classes inherit the
/// body unless they override it"). Calls through the class type and
/// through the capability type then reach the same method.
pub fn inherit_defaults(classes: &mut [HirClass], capabilities: &[HirCapability]) {
    // Whether `class` declares `method` or is given it as a default.
    let provides = |class: &HirClass, method: &str| {
        class.methods.iter().any(|m| m.name == method)
            || class
                .capabilities
                .iter()
                .any(|cap| defaults_of(capabilities, cap).any(|(m, _)| m.name == method))
    };
    let inherits = |class: &HirClass, method: &str| {
        let mut seen = std::collections::HashSet::new();
        let mut parent = class.parent.as_deref();
        while let Some(name) = parent.filter(|name| seen.insert(*name)) {
            let Some(parent_class) = classes.iter().find(|c| c.name == name) else {
                return false;
            };
            if provides(parent_class, method) {
                return true;
            }
            parent = parent_class.parent.as_deref();
        }
        false
    };
    let mut additions: Vec<(usize, HirMethod)> = Vec::new();
    for (index, class) in classes.iter().enumerate() {
        for (method, body) in class
            .capabilities
            .iter()
            .flat_map(|cap| defaults_of(capabilities, cap))
        {
            let given = additions
                .iter()
                .any(|(i, m)| *i == index && m.name == method.name);
            if given
                || class.methods.iter().any(|m| m.name == method.name)
                || inherits(class, &method.name)
            {
                continue;
            }
            additions.push((
                index,
                HirMethod {
                    name: method.name.clone(),
                    parameters: method.parameters.clone(),
                    return_type: method.return_type.clone(),
                    body: body.clone(),
                    is_private: false,
                    location: method.location.clone(),
                },
            ));
        }
    }
    for (index, method) in additions {
        classes[index].methods.push(method);
    }
}
