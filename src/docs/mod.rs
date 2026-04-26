/*!
 * Doc-Code Synchronization Engine
 *
 * Scans docs/features/ for *.md files with YAML frontmatter, validates
 * doc: references against compiler-visible symbols, and detects stale
 * signatures when the compiler output changes.
 */

pub mod coverage;

use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ============================================================================
// Public Types
// ============================================================================

/// A function signature extracted from a Clean Language source file.
#[derive(Debug, Clone)]
pub struct FunctionSig {
    /// Function name as it appears in source (e.g., "transfer").
    pub name: String,
    /// Full human-readable signature string (e.g., "void transfer(BankAccount from, BankAccount to, number amount)").
    pub signature: String,
}

/// The set of symbols visible in a compiled Clean Language source unit.
///
/// Used by [`DocSyncEngine::validate`] to check whether every `doc:` reference
/// in a feature spec resolves to a real symbol.
#[derive(Debug, Default)]
pub struct AvailableSymbols {
    /// All functions with their full signatures.
    pub functions: Vec<FunctionSig>,
    /// Class names defined in the source.
    pub classes: Vec<String>,
    /// State variable names defined in `state:` blocks.
    pub state_vars: Vec<String>,
}

/// A reference in a feature spec's `doc:` field, identifying a symbol by kind and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocRef {
    /// e.g., `functions/transfer`
    Function(String),
    /// e.g., `classes/BankAccount`
    Class(String),
    /// e.g., `state/balance`
    State(String),
}

impl DocRef {
    /// Parse a single `type/name` reference string.
    ///
    /// Returns `None` if the string does not match any known ref kind.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(name) = s.strip_prefix("functions/") {
            return Some(DocRef::Function(name.to_string()));
        }
        if let Some(name) = s.strip_prefix("classes/") {
            return Some(DocRef::Class(name.to_string()));
        }
        if let Some(name) = s.strip_prefix("state/") {
            return Some(DocRef::State(name.to_string()));
        }
        None
    }

    /// Returns the raw `type/name` string representation.
    pub fn as_ref_str(&self) -> String {
        match self {
            DocRef::Function(n) => format!("functions/{}", n),
            DocRef::Class(n) => format!("classes/{}", n),
            DocRef::State(n) => format!("state/{}", n),
        }
    }

    /// Returns the symbol name component (without the kind prefix).
    pub fn name(&self) -> &str {
        match self {
            DocRef::Function(n) | DocRef::Class(n) | DocRef::State(n) => n.as_str(),
        }
    }
}

/// A `doc:` reference that could not be resolved against available symbols.
#[derive(Debug)]
pub struct BrokenRef {
    /// The original ref string as written in the frontmatter (e.g., `"functions/transfer"`).
    pub ref_str: String,
    /// Human-readable explanation of why the ref is broken.
    pub reason: String,
}

/// A resolved reference whose stored signature no longer matches the current compiler output.
#[derive(Debug)]
pub struct StaleRef {
    /// Symbol name (function, class, or state var).
    pub symbol_name: String,
    /// Signature stored in the spec file's frontmatter.
    pub stored_sig: String,
    /// Signature currently emitted by the compiler for this symbol.
    pub current_sig: String,
}

/// The outcome of validating one feature spec against a set of available symbols.
#[derive(Debug)]
pub struct ValidationResult {
    /// Path to the feature spec file that was validated.
    pub spec_path: PathBuf,
    /// Value of the `feature:` frontmatter key.
    pub feature_name: String,
    /// References in `doc:` that could not be resolved.
    pub broken_refs: Vec<BrokenRef>,
    /// References whose stored signatures are outdated.
    pub stale_refs: Vec<StaleRef>,
}

impl ValidationResult {
    /// Returns `true` if there are no broken or stale references.
    pub fn is_clean(&self) -> bool {
        self.broken_refs.is_empty() && self.stale_refs.is_empty()
    }
}

