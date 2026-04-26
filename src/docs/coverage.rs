/*!
 * Documentation Coverage Reporting
 *
 * Computes which compiler-visible symbols are referenced by at least one
 * feature spec, and which are not yet documented.
 */

use crate::docs::{AvailableSymbols, DocRef, FeatureSpec};

/// A symbol that has no corresponding `doc:` reference in any feature spec.
#[derive(Debug)]
pub struct UncoveredSymbol {
    /// Broad kind: `"function"`, `"class"`, or `"state"`.
    pub kind: String,
    /// Symbol name.
    pub name: String,
}

/// The result of a doc-coverage analysis.
#[derive(Debug)]
pub struct CoverageReport {
    /// Symbols that are referenced by at least one feature spec.
    pub covered: Vec<String>,
    /// Symbols with no documentation coverage.
    pub uncovered: Vec<UncoveredSymbol>,
    /// Percentage of symbols that are covered (0.0 – 100.0).
    pub coverage_pct: f32,
}

/// Compute documentation coverage for the given symbol set against all loaded
/// feature specs.
///
/// A symbol is considered *covered* if at least one spec's `doc_refs` contains
/// a reference whose kind and name (case-insensitive) match the symbol.
pub fn compute_coverage(symbols: &AvailableSymbols, specs: &[FeatureSpec]) -> CoverageReport {
    // Build a flat set of all ref strings mentioned in any spec
    let mut referenced_functions: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut referenced_classes: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut referenced_state: std::collections::HashSet<String> = std::collections::HashSet::new();

    for spec in specs {
        for doc_ref in &spec.doc_refs {
            match doc_ref {
                DocRef::Function(n) => {
                    referenced_functions.insert(n.to_lowercase());
                }
                DocRef::Class(n) => {
                    referenced_classes.insert(n.to_lowercase());
                }
                DocRef::State(n) => {
                    referenced_state.insert(n.to_lowercase());
                }
            }
        }
    }

    let mut covered: Vec<String> = Vec::new();
    let mut uncovered: Vec<UncoveredSymbol> = Vec::new();

    // Check functions
    for func in &symbols.functions {
        if referenced_functions.contains(&func.name.to_lowercase()) {
            covered.push(format!("functions/{}", func.name));
        } else {
            uncovered.push(UncoveredSymbol {
                kind: "function".to_string(),
                name: func.name.clone(),
            });
        }
    }

    // Check classes
    for class in &symbols.classes {
        if referenced_classes.contains(&class.to_lowercase()) {
            covered.push(format!("classes/{}", class));
        } else {
            uncovered.push(UncoveredSymbol {
                kind: "class".to_string(),
                name: class.clone(),
            });
        }
    }

    // Check state vars
    for sv in &symbols.state_vars {
        if referenced_state.contains(&sv.to_lowercase()) {
            covered.push(format!("state/{}", sv));
        } else {
            uncovered.push(UncoveredSymbol {
                kind: "state".to_string(),
                name: sv.clone(),
            });
        }
    }

    let total = covered.len() + uncovered.len();
    let coverage_pct = if total == 0 {
        100.0_f32
    } else {
        (covered.len() as f32 / total as f32) * 100.0
    };

    CoverageReport {
        covered,
        uncovered,
        coverage_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::{DocRef, FeatureSpec, FunctionSig};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_spec(refs: Vec<DocRef>) -> FeatureSpec {
        FeatureSpec {
            path: PathBuf::from("/tmp/test.md"),
            feature: "test".to_string(),
            doc_refs: refs,
            stored_signatures: HashMap::new(),
        }
    }

    #[test]
    fn test_full_coverage() {
        let symbols = AvailableSymbols {
            functions: vec![FunctionSig {
                name: "deposit".to_string(),
                signature: "void deposit(number amount)".to_string(),
            }],
            classes: vec!["Account".to_string()],
            state_vars: vec!["balance".to_string()],
        };

        let specs = vec![make_spec(vec![
            DocRef::Function("deposit".to_string()),
            DocRef::Class("Account".to_string()),
            DocRef::State("balance".to_string()),
        ])];

        let report = compute_coverage(&symbols, &specs);
        assert_eq!(report.covered.len(), 3);
        assert!(report.uncovered.is_empty());
        assert!((report.coverage_pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_zero_coverage() {
        let symbols = AvailableSymbols {
            functions: vec![FunctionSig {
                name: "deposit".to_string(),
                signature: "void deposit(number amount)".to_string(),
            }],
            classes: vec![],
            state_vars: vec![],
        };

        let specs: Vec<FeatureSpec> = vec![];
        let report = compute_coverage(&symbols, &specs);
        assert!(report.covered.is_empty());
        assert_eq!(report.uncovered.len(), 1);
        assert!((report.coverage_pct - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_empty_symbols() {
        let symbols = AvailableSymbols::default();
        let specs: Vec<FeatureSpec> = vec![];
        let report = compute_coverage(&symbols, &specs);
        // No symbols → 100% coverage (nothing to document)
        assert!((report.coverage_pct - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_case_insensitive_matching() {
        let symbols = AvailableSymbols {
            functions: vec![FunctionSig {
                name: "Transfer".to_string(),
                signature: "void Transfer(Account a, Account b)".to_string(),
            }],
            classes: vec![],
            state_vars: vec![],
        };

        let specs = vec![make_spec(vec![DocRef::Function("transfer".to_string())])];
        let report = compute_coverage(&symbols, &specs);
        assert_eq!(report.covered.len(), 1);
        assert!(report.uncovered.is_empty());
    }
}
