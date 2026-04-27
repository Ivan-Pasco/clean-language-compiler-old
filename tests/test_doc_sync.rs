//! Integration tests for the doc-code synchronisation engine.
//!
//! Tests cover:
//!   - validate: clean spec (all refs resolve)
//!   - validate: broken refs (symbol not in source)
//!   - validate: stale refs (stored signature differs from current)
//!   - validate: update_signatures writes snapshot back to file
//!   - coverage: covered / uncovered split
//!   - coverage: percentage calculation

use clean_language_compiler::docs::coverage::compute_coverage;
use clean_language_compiler::docs::{AvailableSymbols, DocSyncEngine, FunctionSig};
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn docs_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("docs-features")
}

fn banking_source() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("cln")
            .join("docs")
            .join("banking.cln"),
    )
    .expect("banking.cln fixture missing")
}

fn make_banking_symbols() -> AvailableSymbols {
    AvailableSymbols {
        functions: vec![
            FunctionSig {
                name: "deposit".into(),
                signature: "void deposit(BankAccount account, number amount)".into(),
            },
            FunctionSig {
                name: "getBalance".into(),
                signature: "number getBalance(BankAccount account)".into(),
            },
            FunctionSig {
                name: "withdraw".into(),
                signature: "void withdraw(BankAccount account, number amount)".into(),
            },
        ],
        classes: vec!["BankAccount".into()],
        state_vars: vec![],
    }
}

/// Write a temporary spec file in a temp dir and return its parent dir + path.
fn write_temp_spec(dir: &tempfile::TempDir, filename: &str, content: &str) -> PathBuf {
    let path = dir.path().join(filename);
    fs::write(&path, content).unwrap();
    path
}

// ---------------------------------------------------------------------------
// validate — clean spec
// ---------------------------------------------------------------------------

#[test]
fn test_validate_clean_spec() {
    let engine = DocSyncEngine::new(docs_fixtures());
    let specs = engine.scan_specs();

    let deposit_spec = specs
        .iter()
        .find(|s| s.feature == "Deposit Money")
        .expect("deposit.md should be loaded");

    let symbols = make_banking_symbols();
    let result = engine.validate(deposit_spec, &symbols);

    assert!(
        result.broken_refs.is_empty(),
        "Expected no broken refs, got: {:?}",
        result.broken_refs
    );
    assert!(
        result.stale_refs.is_empty(),
        "Expected no stale refs (no stored signatures yet)"
    );
}

// ---------------------------------------------------------------------------
// validate — broken refs
// ---------------------------------------------------------------------------

#[test]
fn test_validate_broken_refs() {
    let engine = DocSyncEngine::new(docs_fixtures());
    let specs = engine.scan_specs();

    let ghost = specs
        .iter()
        .find(|s| s.feature == "Ghost Feature")
        .expect("broken.md should be loaded");

    let symbols = make_banking_symbols();
    let result = engine.validate(ghost, &symbols);

    assert_eq!(
        result.broken_refs.len(),
        2,
        "Expected 2 broken refs (transfer + Wallet), got: {:?}",
        result.broken_refs
    );

    let ref_strs: Vec<&str> = result
        .broken_refs
        .iter()
        .map(|r| r.ref_str.as_str())
        .collect();
    assert!(
        ref_strs.contains(&"functions/transfer"),
        "Missing broken ref for transfer"
    );
    assert!(
        ref_strs.contains(&"classes/Wallet"),
        "Missing broken ref for Wallet"
    );
}

// ---------------------------------------------------------------------------
// validate — stale refs
// ---------------------------------------------------------------------------

#[test]
fn test_validate_stale_ref() {
    let tmp = tempfile::tempdir().unwrap();

    // Spec with a stored signature that differs from what the symbols say
    let content = "---\nfeature: Stale Test\ndoc: functions/deposit\nsignatures:\n  deposit: \"void deposit(number amount)\"\n---\n# body\n";
    write_temp_spec(&tmp, "stale.md", content);

    let engine = DocSyncEngine::new(tmp.path().to_path_buf());
    let specs = engine.scan_specs();
    assert_eq!(specs.len(), 1);

    let symbols = make_banking_symbols();
    let result = engine.validate(&specs[0], &symbols);

    assert!(
        result.broken_refs.is_empty(),
        "Should not be broken, just stale"
    );
    assert_eq!(
        result.stale_refs.len(),
        1,
        "Expected 1 stale ref, got: {:?}",
        result.stale_refs
    );
    assert_eq!(result.stale_refs[0].symbol_name, "deposit");
    assert_eq!(
        result.stale_refs[0].stored_sig,
        "void deposit(number amount)"
    );
    assert!(
        result.stale_refs[0].current_sig.contains("BankAccount"),
        "Current sig should mention BankAccount parameter"
    );
}

