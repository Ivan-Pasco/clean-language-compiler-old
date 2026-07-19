/// Plugin Contract 5 — Lint Extension host-side subsystem.
///
/// Implements the host side of the lint ABI described in
/// `foundation/spec/framework/contracts/lint-extension.md`.
///
/// Layout:
///   - `arena`    — single-call `LintArena` holding a snapshot of the
///                  pre-expansion `Program`, the monotonic `handle`, and the
///                  4 accessor methods that produce the JSON payloads
///                  described in §4.
///   - `bridges`  — 4 WASM host functions (`_ast_list_classes`,
///                  `_ast_class_fields`, `_ast_list_functions`,
///                  `_ast_list_blocks`) registered onto a dedicated lint
///                  linker in `wasm_adapter.rs`.
///   - `pass`     — `run_lint_pass` orchestrates one lint pass over all
///                  plugins in the registry that declare `[exports].lint`,
///                  parses their JSON output into `LintDiagnostic`s, and
///                  enforces cross-plugin diagnostic code uniqueness.
pub(crate) mod arena;
pub(crate) mod bridges;

pub(crate) use arena::LintArena;
pub(crate) use bridges::register_lint_bridges;

use crate::ast::Program;
use crate::plugins::PluginRegistry;
use serde::Deserialize;
use std::collections::HashMap;

/// Structured diagnostic from a plugin's `lint_project` return per spec §3.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintDiagnostic {
    /// Which plugin produced this diagnostic. Set by the compiler on the
    /// return path so the LSP diagnostic source can be `<plugin-name>`
    /// per spec §7.
    pub plugin_name: String,
    /// Plugin-namespaced code, e.g. `FRAME-DATA-E026`. Uniqueness is
    /// enforced across plugins at pass time — a duplicate produces LINT003
    /// against the second plugin (per §5).
    pub code: String,
    /// `error`, `warning`, or `info`. Any other value causes the entry to
    /// be dropped and summarised in LINT002.
    pub severity: LintSeverity,
    /// One-line message. ≤160 chars is recommended by the spec but not
    /// enforced — we render whatever the plugin sent.
    pub message: String,
    /// Source location (file, line, column). All 3 are required per §3.2.
    pub location: LintLocation,
    /// Optional follow-up hint. Rendered on a second line by the compiler
    /// renderer.
    pub hint: Option<String>,
}

/// Severity of a `LintDiagnostic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity {
    Error,
    Warning,
    Info,
}

impl LintSeverity {
    /// Parse the JSON string field. Returns `None` for unknown values so
    /// the entry can be dropped and summarised in LINT002.
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "error" => Some(LintSeverity::Error),
            "warning" => Some(LintSeverity::Warning),
            "info" => Some(LintSeverity::Info),
            _ => None,
        }
    }
}

/// Source location attached to a `LintDiagnostic`. `line` and `column` are
/// 1-based per spec §3.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

/// One plugin's contribution to a lint pass.
///
/// Cycle 3: raw JSON is parsed into structured diagnostics before returning
/// to callers, so the CLI can render each entry through the compiler's
/// diagnostic renderer. The `raw_json` field is preserved for observability
/// but is not the rendering surface.
#[derive(Debug, Clone)]
pub enum PluginLintReport {
    /// Plugin ran successfully. `diagnostics` is the parsed list per §3.2;
    /// `invalid_summary` names any entries dropped during parsing (LINT002
    /// per §5). `raw_json` is the untouched string the plugin wrote.
    Ok {
        plugin_name: String,
        raw_json: String,
        diagnostics: Vec<LintDiagnostic>,
        invalid_summary: Vec<String>,
    },
    /// Plugin trapped, timed out, returned malformed JSON, or returned an
    /// error object per §3.3. The compiler emits a `LINT001` diagnostic
    /// naming the plugin and continues — the build is never blocked.
    Failed {
        plugin_name: String,
        message: String,
    },
    /// Emitted alongside a successful report when the plugin's code
    /// collides with a code previously seen from another plugin (§5).
    /// Rendered as a compiler-side `LINT003` warning against the second
    /// plugin. This is *additive* — the plugin's valid diagnostics still
    /// render.
    CodeCollision {
        plugin_name: String,
        conflicting_code: String,
        first_seen_from: String,
    },
}

