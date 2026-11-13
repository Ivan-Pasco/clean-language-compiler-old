//! Capability default bodies (15-classes-and-objects.md §Capabilities).
//!
//! Both run on the whole program, after every module's HIR is merged, so a
//! class may claim a capability declared in another module.

use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::*;

/// SEM014: a capability's default body may use `this` only to call the
/// capability's own methods; any other `this.name` (a field, or a method
/// the capability does not declare) is not guaranteed to exist on every
/// class that claims it.
pub fn check_defaults(capabilities: &[HirCapability]) -> Result<(), CompilerError> {
    for cap in capabilities {
        let declared: std::collections::HashSet<&str> =
            cap.methods.iter().map(|m| m.name.as_str()).collect();
        for method in &cap.methods {
            let Some(body) = &method.default_body else {
                continue;
            };
            let mut used = Vec::new();
            collect_this_members_in_block(body, &mut used);
            if let Some((name, location)) = used
                .into_iter()
                .find(|(name, _)| !declared.contains(name.as_str()))
            {
                return Err(CompilerError::semantic_error_with_code(
                    format!(
                        "The default body of '{}.{}' uses 'this.{name}', but '{name}' is not \
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

/// Every `this.name` member (field read or write, method call) in
/// `block`, with where it appears.
fn collect_this_members_in_block(block: &HirBlock, out: &mut Vec<(String, SourceLocation)>) {
    for statement in &block.statements {
        match statement {
            HirStatement::VariableDeclaration { initializer, .. } => {
                if let Some(init) = initializer {
                    collect_this_members(init, out);
                }
            }
            HirStatement::Assignment { target, value, .. } => {
                match target {
                    HirLValue::Variable { .. } => {}
                    HirLValue::FieldAccess {
                        object,
                        field,
                        location,
                    } => {
                        if matches!(&**object, HirExpression::Variable { name, .. } if name == "this")
                        {
                            out.push((field.clone(), location.clone()));
                        }
                        collect_this_members(object, out);
                    }
                    HirLValue::Index { array, index, .. } => {
                        collect_this_members(array, out);
                        collect_this_members(index, out);
                    }
                }
                collect_this_members(value, out);
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
            } => collect_this_members(expression, out),
            HirStatement::Return { value, .. } => {
                if let Some(value) = value {
                    collect_this_members(value, out);
                }
            }
            HirStatement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                collect_this_members(condition, out);
                collect_this_members_in_block(then_branch, out);
                if let Some(else_branch) = else_branch {
                    collect_this_members_in_block(else_branch, out);
                }
            }
            HirStatement::For { iterable, body, .. } => {
                collect_this_members(iterable, out);
                collect_this_members_in_block(body, out);
            }
            HirStatement::While {
                condition, body, ..
            } => {
                collect_this_members(condition, out);
                collect_this_members_in_block(body, out);
            }
            HirStatement::Break { .. } | HirStatement::Continue { .. } => {}
        }
    }
}

fn collect_this_members(expr: &HirExpression, out: &mut Vec<(String, SourceLocation)>) {
    let is_this =
        |e: &HirExpression| matches!(e, HirExpression::Variable { name, .. } if name == "this");
    match expr {
        HirExpression::Literal { .. } | HirExpression::Variable { .. } => {}
        HirExpression::MethodCall {
            receiver,
            method,
            arguments,
            location,
        } => {
            if is_this(receiver) {
                out.push((method.clone(), location.clone()));
            }
            collect_this_members(receiver, out);
            for arg in arguments {
                collect_this_members(arg, out);
            }
        }
        HirExpression::FieldAccess {
            object,
            field,
            location,
        } => {
            if is_this(object) {
                out.push((field.clone(), location.clone()));
            }
            collect_this_members(object, out);
        }
        HirExpression::BinaryOp { left, right, .. } => {
            collect_this_members(left, out);
            collect_this_members(right, out);
        }
        HirExpression::UnaryOp { operand, .. } => collect_this_members(operand, out),
        HirExpression::Call { arguments, .. }
        | HirExpression::Constructor { arguments, .. }
        | HirExpression::NamespaceCall { arguments, .. }
        | HirExpression::StaticMethodCall { arguments, .. }
        | HirExpression::BaseCall { arguments, .. } => {
            for arg in arguments {
                collect_this_members(arg, out);
            }
        }
        HirExpression::Index { array, index, .. } => {
            collect_this_members(array, out);
            collect_this_members(index, out);
        }
        HirExpression::Array { elements, .. } => {
            for element in elements {
                collect_this_members(element, out);
            }
        }
        HirExpression::Cast { expression, .. } => collect_this_members(expression, out),
        HirExpression::OnError {
            expression,
            fallback,
            ..
        } => {
            collect_this_members(expression, out);
            collect_this_members(fallback, out);
        }
        HirExpression::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            collect_this_members(condition, out);
            collect_this_members(then_expr, out);
            collect_this_members(else_expr, out);
        }
        HirExpression::Range {
            start, end, step, ..
        } => {
            collect_this_members(start, out);
            collect_this_members(end, out);
            if let Some(step) = step {
                collect_this_members(step, out);
            }
        }
        HirExpression::ObjectLiteral { fields, .. } => {
            for (_, value) in fields {
                collect_this_members(value, out);
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