// ---------------------------------------------------------------------------
// validate — update_signatures writes snapshot
// ---------------------------------------------------------------------------

#[test]
fn test_update_signatures_writes_to_file() {
    let tmp = tempfile::tempdir().unwrap();
    let content =
        "---\nfeature: Snapshot Test\ndoc: functions/deposit, functions/getBalance\n---\n# body\n";
    let path = write_temp_spec(&tmp, "snapshot.md", content);

    let engine = DocSyncEngine::new(tmp.path().to_path_buf());
    let _specs = engine.scan_specs();

    let mut new_sigs = std::collections::HashMap::new();
    new_sigs.insert(
        "deposit".to_string(),
        "void deposit(BankAccount account, number amount)".to_string(),
    );
    new_sigs.insert(
        "getBalance".to_string(),
        "number getBalance(BankAccount account)".to_string(),
    );

    DocSyncEngine::update_signatures_in_file(&path, &new_sigs)
        .expect("update_signatures_in_file should succeed");

    let updated = fs::read_to_string(&path).unwrap();
    assert!(
        updated.contains("signatures:"),
        "signatures: block should be present"
    );
    assert!(
        updated.contains("deposit:"),
        "deposit key should be written"
    );
    assert!(
        updated.contains("getBalance:"),
        "getBalance key should be written"
    );
    assert!(
        updated.contains("# body"),
        "markdown body should be preserved"
    );
}

// ---------------------------------------------------------------------------
// coverage — split covered / uncovered
// ---------------------------------------------------------------------------

#[test]
fn test_coverage_partial() {
    let engine = DocSyncEngine::new(docs_fixtures());
    let specs = engine.scan_specs();

    // deposit.md covers deposit, getBalance, BankAccount
    // withdraw is NOT covered by any spec
    let symbols = make_banking_symbols();
    let report = compute_coverage(&symbols, &specs);

    let uncovered_names: Vec<&str> = report.uncovered.iter().map(|u| u.name.as_str()).collect();
    assert!(
        uncovered_names.contains(&"withdraw"),
        "withdraw should be uncovered; got {:?}",
        uncovered_names
    );

    let covered_names: Vec<&str> = report.covered.iter().map(String::as_str).collect();
    assert!(
        covered_names.iter().any(|s| s.contains("deposit")),
        "deposit should be covered"
    );
    assert!(
        covered_names.iter().any(|s| s.contains("getBalance")),
        "getBalance should be covered"
    );
}

#[test]
fn test_coverage_percentage_full() {
    let symbols = AvailableSymbols {
        functions: vec![FunctionSig {
            name: "add".into(),
            signature: "integer add(integer a, integer b)".into(),
        }],
        classes: vec![],
        state_vars: vec![],
    };

    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("add.md"),
        "---\nfeature: Add\ndoc: functions/add\n---\n# body\n",
    )
    .unwrap();

    let engine = DocSyncEngine::new(tmp.path().to_path_buf());
    let specs = engine.scan_specs();
    let report = compute_coverage(&symbols, &specs);

    assert_eq!(report.coverage_pct, 100.0);
    assert!(report.uncovered.is_empty());
}

#[test]
fn test_coverage_percentage_zero() {
    let symbols = AvailableSymbols {
        functions: vec![FunctionSig {
            name: "secret".into(),
            signature: "void secret()".into(),
        }],
        classes: vec![],
        state_vars: vec![],
    };

    // Empty docs dir — no specs, no coverage
    let tmp = tempfile::tempdir().unwrap();
    let engine = DocSyncEngine::new(tmp.path().to_path_buf());
    let specs = engine.scan_specs();
    let report = compute_coverage(&symbols, &specs);

    assert_eq!(report.coverage_pct, 0.0);
    assert_eq!(report.uncovered.len(), 1);
    assert_eq!(report.uncovered[0].name, "secret");
}

// ---------------------------------------------------------------------------
// extract_symbols_from_source — via public API round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_extract_symbols_from_banking_fixture() {
    use clean_language_compiler::docs::extract_symbols_from_source;

    let source = banking_source();
    let symbols = extract_symbols_from_source(&source);

    let fn_names: Vec<&str> = symbols.functions.iter().map(|f| f.name.as_str()).collect();
    assert!(fn_names.contains(&"deposit"), "deposit should be extracted");
    assert!(
        fn_names.contains(&"getBalance"),
        "getBalance should be extracted"
    );
    assert!(
        fn_names.contains(&"withdraw"),
        "withdraw should be extracted"
    );
    assert!(
        symbols.classes.iter().any(|c| c == "BankAccount"),
        "BankAccount class should be extracted"
    );
}
