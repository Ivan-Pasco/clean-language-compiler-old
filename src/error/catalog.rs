//! Error code catalog.
//!
//! The one list of diagnostic codes shared by `cln explain` and the MCP
//! `explain_error` / `list_error_codes` tools. It is read from the error code
//! registry (`foundation/spec/error-codes.md`) and the rule definitions
//! (`foundation/spec/semantic-rules.md`), embedded at build time from the
//! vendored copies under `src/error/spec/`. `build.rs` refreshes those copies
//! from the sibling foundation workspace when it is present, so the catalog
//! never drifts from the spec.

use std::sync::OnceLock;

const ERROR_CODES_MD: &str = include_str!("spec/error-codes.md");
const SEMANTIC_RULES_MD: &str = include_str!("spec/semantic-rules.md");

/// One diagnostic code from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCodeInfo {
    /// The code, e.g. `SEM003` or `BLD-LAYOUT`.
    pub code: String,
    /// Registry name, e.g. `SymbolRedefinition`.
    pub name: String,
    /// Category taken from the registry section, e.g. `Semantic`.
    pub category: String,
    /// One-line description from the registry.
    pub summary: String,
    /// `Error`, `Warning` or `Runtime`.
    pub severity: String,
    /// Title of the rule in semantic-rules.md, when it defines one.
    pub title: Option<String>,
    /// Body of the rule in semantic-rules.md (examples, what it applies to).
    pub details: Option<String>,
}

/// Every code in the registry, in registry order.
pub fn all() -> &'static [ErrorCodeInfo] {
    static CATALOG: OnceLock<Vec<ErrorCodeInfo>> = OnceLock::new();
    CATALOG.get_or_init(|| parse_catalog(ERROR_CODES_MD, SEMANTIC_RULES_MD))
}

/// Look up a code, ignoring case. `None` when the registry does not list it.
pub fn lookup(code: &str) -> Option<&'static ErrorCodeInfo> {
    let wanted = code.trim();
    all().iter().find(|e| e.code.eq_ignore_ascii_case(wanted))
}

fn parse_catalog(registry: &str, rules: &str) -> Vec<ErrorCodeInfo> {
    let rule_sections = parse_rule_sections(rules);
    let mut entries = Vec::new();
    let mut in_index = false;
    let mut category = String::new();

    for line in registry.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_index = heading.trim() == "Full Code Index";
            continue;
        }
        if !in_index {
            continue;
        }
        if let Some(heading) = line.strip_prefix("### ") {
            // "Semantic Codes (SEM)" -> "Semantic"
            category = heading
                .split(" Codes")
                .next()
                .unwrap_or(heading)
                .trim()
                .to_string();
            continue;
        }
        let Some(cells) = table_cells(line) else {
            continue;
        };
        if cells.len() < 4 || !is_code(&cells[0]) {
            continue;
        }
        let code = cells[0].clone();
        let (title, details) = match rule_sections.iter().find(|(c, _, _)| *c == code) {
            Some((_, title, body)) => (Some(title.clone()), Some(body.clone())),
            None => (None, None),
        };
        entries.push(ErrorCodeInfo {
            name: cells[1].trim_matches('`').to_string(),
            summary: cells[2].clone(),
            severity: cells[3].clone(),
            category: category.clone(),
            code,
            title,
            details,
        });
    }
    entries
}

/// Split a markdown table row into trimmed cells. `None` for non-table lines.
fn table_cells(line: &str) -> Option<Vec<String>> {
    let inner = line.trim().strip_prefix('|')?.strip_suffix('|')?;
    Some(inner.split('|').map(|c| c.trim().to_string()).collect())
}

/// Codes are upper-case letters followed by digits (`SEM003`) or a
/// dash-separated tag (`BLD-LAYOUT`).
fn is_code(cell: &str) -> bool {
    let mut chars = cell.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase())
        && cell
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
        && (cell.chars().any(|c| c.is_ascii_digit()) || cell.contains('-'))
}

/// `(code, title, body)` for every `### CODE — Title` section.
fn parse_rule_sections(rules: &str) -> Vec<(String, String, String)> {
    let mut sections = Vec::new();
    let mut current: Option<(String, String, Vec<&str>)> = None;

    let mut finish = |current: &mut Option<(String, String, Vec<&str>)>| {
        if let Some((code, title, body)) = current.take() {
            sections.push((code, title, body.join("\n").trim().to_string()));
        }
    };

    for line in rules.lines() {
        if let Some(heading) = line.strip_prefix("### ") {
            finish(&mut current);
            if let Some((code, title)) = heading.split_once(" — ") {
                let code = code.trim();
                if is_code(code) {
                    current = Some((code.to_string(), title.trim().to_string(), Vec::new()));
                }
            }
            continue;
        }
        if line.starts_with("## ") || line.starts_with("# ") {
            finish(&mut current);
            continue;
        }
        if let Some((_, _, body)) = current.as_mut() {
            body.push(line);
        }
    }
    finish(&mut current);
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_registry_codes_in_every_category() {
        let codes: Vec<&str> = all().iter().map(|e| e.code.as_str()).collect();
        for expected in [
            "SYN001", "SEM003", "SCOPE001", "FUNC002", "COM001", "RUN005",
        ] {
            assert!(codes.contains(&expected), "{expected} missing: {codes:?}");
        }
        assert!(codes.contains(&"BLD-LAYOUT"));
        // Headers and range tables are not codes.
        assert!(!codes.iter().any(|c| *c == "Code" || c.contains('–')));
    }

    #[test]
    fn lookup_is_case_insensitive_and_carries_rule_details() {
        let sem003 = lookup("sem003").expect("SEM003 is in the registry");
        assert_eq!(sem003.name, "SymbolRedefinition");
        assert_eq!(sem003.category, "Semantic");
        assert_eq!(sem003.severity, "Error");
        assert_eq!(sem003.title.as_deref(), Some("Symbol Redefinition"));
        assert!(sem003
            .details
            .as_deref()
            .unwrap_or_default()
            .contains("declared more than once"));
    }

    #[test]
    fn unknown_codes_are_not_found() {
        assert!(lookup("NOPE999").is_none());
        assert!(lookup("").is_none());
    }
}
