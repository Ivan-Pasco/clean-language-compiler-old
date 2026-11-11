/*!
 * Local error recording and bug-report drafting.
 *
 * Nothing in this module talks to the network. Bugs in the old Clean
 * Language toolchain are filed as GitHub issues (label `compiler-old`) in the
 * repository that owns the fix; this module prepares those issues and, while
 * the platform authors work inside a component source tree, records
 * failures in a local dev queue.
 *
 * # Architecture
 *
 * - `config`: the `telemetry` on/off switch in ~/.cleen/telemetry/config.toml
 * - `dev_context`: classifies a run as dev work (component source tree,
 *   `target/` build) or a normal user run
 * - `dev_queue`: the local dev-mode error queue (`cln dev-queue`)
 * - [`IssueDraft`]: the `gh issue create` command for a bug report
 */

pub mod config;
pub mod dev_context;
pub mod dev_queue;

pub use config::TelemetryConfig;
pub use dev_context::{
    detect as detect_dev_context, detect_for_component as detect_dev_context_for_component,
    DevContext,
};

/// Components that own a GitHub repository for the old toolchain, with the
/// repository that bug reports for them go to.
const ISSUE_REPOS: &[(&str, &str)] = &[
    ("compiler", "Ivan-Pasco/clean-language-compiler-old"),
    ("server", "Ivan-Pasco/clean-server-old"),
    ("framework", "Ivan-Pasco/cleen-framework-old"),
    ("manager", "Ivan-Pasco/clean-language-manager-old"),
];

/// Map a component name (including the aliases used in bug reports, such as
/// `parser`, `node-server` or a `frame.*` plugin name) to one of the
/// components in [`ISSUE_REPOS`]. `None` when the name is not recognised.
pub fn normalize_component(component: &str) -> Option<&'static str> {
    let normalized = match component {
        "compiler" | "parser" | "semantic" | "codegen" | "cli" | "syntax" | "system" | "mcp"
        | "unknown" => "compiler",
        "server" | "node-server" | "runtime" => "server",
        "framework" | "plugin" | "ui" | "canvas" => "framework",
        c if c.starts_with("frame.") => "framework",
        "manager" => "manager",
        _ => return None,
    };
    Some(normalized)
}

/// The component names accepted by [`normalize_component`] as canonical names.
pub fn issue_components() -> impl Iterator<Item = &'static str> {
    ISSUE_REPOS.iter().map(|(component, _)| *component)
}

/// A bug report ready to be filed as a GitHub issue.
#[derive(Debug, Clone)]
pub struct IssueDraft {
    /// Canonical component (one of [`issue_components`]).
    pub component: &'static str,
    pub title: String,
    pub body: String,
}

impl IssueDraft {
    /// Draft an issue for `component` (any name [`normalize_component`]
    /// accepts; unknown names go to the compiler repository).
    pub fn new(component: &str, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            component: normalize_component(component).unwrap_or("compiler"),
            title: title.into(),
            body: body.into(),
        }
    }

    /// `owner/name` of the repository that owns the fix.
    pub fn repo(&self) -> &'static str {
        ISSUE_REPOS
            .iter()
            .find(|(component, _)| *component == self.component)
            .map(|(_, repo)| *repo)
            .unwrap_or(ISSUE_REPOS[0].1)
    }

    /// The exact shell command that files this issue.
    pub fn gh_command(&self) -> String {
        format!(
            "gh issue create --repo {} --label bug --label compiler-old --title {} --body {}",
            self.repo(),
            shell_quote(&self.title),
            shell_quote(&self.body)
        )
    }
}