impl PluginLintReport {
    /// The plugin name attached to this report (regardless of variant).
    pub fn plugin_name(&self) -> &str {
        match self {
            PluginLintReport::Ok { plugin_name, .. } => plugin_name,
            PluginLintReport::Failed { plugin_name, .. } => plugin_name,
            PluginLintReport::CodeCollision { plugin_name, .. } => plugin_name,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON parsing (§3.2, §3.3)
// ─────────────────────────────────────────────────────────────────────────────

/// Raw shape of one diagnostic entry as sent by the plugin. Matches §3.2.
///
/// All fields are validated on the compiler side; unknown severity, missing
/// message, or malformed location cause the entry to be dropped (and
/// summarised in LINT002).
#[derive(Debug, Deserialize)]
struct RawDiagnosticEntry {
    code: Option<String>,
    severity: Option<String>,
    message: Option<String>,
    location: Option<RawLocation>,
    hint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLocation {
    file: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
}

/// Shape of the §3.3 plugin-error object.
#[derive(Debug, Deserialize)]
struct RawPluginError {
    error: String,
    #[allow(dead_code)]
    plugin: Option<String>,
}

/// Result of parsing a plugin's `lint_project` JSON output.
///
/// `Diagnostics` is the happy path — a JSON array per §3.2, potentially
/// with some invalid entries dropped and summarised.
///
/// `PluginError` is the §3.3 self-reported failure. The caller treats it
/// as a LINT001 against the plugin.
///
/// `Unparseable` covers "we couldn't even decode the JSON" — again LINT001.
enum ParseOutcome {
    Diagnostics {
        entries: Vec<LintDiagnostic>,
        invalid_summary: Vec<String>,
    },
    PluginError(String),
    Unparseable(String),
}

/// Parse a plugin's lint output per spec §3.2 / §3.3.
///
/// Attempts, in order:
///   1. JSON array of diagnostic entries (§3.2).
///   2. JSON object `{"error": "...", "plugin": "..."}` (§3.3).
///   3. Neither — report as unparseable.
fn parse_diagnostic_json(plugin_name: &str, raw_json: &str) -> ParseOutcome {
    let trimmed = raw_json.trim();
    // Empty return (spec: no diagnostics) — this path is handled upstream
    // by `call_lint_project` returning `String::new()`, but we tolerate it
    // here too for defensive symmetry.
    if trimmed.is_empty() {
        return ParseOutcome::Diagnostics {
            entries: Vec::new(),
            invalid_summary: Vec::new(),
        };
    }

    // §3.2 shape: JSON array.
    if trimmed.starts_with('[') {
        match serde_json::from_str::<Vec<RawDiagnosticEntry>>(trimmed) {
            Ok(raws) => {
                let (entries, invalid) = collect_valid_entries(plugin_name, raws);
                return ParseOutcome::Diagnostics {
                    entries,
                    invalid_summary: invalid,
                };
            }
            Err(e) => {
                return ParseOutcome::Unparseable(format!(
                    "diagnostic array failed to deserialize: {}",
                    e
                ));
            }
        }
    }

    // §3.3 shape: JSON error object.
    if trimmed.starts_with('{') {
        match serde_json::from_str::<RawPluginError>(trimmed) {
            Ok(err) => {
                return ParseOutcome::PluginError(err.error);
            }
            Err(e) => {
                return ParseOutcome::Unparseable(format!(
                    "expected diagnostic array or error object, got malformed object: {}",
                    e
                ));
            }
        }
    }

    ParseOutcome::Unparseable(format!(
        "plugin `{}` returned JSON that is neither an array nor an object",
        plugin_name
    ))
}

/// Validate each raw entry, keeping the good ones and summarising the bad
/// ones. Returns `(valid, invalid_summary)`.
fn collect_valid_entries(
    plugin_name: &str,
    raws: Vec<RawDiagnosticEntry>,
) -> (Vec<LintDiagnostic>, Vec<String>) {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for (idx, r) in raws.into_iter().enumerate() {
        let code = match r.code {
            Some(c) if !c.is_empty() => c,
            _ => {
                invalid.push(format!("entry {}: missing or empty `code`", idx));
                continue;
            }
        };
        let severity = match r.severity.as_deref().and_then(LintSeverity::from_str) {
            Some(s) => s,
            None => {
                invalid.push(format!(
                    "entry {} ({}): unknown severity `{}`",
                    idx,
                    code,
                    r.severity.as_deref().unwrap_or("<missing>")
                ));
                continue;
            }
        };
        let message = match r.message {
            Some(m) if !m.is_empty() => m,
            _ => {
                invalid.push(format!(
                    "entry {} ({}): missing or empty `message`",
                    idx, code
                ));
                continue;
            }
        };
        let location = match r.location {
            Some(RawLocation {
                file: Some(file),
                line: Some(line),
                column: Some(column),
            }) if !file.is_empty() => LintLocation { file, line, column },
            _ => {
                invalid.push(format!(
                    "entry {} ({}): missing or incomplete `location` (need file, line, column)",
                    idx, code
                ));
                continue;
            }
        };
        valid.push(LintDiagnostic {
            plugin_name: plugin_name.to_string(),
            code,
            severity,
            message,
            location,
            hint: r.hint,
        });
    }
    (valid, invalid)
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass orchestration + cross-plugin code uniqueness (LINT003)
// ─────────────────────────────────────────────────────────────────────────────

/// Run the Contract 5 lint pass over all plugins in the registry that
/// declare `[exports].lint`. Returns one or more `PluginLintReport` per
/// participating plugin: an `Ok` with parsed diagnostics and any LINT002
/// summary, optionally followed by a `CodeCollision` per code that
/// conflicts with an earlier plugin's diagnostic (LINT003).
///
/// `pre_expansion` MUST be the AST snapshot taken *before* plugin block
/// expansion. See `foundation/spec/framework/contracts/lint-extension.md`
/// §6 for the ordering requirement.
///
/// `config_level` is `"error"`, `"warning"`, or `"info"` — surfaced to
/// plugins in the context JSON per §3.1.
pub(crate) fn run_lint_pass(
    registry: &PluginRegistry,
    pre_expansion: &Program,
    config_level: &str,
) -> Vec<PluginLintReport> {
    let mut per_plugin: Vec<(String, Result<String, String>)> = Vec::new();
    let mut seen_plugins: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for plugin in registry.iter_unique_plugins() {
        let name = plugin.name();
        if !seen_plugins.insert(name) {
            continue;
        }
        match plugin.lint_project(pre_expansion, config_level) {
            Ok(None) => continue,
            Ok(Some(raw_json)) => per_plugin.push((name.to_string(), Ok(raw_json))),
            Err(err) => per_plugin.push((name.to_string(), Err(err.to_string()))),
        }
    }

    assemble_reports(per_plugin)
}

/// Deterministic pure-function core of the pass: turn per-plugin raw JSON
/// (or WASM-side failures) into the final `Vec<PluginLintReport>`.
///
/// Split out from `run_lint_pass` so LINT002 / LINT003 / §3.3 handling
/// can be unit-tested without a real `PluginRegistry` or WASM instance.
///
/// Ordering rule: for each plugin, an `Ok` report is emitted immediately,
/// followed by zero-or-more `CodeCollision` reports for diagnostics whose
/// code was already claimed by an earlier plugin. This matches spec §5's
/// "render the diagnostic; emit LINT003 against the second plugin only".
fn assemble_reports(per_plugin: Vec<(String, Result<String, String>)>) -> Vec<PluginLintReport> {
    let mut reports: Vec<PluginLintReport> = Vec::new();
    let mut first_source_of_code: HashMap<String, String> = HashMap::new();

    for (name, outcome) in per_plugin {
        match outcome {
            Ok(raw_json) => match parse_diagnostic_json(&name, &raw_json) {
                ParseOutcome::Diagnostics {
                    entries,
                    invalid_summary,
                } => {
                    let mut collisions: Vec<PluginLintReport> = Vec::new();
                    for d in &entries {
                        match first_source_of_code.get(&d.code) {
                            Some(first) if first != &name => {
                                collisions.push(PluginLintReport::CodeCollision {
                                    plugin_name: name.clone(),
                                    conflicting_code: d.code.clone(),
                                    first_seen_from: first.clone(),
                                });
                            }
                            None => {
                                first_source_of_code.insert(d.code.clone(), name.clone());
                            }
                            _ => {}
                        }
                    }
                    reports.push(PluginLintReport::Ok {
                        plugin_name: name.clone(),
                        raw_json,
                        diagnostics: entries,
                        invalid_summary,
                    });
                    reports.extend(collisions);
                }
                ParseOutcome::PluginError(msg) => reports.push(PluginLintReport::Failed {
                    plugin_name: name.clone(),
                    message: format!("plugin self-reported error: {}", msg),
                }),
                ParseOutcome::Unparseable(msg) => reports.push(PluginLintReport::Failed {
                    plugin_name: name.clone(),
                    message: msg,
                }),
            },
            Err(err_msg) => reports.push(PluginLintReport::Failed {
                plugin_name: name.clone(),
                message: err_msg,
            }),
        }
    }

    reports
}

// ────────────────────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_array_of_diagnostics() {
        let raw = r#"[
            {
                "code": "FRAME-DATA-E026",
                "severity": "error",
                "message": "field mismatch",
                "location": {"file": "app/data.cln", "line": 47, "column": 9},
                "hint": "add the column"
            },
            {
                "code": "FRAME-DATA-I002",
                "severity": "info",
                "message": "unused data class",
                "location": {"file": "app/data.cln", "line": 88, "column": 1}
            }
        ]"#;
        let outcome = parse_diagnostic_json("frame.data", raw);
        match outcome {
            ParseOutcome::Diagnostics {
                entries,
                invalid_summary,
            } => {
                assert_eq!(entries.len(), 2);
                assert!(invalid_summary.is_empty());
                assert_eq!(entries[0].code, "FRAME-DATA-E026");
                assert_eq!(entries[0].severity, LintSeverity::Error);
                assert_eq!(entries[0].hint.as_deref(), Some("add the column"));
                assert_eq!(entries[0].plugin_name, "frame.data");
                assert_eq!(entries[1].severity, LintSeverity::Info);
                assert_eq!(entries[1].hint, None);
            }
            other => panic!(
                "expected Diagnostics, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn drops_partial_invalid_entries_and_keeps_valid() {
        let raw = r#"[
            {"code": "E1", "severity": "error", "message": "ok", "location": {"file":"a","line":1,"column":1}},
            {"code": "E2", "severity": "shouty", "message": "unknown severity", "location": {"file":"a","line":2,"column":1}},
            {"code": "", "severity": "warning", "message": "empty code", "location": {"file":"a","line":3,"column":1}},
            {"code": "E4", "severity": "warning", "message": "", "location": {"file":"a","line":4,"column":1}},
            {"code": "E5", "severity": "info", "message": "no location"}
        ]"#;
        let outcome = parse_diagnostic_json("plug", raw);
        match outcome {
            ParseOutcome::Diagnostics {
                entries,
                invalid_summary,
            } => {
                assert_eq!(entries.len(), 1, "only E1 is valid");
                assert_eq!(entries[0].code, "E1");
                assert_eq!(invalid_summary.len(), 4);
                assert!(invalid_summary
                    .iter()
                    .any(|s| s.contains("unknown severity")));
                assert!(invalid_summary
                    .iter()
                    .any(|s| s.contains("missing or empty `code`")));
                assert!(invalid_summary
                    .iter()
                    .any(|s| s.contains("missing or empty `message`")));
                assert!(invalid_summary
                    .iter()
                    .any(|s| s.contains("missing or incomplete `location`")));
            }
            other => panic!(
                "expected Diagnostics, got {:?}",
                std::mem::discriminant(&other)
            ),
        }
    }

    #[test]
    fn parses_plugin_error_object() {
        let raw = r#"{"error": "internal panic X", "plugin": "frame.data"}"#;
        match parse_diagnostic_json("frame.data", raw) {
            ParseOutcome::PluginError(msg) => assert_eq!(msg, "internal panic X"),
            _ => panic!("expected PluginError"),
        }
    }

    #[test]
    fn malformed_json_is_unparseable() {
        let raw = "not json at all";
        match parse_diagnostic_json("plug", raw) {
            ParseOutcome::Unparseable(_) => (),
            _ => panic!("expected Unparseable"),
        }
    }

    #[test]
    fn empty_string_produces_empty_diagnostics() {
        match parse_diagnostic_json("plug", "") {
            ParseOutcome::Diagnostics {
                entries,
                invalid_summary,
            } => {
                assert!(entries.is_empty());
                assert!(invalid_summary.is_empty());
            }
            _ => panic!("empty string should be no-op success"),
        }
    }

    #[test]
    fn severity_from_str_rejects_unknown() {
        assert_eq!(LintSeverity::from_str("error"), Some(LintSeverity::Error));
        assert_eq!(
            LintSeverity::from_str("warning"),
            Some(LintSeverity::Warning)
        );
        assert_eq!(LintSeverity::from_str("info"), Some(LintSeverity::Info));
        assert_eq!(LintSeverity::from_str("Error"), None, "case-sensitive");
        assert_eq!(LintSeverity::from_str(""), None);
        assert_eq!(LintSeverity::from_str("critical"), None);
    }

    // ── assemble_reports tests (§5 LINT001/LINT002/LINT003) ──────────────────

    /// Build a minimal JSON-array payload with one diagnostic entry.
    fn one_diag(code: &str) -> String {
        format!(
            r#"[{{"code":"{}","severity":"error","message":"m","location":{{"file":"f","line":1,"column":1}}}}]"#,
            code
        )
    }

    #[test]
    fn assemble_reports_lint003_fires_only_on_second_plugin() {
        // Plugin A emits code X first; plugin B emits X too → LINT003 on B.
        let per_plugin = vec![
            ("plugin.a".to_string(), Ok(one_diag("SHARED-01"))),
            ("plugin.b".to_string(), Ok(one_diag("SHARED-01"))),
        ];
        let reports = assemble_reports(per_plugin);
        assert_eq!(
            reports.len(),
            3,
            "one Ok per plugin + one CodeCollision on B"
        );
        match &reports[0] {
            PluginLintReport::Ok { plugin_name, .. } => assert_eq!(plugin_name, "plugin.a"),
            _ => panic!("first report must be Ok from plugin.a"),
        }
        match &reports[1] {
            PluginLintReport::Ok {
                plugin_name,
                diagnostics,
                ..
            } => {
                assert_eq!(plugin_name, "plugin.b");
                assert_eq!(
                    diagnostics.len(),
                    1,
                    "B's diagnostic still renders even on collision"
                );
            }
            _ => panic!("second report must be Ok from plugin.b"),
        }
        match &reports[2] {
            PluginLintReport::CodeCollision {
                plugin_name,
                conflicting_code,
                first_seen_from,
            } => {
                assert_eq!(
                    plugin_name, "plugin.b",
                    "LINT003 attaches to the SECOND plugin"
                );
                assert_eq!(conflicting_code, "SHARED-01");
                assert_eq!(first_seen_from, "plugin.a");
            }
            _ => panic!("third report must be CodeCollision on plugin.b"),
        }
    }

    #[test]
    fn assemble_reports_lint003_not_emitted_for_same_plugin_dup_code() {
        // A plugin emitting the same code twice is not a cross-plugin
        // collision — LINT003 is scoped to CROSS-plugin dedup only.
        let dup_payload = r#"[
            {"code":"X","severity":"error","message":"first","location":{"file":"f","line":1,"column":1}},
            {"code":"X","severity":"error","message":"second","location":{"file":"f","line":2,"column":1}}
        ]"#;
        let per_plugin = vec![("plugin.a".to_string(), Ok(dup_payload.to_string()))];
        let reports = assemble_reports(per_plugin);
        assert_eq!(
            reports.len(),
            1,
            "only the Ok report, no LINT003 from self-collision"
        );
        match &reports[0] {
            PluginLintReport::Ok { diagnostics, .. } => assert_eq!(diagnostics.len(), 2),
            _ => panic!("expected Ok"),
        }
    }

    #[test]
    fn assemble_reports_unparseable_becomes_lint001_failed() {
        let per_plugin = vec![("plugin.a".to_string(), Ok("not json".to_string()))];
        let reports = assemble_reports(per_plugin);
        assert_eq!(reports.len(), 1);
        match &reports[0] {
            PluginLintReport::Failed { plugin_name, .. } => assert_eq!(plugin_name, "plugin.a"),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn assemble_reports_error_object_becomes_lint001_failed() {
        // §3.3: plugin self-reported error → LINT001 against the plugin.
        let per_plugin = vec![(
            "plugin.a".to_string(),
            Ok(r#"{"error":"internal panic","plugin":"plugin.a"}"#.to_string()),
        )];
        let reports = assemble_reports(per_plugin);
        assert_eq!(reports.len(), 1);
        match &reports[0] {
            PluginLintReport::Failed {
                plugin_name,
                message,
            } => {
                assert_eq!(plugin_name, "plugin.a");
                assert!(message.contains("internal panic"));
            }
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn assemble_reports_partial_invalid_becomes_lint002_summary() {
        // §5: valid entries render; invalid ones summarised via
        // `invalid_summary` (which the CLI later prints as LINT002).
        let mixed = r#"[
            {"code":"E1","severity":"error","message":"ok","location":{"file":"f","line":1,"column":1}},
            {"code":"E2","severity":"shouty","message":"nope","location":{"file":"f","line":2,"column":1}}
        ]"#;
        let per_plugin = vec![("plugin.a".to_string(), Ok(mixed.to_string()))];
        let reports = assemble_reports(per_plugin);
        match &reports[0] {
            PluginLintReport::Ok {
                diagnostics,
                invalid_summary,
                ..
            } => {
                assert_eq!(diagnostics.len(), 1, "only E1 valid");
                assert_eq!(invalid_summary.len(), 1);
                assert!(invalid_summary[0].contains("unknown severity"));
            }
            _ => panic!("expected Ok"),
        }
    }

    #[test]
    fn assemble_reports_preserves_wasm_side_failures() {
        let per_plugin = vec![
            (
                "plugin.a".to_string(),
                Err("wasm trap: interrupt".to_string()),
            ),
            ("plugin.b".to_string(), Ok(one_diag("OK-01"))),
        ];
        let reports = assemble_reports(per_plugin);
        assert_eq!(reports.len(), 2);
        match &reports[0] {
            PluginLintReport::Failed {
                plugin_name,
                message,
            } => {
                assert_eq!(plugin_name, "plugin.a");
                assert!(message.contains("interrupt"));
            }
            _ => panic!("first must be Failed on plugin.a"),
        }
        match &reports[1] {
            PluginLintReport::Ok { plugin_name, .. } => assert_eq!(plugin_name, "plugin.b"),
            _ => panic!("second must be Ok from plugin.b (build not blocked)"),
        }
    }
}