/// A parsed feature spec document extracted from a `docs/features/*.md` file.
#[derive(Debug)]
pub struct FeatureSpec {
    /// Absolute path to the source file.
    pub path: PathBuf,
    /// Value of the `feature:` frontmatter key.
    pub feature: String,
    /// Parsed `doc:` references (comma-separated `type/name` pairs).
    pub doc_refs: Vec<DocRef>,
    /// Key-value pairs from the `signatures:` block in the frontmatter.
    pub stored_signatures: HashMap<String, String>,
}

// ============================================================================
// Engine
// ============================================================================

/// Scans a `docs/features/` directory for feature spec Markdown files and
/// validates them against compiler-visible symbols.
pub struct DocSyncEngine {
    /// Root directory containing `*.md` feature spec files.
    pub docs_dir: PathBuf,
}

impl DocSyncEngine {
    /// Create a new engine that reads specs from `docs_dir`.
    pub fn new(docs_dir: PathBuf) -> Self {
        Self { docs_dir }
    }

    /// Scan the `docs_dir` directory for `*.md` files and parse their YAML
    /// frontmatter.  Files that lack a `---` frontmatter block are skipped.
    pub fn scan_specs(&self) -> Vec<FeatureSpec> {
        let mut specs = Vec::new();

        let read_dir = match std::fs::read_dir(&self.docs_dir) {
            Ok(rd) => rd,
            Err(e) => {
                eprintln!("[DocSync] Cannot read docs dir {:?}: {}", self.docs_dir, e);
                return specs;
            }
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }

            match self.parse_spec_file(&path) {
                Some(spec) => specs.push(spec),
                None => {
                    eprintln!("[DocSync] Skipping {:?}: no valid frontmatter", path);
                }
            }
        }