/// Quote `text` as one POSIX shell word.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Environment lines included in every drafted issue.
pub fn environment_section() -> String {
    format!(
        "Compiler: cln {}\nOS: {} ({})",
        crate::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// Record a bug in the local dev queue when this run is dev work on
/// `component` (see [`dev_context`]). Returns the queue outcome, or `None`
/// for a normal user run, where nothing is stored.
pub fn record_in_dev_queue(
    component: &str,
    error_code: &str,
    message: &str,
    file_context: Option<&str>,
) -> Option<(DevContext, dev_queue::AppendOutcome)> {
    let ctx = detect_dev_context_for_component(component);
    if !ctx.is_dev() {
        return None;
    }
    let entry = dev_queue::entry_from(
        &ctx,
        error_code,
        component,
        message,
        file_context,
        crate::VERSION,
    );
    let outcome = dev_queue::append(entry);
    Some((ctx, outcome))
}

/// Handle a failed `cln compile` / `cln build` when `telemetry` is on.
///
/// Errors in the user's own code (syntax, types, missing files) are the
/// compiler working correctly and are ignored. A failure that points at a
/// compiler or plugin bug is recorded in the local dev queue when the run is
/// dev work; otherwise the GitHub issue command for it is printed to stderr.
/// Never prompts and never blocks the compile.
pub fn report_compile_failure(
    errors: &[crate::error::CompilerError],
    source_file: &str,
    source_content: Option<&str>,
) {
    if !TelemetryConfig::load().enabled {
        return;
    }

    // Take the first error as representative
    let Some(first_error) = errors.first() else {
        return;
    };
    if !is_likely_compiler_bug(first_error) {
        tracing::debug!(
            error_kind = ?std::mem::discriminant(first_error),
            "Not a compiler bug: error is in the user's code"
        );
        return;
    }

    let info = extract_error_info(first_error);

    // Dev-context gate: when the platform authors are iterating on Clean
    // Language itself (a `target/` build, or a source inside a component
    // tree), record the failure in the local dev queue. Env var
    // `CLEEN_TELEMETRY_FORCE=publish|local` overrides auto-detect.
    let dev_ctx = dev_context::detect(source_file);
    if dev_ctx.is_dev() {
        let entry = dev_queue::entry_from(
            &dev_ctx,
            &info.code,
            &info.component,
            &info.message,
            Some(source_file),
            crate::VERSION,
        );
        let outcome = dev_queue::append(entry);
        // One line on stderr right after the error itself.
        eprintln!(
            "[dev-queue] {} recorded locally ({} \u{00d7}{}) \u{2014} `cln dev-queue list`",
            info.code, outcome.fingerprint, outcome.occurrences
        );
        return;
    }

    // Only the file name: the full path is machine-specific.
    let file_hint = std::path::Path::new(source_file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(source_file);
    let snippet = source_content.and_then(|src| {
        extract_location_snippet(first_error, src).or_else(|| extract_file_head_snippet(src))
    });
    let mut body = format!(
        "{}\n\nFile: {}\nError {} ({}{}): {}\n\nExpected: {}\n",
        environment_section(),
        file_hint,
        info.code,
        info.category,
        info.subsystem
            .as_deref()
            .map(|s| format!(", {s}"))
            .unwrap_or_default(),
        info.message,
        infer_expected_behavior(first_error)
    );
    if let Some(snippet) = snippet {
        body.push_str(&format!(
            "\nSource around the error:\n```\n{snippet}\n```\n"
        ));
    }
    body.push_str("\nReplace the source excerpt with a minimal reproduction before filing.\n");
    let draft = IssueDraft::new(
        &info.component,
        format!(
            "{}: {}",
            info.code,
            crate::error::truncate_chars(&info.message, 80)
        ),
        body,
    );
    eprintln!(
        "\nThis looks like a bug in the Clean Language toolchain ({}), not in your code.\nTo report it, review and run:\n  {}",
        info.component,
        draft.gh_command()
    );
}

/// Fallback snippet: the first ~50 lines of the source. Used when the error
/// carries no line info but we still have the file content, so a repro is
/// present rather than nothing.
fn extract_file_head_snippet(source: &str) -> Option<String> {
    let lines: Vec<&str> = source.lines().take(50).collect();
    if lines.is_empty() {
        return None;
    }
    let snippet: String = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>3} | {}", i + 1, l))
        .collect::<Vec<_>>()
        .join("\n");
    Some(snippet)
}

/// Extract a few lines of context around the error location from the source.
/// Returns at most 5 lines centred on the error line — enough for reproduction
/// without sending the whole file.
fn extract_location_snippet(error: &crate::error::CompilerError, source: &str) -> Option<String> {
    // Pull the line number from the error context if available.
    let line_num: usize = match error {
        crate::error::CompilerError::Syntax { context }
        | crate::error::CompilerError::Type { context }
        | crate::error::CompilerError::Codegen { context }
        | crate::error::CompilerError::Runtime { context }
        | crate::error::CompilerError::Memory { context }
        | crate::error::CompilerError::IO { context }
        | crate::error::CompilerError::Validation { context }
        | crate::error::CompilerError::Module { context } => {
            // Use pre-attached snippet if already present
            if let Some(ref s) = context.source_snippet {
                return Some(s.clone());
            }
            context.location.as_ref()?.line
        }
        crate::error::CompilerError::PluginError { .. }
        | crate::error::CompilerError::LexError(_) => return None,
    };

    if line_num == 0 {
        return None;
    }

    let lines: Vec<&str> = source.lines().collect();
    let total = lines.len();
    // 2 lines of context before and after the error line (1-indexed → 0-indexed)
    let start = line_num.saturating_sub(3);
    let end = (line_num + 2).min(total);
    if start >= total {
        return None;
    }

    let snippet: String = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>3} | {}", start + i + 1, l))
        .collect::<Vec<_>>()
        .join("\n");

    Some(snippet)
}

/// Return a generic expected-behavior description for a compiler error type.
/// Used in auto-reports where no human writes the expected behavior explicitly.
fn infer_expected_behavior(error: &crate::error::CompilerError) -> &'static str {
    match error {
        crate::error::CompilerError::Codegen { .. } => {
            "Compiler should produce valid WebAssembly output without internal errors"
        }
        crate::error::CompilerError::Memory { .. } => {
            "Compiler should manage memory correctly during compilation"
        }
        crate::error::CompilerError::Validation { .. } => {
            "Compiler should complete semantic validation without internal failures"
        }
        crate::error::CompilerError::Module { .. } => {
            "Compiler should resolve module references and imports correctly"
        }
        crate::error::CompilerError::PluginError { .. } => {
            "Plugin should execute correctly and return valid generated code"
        }
        _ => "Compiler should process the input without internal errors",
    }
}

