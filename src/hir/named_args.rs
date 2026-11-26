//! Matching of positional and named call arguments to a callee's parameters
//! (FUNC008-FUNC011, semantic-rules.md; grammar.ebnf `argument_item`,
//! `named_argument`).
//!
//! The matching only needs the parameter names and which parameters have a
//! default, so the HIR builder (free functions and constructors, whose callee
//! is known by name) and the type checker (instance methods, whose callee is
//! known once the receiver's class is) share it.

use crate::ast::SourceLocation;
use crate::error::CompilerError;

/// One argument of a call, in the order it was written.
pub struct CallArgument<'a> {
    /// The label of a named argument, `None` for a positional one.
    pub label: Option<&'a str>,
    /// Where the argument is written.
    pub location: &'a SourceLocation,
}

/// The parameters of the callee.
pub struct CalleeParameters<'a> {
    pub names: &'a [String],
    /// `true` for each parameter that declares a default value.
    pub has_default: &'a [bool],
}

/// Match `arguments` to the parameters. The result holds, for every
/// parameter, the index into `arguments` that supplies it, or `None` when
/// the call leaves a parameter with a default out.
///
/// * FUNC010: a positional argument after a named one.
/// * FUNC009: a label used twice.
/// * FUNC008: a label that names no parameter.
/// * FUNC011: a parameter supplied twice, a parameter without default left
///   out, or more arguments than parameters.
pub fn match_arguments(
    callee: &str,
    parameters: &CalleeParameters<'_>,
    arguments: &[CallArgument<'_>],
    call_location: &SourceLocation,
) -> Result<Vec<Option<usize>>, CompilerError> {
    let names = parameters.names;
    let param_count = names.len();

    let mut positional_count = 0;
    let mut first_named: Option<usize> = None;
    for (i, argument) in arguments.iter().enumerate() {
        if argument.label.is_some() {
            first_named.get_or_insert(i);
        } else if let Some(named_start) = first_named {
            return Err(CompilerError::semantic_error_with_code(
                format!(
                    "FUNC010: positional argument at position {} appears after named \
                     argument at position {} in call to `{}`",
                    i, named_start, callee
                ),
                Some("All positional arguments must come before named arguments".to_string()),
                Some(argument.location.clone()),
                "FUNC010",
            ));
        } else {
            positional_count = i + 1;
        }
    }

    if positional_count > param_count {
        return Err(CompilerError::semantic_error_with_code(
            format!(
                "FUNC011: `{}` expects {} argument(s), got {}",
                callee,
                param_count,
                arguments.len()
            ),
            Some(format!(
                "Provide at most {} argument(s): {}",
                param_count,
                names.join(", ")
            )),
            Some(call_location.clone()),
            "FUNC011",
        ));
    }

    let mut slots: Vec<Option<usize>> = vec![None; param_count];
    for (i, slot) in slots.iter_mut().enumerate().take(positional_count) {
        *slot = Some(i);
    }

    let mut seen_labels: Vec<&str> = Vec::new();
    for (i, argument) in arguments.iter().enumerate().skip(positional_count) {
        let Some(label) = argument.label else {
            continue;
        };
        if seen_labels.contains(&label) {
            return Err(CompilerError::semantic_error_with_code(
                format!(
                    "FUNC009: duplicate named argument `{}` in call to `{}`",
                    label, callee
                ),
                Some(format!("Remove the duplicate `{}:` argument", label)),
                Some(argument.location.clone()),
                "FUNC009",
            ));
        }
        seen_labels.push(label);

        let Some(idx) = names.iter().position(|n| n == label) else {
            return Err(CompilerError::semantic_error_with_code(
                format!(
                    "FUNC008: `{}` has no parameter named `{}` in call to `{}`",
                    callee, label, callee
                ),
                Some(format!("Valid parameter names are: {}", names.join(", "))),
                Some(argument.location.clone()),
                "FUNC008",
            ));
        };
        if idx < positional_count {
            return Err(CompilerError::semantic_error_with_code(
                format!(
                    "FUNC011: parameter `{}` (position {}) is already covered \
                     by a positional argument in call to `{}`",
                    label, idx, callee
                ),
                Some(format!(
                    "Remove the positional argument or the `{}:` label",
                    label
                )),
                Some(argument.location.clone()),
                "FUNC011",
            ));
        }
        slots[idx] = Some(i);
    }

    for (i, slot) in slots.iter().enumerate() {
        if slot.is_none() && !parameters.has_default.get(i).copied().unwrap_or(false) {
            return Err(CompilerError::semantic_error_with_code(
                format!(
                    "FUNC011: parameter `{}` (position {}) is not covered in call to `{}`",
                    names[i], i, callee
                ),
                Some(format!(
                    "Add a `{}:` named argument or a positional argument for this \
                     parameter",
                    names[i]
                )),
                Some(call_location.clone()),
                "FUNC011",
            ));
        }
    }
    Ok(slots)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        params: &[&str],
        defaults: &[bool],
        labels: &[Option<&str>],
    ) -> Result<Vec<Option<usize>>, String> {
        let location = SourceLocation::default();
        let names: Vec<String> = params.iter().map(|s| s.to_string()).collect();
        let arguments: Vec<CallArgument<'_>> = labels
            .iter()
            .map(|label| CallArgument {
                label: *label,
                location: &location,
            })
            .collect();
        match_arguments(
            "f",
            &CalleeParameters {
                names: &names,
                has_default: defaults,
            },
            &arguments,
            &location,
        )
        .map_err(|e| e.to_string())
    }

    #[test]
    fn named_arguments_are_put_in_parameter_order() {
        let slots = run(&["a", "b"], &[false, false], &[Some("b"), Some("a")]).unwrap();
        assert_eq!(slots, vec![Some(1), Some(0)]);
    }

    #[test]
    fn a_defaulted_parameter_may_be_left_out() {
        let slots = run(
            &["a", "b", "c"],
            &[false, true, true],
            &[Some("c"), Some("a")],
        )
        .unwrap();
        assert_eq!(slots, vec![Some(1), None, Some(0)]);
    }

    #[test]
    fn a_required_parameter_cannot_be_left_out() {
        let error = run(&["a", "b"], &[false, false], &[Some("a")]).unwrap_err();
        assert!(error.contains("FUNC011"), "{error}");
    }

    #[test]
    fn unknown_duplicate_and_misordered_labels_are_rejected() {
        let unknown = run(&["a"], &[false], &[Some("z")]).unwrap_err();
        assert!(unknown.contains("FUNC008"), "{unknown}");
        let duplicate = run(&["a", "b"], &[false, true], &[Some("a"), Some("a")]).unwrap_err();
        assert!(duplicate.contains("FUNC009"), "{duplicate}");
        let misordered = run(&["a", "b"], &[false, false], &[Some("a"), None]).unwrap_err();
        assert!(misordered.contains("FUNC010"), "{misordered}");
    }
}