        specs
    }

    /// Parse a single feature spec file.  Returns `None` if the file cannot
    /// be read or has no YAML frontmatter delimited by `---` lines.
    fn parse_spec_file(&self, path: &Path) -> Option<FeatureSpec> {
        let content = std::fs::read_to_string(path).ok()?;
        let frontmatter = extract_frontmatter(&content)?;

        let feature = extract_scalar_field(&frontmatter, "feature").unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });

        let doc_refs = extract_scalar_field(&frontmatter, "doc")
            .map(|raw| {
                raw.split(',')
                    .filter_map(|s| DocRef::parse(s.trim()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let stored_signatures = extract_signatures_block(&frontmatter);

        Some(FeatureSpec {
            path: path.to_path_buf(),
            feature,
            doc_refs,
            stored_signatures,
        })
    }

    /// Validate a feature spec against a set of available symbols.
    ///
    /// - **Broken refs**: every `doc:` reference that names a symbol not present
    ///   in `available_symbols`.
    /// - **Stale refs**: every entry in `signatures:` whose value no longer
    ///   matches the signature emitted by the compiler for that symbol.
    pub fn validate(
        &self,
        spec: &FeatureSpec,
        available_symbols: &AvailableSymbols,
    ) -> ValidationResult {
        let mut broken_refs = Vec::new();
        let mut stale_refs = Vec::new();

        for doc_ref in &spec.doc_refs {
            match doc_ref {
                DocRef::Function(name) => {
                    if let Some(sig) = available_symbols
                        .functions
                        .iter()
                        .find(|f| f.name.eq_ignore_ascii_case(name))
                    {
                        // Check for staleness
                        if let Some(stored) = spec.stored_signatures.get(name.as_str()) {
                            if stored.trim() != sig.signature.trim() {
                                stale_refs.push(StaleRef {
                                    symbol_name: name.clone(),
                                    stored_sig: stored.clone(),
                                    current_sig: sig.signature.clone(),
                                });
                            }
                        }
                    } else {
                        broken_refs.push(BrokenRef {
                            ref_str: doc_ref.as_ref_str(),
                            reason: format!("Function '{}' not found in source symbols", name),
                        });
                    }
                }
                DocRef::Class(name) => {
                    let found = available_symbols
                        .classes
                        .iter()
                        .any(|c| c.eq_ignore_ascii_case(name));
                    if !found {
                        broken_refs.push(BrokenRef {
                            ref_str: doc_ref.as_ref_str(),
                            reason: format!("Class '{}' not found in source symbols", name),
                        });
                    } else if let Some(stored) = spec.stored_signatures.get(name.as_str()) {
                        // Class signatures are recorded as the class name itself; any
                        // mismatch means the name changed (rename).
                        if stored.trim() != name.trim() {
                            stale_refs.push(StaleRef {
                                symbol_name: name.clone(),
                                stored_sig: stored.clone(),
                                current_sig: name.clone(),
                            });
                        }
                    }
                }
                DocRef::State(name) => {
                    let found = available_symbols
                        .state_vars
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(name));
                    if !found {
                        broken_refs.push(BrokenRef {
                            ref_str: doc_ref.as_ref_str(),
                            reason: format!(
                                "State variable '{}' not found in source symbols",
                                name
                            ),
                        });
                    }
                }
            }
        }

        ValidationResult {
            spec_path: spec.path.clone(),
            feature_name: spec.feature.clone(),
            broken_refs,
            stale_refs,
        }
    }

    /// Rewrite the `signatures:` block in `spec_path` with the values in
    /// `signatures`.  The rest of the frontmatter and the document body are
    /// preserved exactly.
    ///
    /// If the file has no `signatures:` key the block is inserted before the
    /// closing `---` of the frontmatter.
    pub fn update_signatures_in_file(
        spec_path: &Path,
        signatures: &HashMap<String, String>,
    ) -> std::io::Result<()> {
        let original = std::fs::read_to_string(spec_path)?;
        let updated = rewrite_signatures_block(&original, signatures);
        std::fs::write(spec_path, updated)
    }
}

// ============================================================================
// Symbol Extraction (regex-based)
// ============================================================================

/// Extract `AvailableSymbols` from Clean Language source code using regex
/// pattern matching.  This is a lightweight approach suitable for the doc-sync
/// use case: we need symbol names and signatures, not full semantic analysis.
pub fn extract_symbols_from_source(source: &str) -> AvailableSymbols {
    let functions = extract_functions(source);
    let classes = extract_classes(source);
    let state_vars = extract_state_vars(source);
    AvailableSymbols {
        functions,
        classes,
        state_vars,
    }
}

/// Extract function definitions.
///
/// Matches lines of the form:
/// `    ReturnType functionName(params...)` or
/// `ReturnType functionName(params...)`
/// inside a `functions:` block.
fn extract_functions(source: &str) -> Vec<FunctionSig> {
    // Match indented function declarations: optional whitespace, return type, name, ( params )
    let re = Regex::new(
        r"(?m)^[ \t]*(?:void|integer|number|string|boolean|[\w<>\[\],\s]+?)\s+(\w+)\s*\(([^)]*)\)",
    )
    .expect("valid regex");

    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Walk through functions: block
    let mut in_functions_block = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "functions:" {
            in_functions_block = true;
            continue;
        }
        // A non-indented non-empty line that isn't a comment ends the block
        if in_functions_block
            && !line.starts_with('\t')
            && !line.starts_with("    ")
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
        {
            in_functions_block = false;
        }
        if !in_functions_block {
            continue;
        }

        // Only consider lines that look like function signatures (not bodies)
        if let Some(cap) = re.find(trimmed) {
            let sig_line = cap.as_str().trim().to_string();
            // Extract function name from the capture
            if let Some(name_cap) = Regex::new(r"(\w+)\s*\(")
                .expect("valid regex")
                .captures(&sig_line)
            {
                let name = name_cap[1].to_string();
                // Skip common control-flow keywords that look like calls
                let reserved = ["if", "while", "for", "return", "print", "printl", "input"];
                if reserved.contains(&name.as_str()) {
                    continue;
                }
                if seen.insert(name.clone()) {
                    results.push(FunctionSig {
                        name,
                        signature: sig_line,
                    });
                }
            }
        }
    }

    // Also scan outside functions: block for top-level function declarations
    // (some files declare functions at the top level without a block header)
    let re2 = Regex::new(
        r"(?m)^(?:void|integer|number|string|boolean|[\w<>]+)\s+(\w+)\s*\(([^)]*)\)\s*$",
    )
    .expect("valid regex");

    for cap in re2.captures_iter(source) {
        let name = cap[1].to_string();
        let reserved = [
            "if", "while", "for", "return", "print", "printl", "input", "start",
        ];
        if reserved.contains(&name.as_str()) {
            continue;
        }
        if seen.insert(name.clone()) {
            let sig_line = cap[0].trim().to_string();
            results.push(FunctionSig {
                name,
                signature: sig_line,
            });
        }
    }

    results
}