/// Classify whether a compiler error likely indicates a bug in the Clean
/// Language toolchain (compiler/plugin) vs. a correctness issue in the user's
/// source code. Only the former is appropriate for the bug dashboard.
///
/// Policy:
/// - `Codegen` / `Validation` / `Memory` — compiler-internal failures,
///   always treated as bugs.
/// - `PluginError` — a bug (in the plugin or the compiler that ran it),
///   except PLUGIN001: a plugin named in `plugins:` that is not installed
///   or cannot be loaded for this build is the project's configuration.
/// - Everything else — `Module` errors are imports that name a missing
///   module or form a cycle; syntax/type/lex errors are the compiler
///   correctly rejecting bad user code; IO errors are the user's
///   environment; runtime errors surface from executing user code.
fn is_likely_compiler_bug(error: &crate::error::CompilerError) -> bool {
    use crate::error::CompilerError::*;
    match error {
        Codegen { .. } | Validation { .. } | Memory { .. } => true,
        PluginError { message, .. } => !message.contains("PLUGIN001"),
        _ => false,
    }
}

/// Structured fields extracted from a CompilerError for reporting.
struct ErrorInfo {
    code: String,
    category: String,
    component: String,
    subsystem: Option<String>,
    message: String,
}

/// Map a plugin name to its owning component. Plugins in the `frame.*`
/// family live in the `clean-framework` repository, so their compiler-side
/// diagnostics should route to the `framework` component.
///
/// Unknown plugins fall back to `framework` on the assumption that any
/// third-party plugin is owned by whoever authored it — and the vast majority
/// of plugin-emitted code today comes from `frame.*`. If a specific mapping
/// is needed for other plugin families, add it here.
fn owning_component_for_plugin(plugin_name: &str) -> &'static str {
    if plugin_name.starts_with("frame.") {
        "framework"
    } else {
        // Conservative default — better than routing plugin-origin errors
        // back to `compiler` (which is what the bug fixes).
        "framework"
    }
}

/// If the error's `SourceLocation.file` carries a plugin-origin marker
/// (`<plugin:NAME>`), return the (component, subsystem) pair to override the
/// default classification with. Returns `None` for user-code locations,
/// which keeps the existing routing untouched.
///
/// Resolves `TELEMETRY-SYNTHETIC-ORIGIN-CLASSIFICATION` (fingerprint
/// f5f2a9954225): SEM007 and related validation errors emitted against
/// plugin-synthesized code previously misclassified as `component=compiler`
/// even though the responsibility for fixing them lies with the plugin.
fn component_override_from_location(
    location: Option<&crate::ast::SourceLocation>,
) -> Option<(String, Option<String>)> {
    let loc = location?;
    let plugin_name = crate::ast::plugin_name_from_origin_marker(&loc.file)?;
    let component = owning_component_for_plugin(plugin_name).to_string();
    let subsystem = Some(format!("plugin:{}", plugin_name));
    Some((component, subsystem))
}

