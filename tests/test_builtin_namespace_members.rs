//! A built-in namespace lists only functions that are registered.
//!
//! The namespace lists used `lookup_symbol(name).unwrap_or(SymbolId(0))`, and
//! SymbolId(0) is `print`: every unregistered name became a `print` member of
//! its namespace (MCP `list_builtins` showed `print` twice under `http`).

use clean_language_compiler::resolver::{GlobalSymbolTable, SymbolKind};
use std::collections::HashSet;

#[test]
fn namespace_members_are_distinct_registered_functions() {
    let table = GlobalSymbolTable::new();
    let mut namespaces = 0;
    for symbol in table.all_symbols().values() {
        let SymbolKind::Namespace { functions } = &symbol.kind else {
            continue;
        };
        namespaces += 1;
        let mut seen = HashSet::new();
        for id in functions {
            let member = table
                .get_symbol(*id)
                .unwrap_or_else(|| panic!("namespace '{}' lists a missing symbol", symbol.name));
            assert_ne!(
                member.name, "print",
                "namespace '{}' lists print",
                symbol.name
            );
            assert!(
                seen.insert(*id),
                "namespace '{}' lists '{}' twice",
                symbol.name,
                member.name
            );
        }
    }
    assert!(namespaces > 0, "no built-in namespaces registered");
}