/// Extract class names.  Matches `class ClassName` and `class ClassName extends Base`.
fn extract_classes(source: &str) -> Vec<String> {
    let re = Regex::new(r"(?m)^class\s+(\w+)").expect("valid regex");
    re.captures_iter(source).map(|c| c[1].to_string()).collect()
}

/// Extract state variable names from `state:` blocks.
///
/// Handles indented declarations like `    string balance` inside a `state:` block.
fn extract_state_vars(source: &str) -> Vec<String> {
    let decl_re = Regex::new(r"^[ \t]*(void|integer|number|string|boolean|[\w<>\[\]]+)\s+(\w+)")
        .expect("valid regex");

    let mut vars = Vec::new();
    let mut in_state = false;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == "state:" {
            in_state = true;
            continue;
        }
        if in_state {
            if !line.starts_with('\t') && !line.starts_with("    ") && !trimmed.is_empty() {
                in_state = false;
                continue;
            }
            if let Some(cap) = decl_re.captures(line) {
                let name = cap[2].to_string();
                let reserved = ["if", "while", "for", "return"];
                if !reserved.contains(&name.as_str()) {
                    vars.push(name);
                }
            }
        }
    }

    vars
}

// ============================================================================
// Frontmatter Parsing Helpers
// ============================================================================

/// Extract the YAML frontmatter string from a Markdown document.
///
/// The frontmatter is everything between the first `---` line and the second
/// `---` line.  Returns `None` if the document does not begin with `---`.
fn extract_frontmatter(content: &str) -> Option<String> {
    let mut lines = content.lines();

    // First line must be `---`
    if lines.next()?.trim() != "---" {
        return None;
    }

    let mut fm_lines = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return Some(fm_lines.join("\n"));
        }
        fm_lines.push(line.to_string());
    }

    // Reached EOF without closing ---; still return what we have
    if !fm_lines.is_empty() {
        Some(fm_lines.join("\n"))
    } else {
        None
    }
}