/// Extract structured error info from a CompilerError.
/// Returns (code, category, component, subsystem, message).
fn extract_error_info(error: &crate::error::CompilerError) -> ErrorInfo {
    let mut info = match error {
        crate::error::CompilerError::Syntax { context } => ErrorInfo {
            code: context
                .error_code
                .as_deref()
                .unwrap_or("SYN000")
                .to_string(),
            category: "syntax".to_string(),
            component: "compiler".to_string(),
            subsystem: Some("parser".to_string()),
            message: context.message.clone(),
        },
        crate::error::CompilerError::Type { context } => ErrorInfo {
            code: context
                .error_code
                .as_deref()
                .unwrap_or("TYP000")
                .to_string(),
            category: "semantic".to_string(),
            component: "compiler".to_string(),
            subsystem: Some("typechecker".to_string()),
            message: context.message.clone(),
        },
        crate::error::CompilerError::Codegen { context } => ErrorInfo {
            code: context
                .error_code
                .as_deref()
                .unwrap_or("COD000")
                .to_string(),
            category: "codegen".to_string(),
            component: "compiler".to_string(),
            subsystem: Some("codegen".to_string()),
            message: context.message.clone(),
        },
        crate::error::CompilerError::Runtime { context } => ErrorInfo {
            code: context
                .error_code
                .as_deref()
                .unwrap_or("RUN000")
                .to_string(),
            category: "runtime".to_string(),
            component: "server".to_string(),
            subsystem: Some("runtime".to_string()),
            message: context.message.clone(),
        },
        crate::error::CompilerError::Memory { context } => ErrorInfo {
            code: context
                .error_code
                .as_deref()
                .unwrap_or("MEM000")
                .to_string(),
            category: "memory".to_string(),
            component: "compiler".to_string(),
            subsystem: Some("memory".to_string()),
            message: context.message.clone(),
        },
        crate::error::CompilerError::IO { context } => ErrorInfo {
            code: context.error_code.as_deref().unwrap_or("IO000").to_string(),
            category: "io".to_string(),
            component: "compiler".to_string(),
            subsystem: Some("io".to_string()),
            message: context.message.clone(),
        },
        crate::error::CompilerError::Validation { context } => ErrorInfo {
            code: context
                .error_code
                .as_deref()
                .unwrap_or("VAL000")
                .to_string(),
            category: "validation".to_string(),
            component: "compiler".to_string(),
            subsystem: Some("resolver".to_string()),
            message: context.message.clone(),
        },
        crate::error::CompilerError::Module { context } => ErrorInfo {
            code: context
                .error_code
                .as_deref()
                .unwrap_or("MOD000")
                .to_string(),
            category: "module".to_string(),
            component: "compiler".to_string(),
            subsystem: Some("module".to_string()),
            message: context.message.clone(),
        },
        crate::error::CompilerError::LexError(lex_err) => {
            use crate::lexer::specification_lexer::LexError;
            let code = match lex_err {
                LexError::InvalidCharacter { .. } => "LEX001",
                LexError::UnterminatedString { .. } => "LEX002",
                LexError::UnterminatedComment { .. } => "LEX003",
                LexError::InvalidNumber { .. } => "LEX004",
                LexError::InvalidIndentation { .. } => "LEX006",
                LexError::SpacesInIndentation { .. } => "LEX007",
            };
            ErrorInfo {
                code: code.to_string(),
                category: "syntax".to_string(),
                component: "compiler".to_string(),
                subsystem: Some("lexer".to_string()),
                message: format!("{}", lex_err),
            }
        }
        crate::error::CompilerError::PluginError { message, .. } => ErrorInfo {
            code: "PLG000".to_string(),
            category: "plugin".to_string(),
            component: "framework".to_string(),
            subsystem: Some("plugin".to_string()),
            message: message.clone(),
        },
    };
    // If the offending source location carries a plugin-origin marker
    // (`<plugin:NAME>`), override the component so the dashboard routes the
    // report to the plugin's owning component instead of `compiler`.
    // Only applies to error classes whose default component is `compiler` —
    // errors that were already correctly attributed (e.g. `PluginError`
    // routes to `framework`, `Runtime` to `server`) are left untouched.
    if info.component == "compiler" {
        if let Some((component, subsystem)) = component_override_from_location(location_of(error)) {
            info.component = component;
            info.subsystem = subsystem;
        }
    }
    info
}

/// Extract the `SourceLocation` from a `CompilerError`, when available.
fn location_of(error: &crate::error::CompilerError) -> Option<&crate::ast::SourceLocation> {
    use crate::error::CompilerError as E;
    match error {
        E::Syntax { context }
        | E::Type { context }
        | E::Codegen { context }
        | E::Runtime { context }
        | E::Memory { context }
        | E::IO { context }
        | E::Validation { context }
        | E::Module { context } => context.location.as_ref(),
        E::LexError(_) => None,
        E::PluginError { location, .. } => location.as_ref(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CompilerError;

    #[test]
    fn user_syntax_errors_are_not_bugs() {
        let err = CompilerError::syntax_error("unexpected token", None, None);
        assert!(!is_likely_compiler_bug(&err));
    }

    #[test]
    fn user_type_errors_are_not_bugs() {
        let err = CompilerError::type_error("type mismatch", None, None);
        assert!(!is_likely_compiler_bug(&err));
    }

    #[test]
    fn missing_file_is_not_a_bug() {
        let err = CompilerError::io_error("file not found", None, None);
        assert!(!is_likely_compiler_bug(&err));
    }

    #[test]
    fn codegen_errors_are_bugs() {
        let err = CompilerError::codegen_error("internal codegen failure", None, None);
        assert!(is_likely_compiler_bug(&err));
    }

    #[test]
    fn plugin_errors_are_bugs() {
        let err = CompilerError::PluginError {
            message: "plugin failed".to_string(),
            location: None,
        };
        assert!(is_likely_compiler_bug(&err));
    }

    #[test]
    fn missing_or_circular_imports_are_not_bugs() {
        let missing = CompilerError::module_error("Module 'NoSuchModule' not found", None, None);
        assert!(!is_likely_compiler_bug(&missing));
        let cycle = CompilerError::import_error(
            "Circular dependency detected: A -> B -> A",
            "import",
            None,
        );
        assert!(!is_likely_compiler_bug(&cycle));
    }

    #[test]
    fn plugin_that_cannot_be_loaded_is_not_a_bug() {
        let err = CompilerError::PluginError {
            message: "[PLUGIN001] Failed to load plugins: Plugin 'frame.nonexistent' not found. \
                      Install with: cleen plugin add frame.nonexistent"
                .to_string(),
            location: None,
        };
        assert!(!is_likely_compiler_bug(&err));
    }

    #[test]
    fn extract_error_info_codegen_has_subsystem() {
        let err = CompilerError::codegen_error("wasm encode failed", None, None);
        let info = extract_error_info(&err);
        assert_eq!(info.component, "compiler");
        assert_eq!(info.subsystem.as_deref(), Some("codegen"));
        assert_eq!(info.category, "codegen");
    }

    #[test]
    fn extract_error_info_memory_has_subsystem_and_category() {
        let err = CompilerError::memory_error("out of stack space", None, None);
        let info = extract_error_info(&err);
        assert_eq!(info.subsystem.as_deref(), Some("memory"));
        assert_ne!(
            info.category, "system",
            "category must not be the catch-all 'system'"
        );
        assert_eq!(info.category, "memory");
    }

    #[test]
    fn plugin_origin_marker_reroutes_validation_error_to_framework() {
        // Resolves TELEMETRY-SYNTHETIC-ORIGIN-CLASSIFICATION (f5f2a9954225).
        // Validation errors raised against plugin-synthesized code must route
        // to the plugin's owning component, not the compiler.
        use crate::ast::SourceLocation;
        let loc = SourceLocation {
            line: 0,
            column: 0,
            file: "<plugin:frame.canvas>".to_string(),
            byte_start: None,
            byte_end: None,
        };
        let err = CompilerError::validation_error("Undefined variable 'dt'", loc);
        let info = extract_error_info(&err);
        assert_eq!(info.component, "framework");
        assert_eq!(info.subsystem.as_deref(), Some("plugin:frame.canvas"));
        assert_eq!(info.category, "validation");
    }

    #[test]
    fn plugin_origin_marker_leaves_non_plugin_locations_untouched() {
        // User-file locations must not be rerouted — only synthetic
        // plugin-marker files are eligible for the framework override.
        use crate::ast::SourceLocation;
        let loc = SourceLocation {
            line: 12,
            column: 4,
            file: "/home/user/project/app.cln".to_string(),
            byte_start: None,
            byte_end: None,
        };
        let err = CompilerError::validation_error("Undefined variable 'foo'", loc);
        let info = extract_error_info(&err);
        assert_eq!(info.component, "compiler");
        assert_eq!(info.subsystem.as_deref(), Some("resolver"));
    }

    #[test]
    fn plugin_origin_marker_unknown_plugin_still_reroutes() {
        // Unknown plugin names default to `framework` (better than routing
        // back to `compiler`).
        use crate::ast::SourceLocation;
        let loc = SourceLocation {
            line: 0,
            column: 0,
            file: "<plugin:third.party.foo>".to_string(),
            byte_start: None,
            byte_end: None,
        };
        let err = CompilerError::validation_error("Undefined variable 'x'", loc);
        let info = extract_error_info(&err);
        assert_eq!(info.component, "framework");
        assert_eq!(info.subsystem.as_deref(), Some("plugin:third.party.foo"));
    }

    #[test]
    fn plugin_origin_marker_reroutes_syntax_error() {
        // Plugin-synthesized code that fails at SYN000 (rare — plugins
        // generally emit already-parseable code, but v3 typed-emission can
        // produce forms that trip validate_program) must also reroute.
        use crate::ast::SourceLocation;
        let loc = SourceLocation {
            line: 0,
            column: 0,
            file: "<plugin:frame.data>".to_string(),
            byte_start: None,
            byte_end: None,
        };
        let err = CompilerError::syntax_error("unexpected token", None, Some(loc));
        let info = extract_error_info(&err);
        assert_eq!(info.component, "framework");
        assert_eq!(info.subsystem.as_deref(), Some("plugin:frame.data"));
    }

    #[test]
    fn extract_error_info_validation_has_subsystem_and_category() {
        use crate::ast::SourceLocation;
        let loc = SourceLocation {
            line: 1,
            column: 1,
            file: String::new(),
            byte_start: None,
            byte_end: None,
        };
        let err = CompilerError::validation_error("constraint violated", loc);
        let info = extract_error_info(&err);
        assert_eq!(info.subsystem.as_deref(), Some("resolver"));
        assert_eq!(info.category, "validation");
        assert_ne!(info.category, "system");
    }

    #[test]
    fn extract_error_info_module_has_subsystem_and_category() {
        let err = CompilerError::module_error("module not found", None, None);
        let info = extract_error_info(&err);
        assert_eq!(info.subsystem.as_deref(), Some("module"));
        assert_eq!(info.category, "module");
        assert_ne!(info.category, "system");
    }

    #[test]
    fn extract_error_info_plugin_routes_to_framework() {
        let err = CompilerError::PluginError {
            message: "expand_block failed".to_string(),
            location: None,
        };
        let info = extract_error_info(&err);
        assert_eq!(info.component, "framework");
        assert_eq!(info.subsystem.as_deref(), Some("plugin"));
    }

    #[test]
    fn file_head_snippet_returns_first_50_lines() {
        let source: String = (1..=100).map(|i| format!("line {}\n", i)).collect();
        let snippet = extract_file_head_snippet(&source).expect("should return a snippet");
        // Should contain line 1 formatted with the leading gutter
        assert!(snippet.contains("  1 | line 1"));
        // Should contain line 50 but not line 51
        assert!(snippet.contains("line 50"));
        assert!(!snippet.contains("line 51"));
    }

    #[test]
    fn file_head_snippet_returns_none_for_empty_source() {
        assert!(extract_file_head_snippet("").is_none());
    }

    #[test]
    fn no_error_type_maps_to_system_category() {
        use crate::ast::SourceLocation;
        // "system" was a catch-all category that lost signal — verify it's gone.
        let loc = SourceLocation {
            line: 1,
            column: 1,
            file: String::new(),
            byte_start: None,
            byte_end: None,
        };
        let errs = vec![
            CompilerError::memory_error("x", None, None),
            CompilerError::validation_error("x", loc),
            CompilerError::module_error("x", None, None),
        ];
        for err in &errs {
            let info = extract_error_info(err);
            assert_ne!(
                info.category, "system",
                "catch-all 'system' category must not appear for {:?}",
                err
            );
            assert!(
                info.subsystem.is_some(),
                "subsystem must be set for {:?}",
                err
            );
        }
    }
}