/// Extract a single scalar value for a given key from the frontmatter block.
///
/// Handles both `key: value` and `key: "value"` (quoted) forms.
fn extract_scalar_field(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{}:", key);
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&prefix) {
            let rest = trimmed[prefix.len()..].trim();
            // Strip surrounding quotes if present
            let value = rest
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(rest)
                .trim()
                .to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// Extract the `signatures:` key-value block from the frontmatter.
///
/// Format expected:
/// ```yaml
/// signatures:
///   functionName: "return_type functionName(params)"
///   ClassName: "ClassName"
/// ```
///
/// Returns an empty map if the key is absent or has no entries.
fn extract_signatures_block(frontmatter: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut in_signatures = false;

    // Regex for indented key: "value" or key: value lines
    let entry_re = Regex::new(r#"^[ \t]+(\w+)\s*:\s*"?([^"]*)"?\s*$"#).expect("valid regex");

    for line in frontmatter.lines() {
        let trimmed = line.trim();

        if trimmed == "signatures:" {
            in_signatures = true;
            continue;
        }

        if in_signatures {
            // A non-indented non-empty key (without leading spaces/tabs) ends the block
            if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.is_empty() {
                in_signatures = false;
                continue;
            }

            if let Some(cap) = entry_re.captures(line) {
                let key = cap[1].to_string();
                let value = cap[2].trim().to_string();
                result.insert(key, value);
            }
        }
    }

    result
}

/// Rewrite the `signatures:` block in a Markdown document's frontmatter.
///
/// The document body and all other frontmatter keys are left untouched.
/// If there is no existing `signatures:` block, one is inserted immediately
/// before the closing `---`.
fn rewrite_signatures_block(content: &str, signatures: &HashMap<String, String>) -> String {
    // Build the replacement signatures block lines
    let mut sig_block = vec!["signatures:".to_string()];
    let mut sorted_keys: Vec<&String> = signatures.keys().collect();
    sorted_keys.sort();
    for key in sorted_keys {
        sig_block.push(format!("  {}: \"{}\"", key, signatures[key]));
    }

    let lines: Vec<&str> = content.lines().collect();

    // Locate frontmatter boundaries
    if lines.is_empty() || lines[0].trim() != "---" {
        // No frontmatter — prepend it
        let mut out = vec!["---".to_string()];
        out.extend(sig_block);
        out.push("---".to_string());
        out.push(content.to_string());
        return out.join("\n");
    }

    let closing_idx = lines[1..]
        .iter()
        .position(|l| l.trim() == "---")
        .map(|i| i + 1); // offset by 1 because we sliced from [1..]

    let closing_idx = match closing_idx {
        Some(i) => i,
        None => {
            // Malformed frontmatter — append signatures at end and re-close
            let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            out.extend(sig_block);
            out.push("---".to_string());
            return out.join("\n");
        }
    };

    // Collect frontmatter lines, skipping any existing signatures: block
    let mut new_fm: Vec<String> = Vec::new();
    let mut skip_sigs = false;
    for line in &lines[1..closing_idx] {
        let trimmed = line.trim();
        if trimmed == "signatures:" {
            skip_sigs = true;
            continue;
        }
        if skip_sigs {
            // Skip indented lines that belong to the old signatures block
            if line.starts_with(' ') || line.starts_with('\t') {
                continue;
            }
            skip_sigs = false;
        }
        new_fm.push(line.to_string());
    }

    // Append the new signatures block
    new_fm.extend(sig_block);

    // Reconstruct the full document
    let mut out = vec!["---".to_string()];
    out.extend(new_fm);
    out.push("---".to_string());
    // Body (everything after the closing ---)
    for line in &lines[closing_idx + 1..] {
        out.push(line.to_string());
    }

    out.join("\n")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_frontmatter_present() {
        let doc = "---\nfeature: banking\ndoc: functions/transfer\n---\n# Body\n";
        let fm = extract_frontmatter(doc);
        assert!(fm.is_some());
        let fm = fm.unwrap();
        assert!(fm.contains("feature: banking"));
    }

    #[test]
    fn test_extract_frontmatter_absent() {
        let doc = "# No frontmatter\nJust content\n";
        assert!(extract_frontmatter(doc).is_none());
    }

    #[test]
    fn test_extract_scalar_field() {
        let fm = "feature: banking\ndoc: functions/transfer, classes/Account\n";
        assert_eq!(
            extract_scalar_field(fm, "feature"),
            Some("banking".to_string())
        );
    }

    #[test]
    fn test_extract_scalar_field_quoted() {
        let fm = "feature: \"my feature\"\n";
        assert_eq!(
            extract_scalar_field(fm, "feature"),
            Some("my feature".to_string())
        );
    }

    #[test]
    fn test_extract_signatures_block() {
        let fm = "feature: test\nsignatures:\n  transfer: \"void transfer(Account from, Account to)\"\n  Account: \"Account\"\n";
        let sigs = extract_signatures_block(fm);
        assert_eq!(
            sigs.get("transfer").map(String::as_str),
            Some("void transfer(Account from, Account to)")
        );
        assert_eq!(sigs.get("Account").map(String::as_str), Some("Account"));
    }

    #[test]
    fn test_doc_ref_parse() {
        assert_eq!(
            DocRef::parse("functions/transfer"),
            Some(DocRef::Function("transfer".to_string()))
        );
        assert_eq!(
            DocRef::parse("classes/Account"),
            Some(DocRef::Class("Account".to_string()))
        );
        assert_eq!(
            DocRef::parse("state/balance"),
            Some(DocRef::State("balance".to_string()))
        );
        assert_eq!(DocRef::parse("unknown/foo"), None);
    }

    #[test]
    fn test_doc_ref_as_ref_str() {
        assert_eq!(
            DocRef::Function("transfer".to_string()).as_ref_str(),
            "functions/transfer"
        );
    }

    #[test]
    fn test_validate_broken_ref() {
        let engine = DocSyncEngine::new(PathBuf::from("/tmp/docs"));
        let spec = FeatureSpec {
            path: PathBuf::from("/tmp/docs/banking.md"),
            feature: "banking".to_string(),
            doc_refs: vec![DocRef::Function("transfer".to_string())],
            stored_signatures: HashMap::new(),
        };
        let symbols = AvailableSymbols::default();
        let result = engine.validate(&spec, &symbols);
        assert_eq!(result.broken_refs.len(), 1);
        assert!(result.stale_refs.is_empty());
    }

    #[test]
    fn test_validate_stale_ref() {
        let engine = DocSyncEngine::new(PathBuf::from("/tmp/docs"));
        let mut stored = HashMap::new();
        stored.insert(
            "transfer".to_string(),
            "void transfer(Account from)".to_string(),
        );
        let spec = FeatureSpec {
            path: PathBuf::from("/tmp/docs/banking.md"),
            feature: "banking".to_string(),
            doc_refs: vec![DocRef::Function("transfer".to_string())],
            stored_signatures: stored,
        };
        let symbols = AvailableSymbols {
            functions: vec![FunctionSig {
                name: "transfer".to_string(),
                signature: "void transfer(Account from, Account to, number amount)".to_string(),
            }],
            classes: vec![],
            state_vars: vec![],
        };
        let result = engine.validate(&spec, &symbols);
        assert!(result.broken_refs.is_empty());
        assert_eq!(result.stale_refs.len(), 1);
        assert_eq!(result.stale_refs[0].symbol_name, "transfer");
    }

    #[test]
    fn test_validate_clean() {
        let engine = DocSyncEngine::new(PathBuf::from("/tmp/docs"));
        let mut stored = HashMap::new();
        stored.insert(
            "transfer".to_string(),
            "void transfer(Account from, Account to)".to_string(),
        );
        let spec = FeatureSpec {
            path: PathBuf::from("/tmp/docs/banking.md"),
            feature: "banking".to_string(),
            doc_refs: vec![DocRef::Function("transfer".to_string())],
            stored_signatures: stored,
        };
        let symbols = AvailableSymbols {
            functions: vec![FunctionSig {
                name: "transfer".to_string(),
                signature: "void transfer(Account from, Account to)".to_string(),
            }],
            classes: vec![],
            state_vars: vec![],
        };
        let result = engine.validate(&spec, &symbols);
        assert!(result.is_clean());
    }

    #[test]
    fn test_rewrite_signatures_block_existing() {
        let doc = "---\nfeature: banking\nsignatures:\n  old: \"old sig\"\n---\n# Body\n";
        let mut sigs = HashMap::new();
        sigs.insert(
            "transfer".to_string(),
            "void transfer(Account a)".to_string(),
        );
        let out = rewrite_signatures_block(doc, &sigs);
        assert!(out.contains("transfer: \"void transfer(Account a)\""));
        assert!(!out.contains("old: \"old sig\""));
        assert!(out.contains("feature: banking"));
        assert!(out.contains("# Body"));
    }

    #[test]
    fn test_rewrite_signatures_block_no_existing() {
        let doc = "---\nfeature: banking\n---\n# Body\n";
        let mut sigs = HashMap::new();
        sigs.insert(
            "deposit".to_string(),
            "void deposit(number amount)".to_string(),
        );
        let out = rewrite_signatures_block(doc, &sigs);
        assert!(out.contains("signatures:"));
        assert!(out.contains("deposit: \"void deposit(number amount)\""));
    }

    #[test]
    fn test_extract_classes() {
        let source = "class BankAccount:\n\tinteger balance\nclass Transaction:\n";
        let classes = extract_classes(source);
        assert!(classes.contains(&"BankAccount".to_string()));
        assert!(classes.contains(&"Transaction".to_string()));
    }

    #[test]
    fn test_extract_state_vars() {
        let source = "state:\n\tinteger balance\n\tstring owner\n\nstart:\n\tprint(balance)\n";
        let vars = extract_state_vars(source);
        assert!(vars.contains(&"balance".to_string()));
        assert!(vars.contains(&"owner".to_string()));
    }
}
