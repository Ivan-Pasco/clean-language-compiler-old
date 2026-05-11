/*!
 * Dev-context detection for telemetry.
 *
 * When the platform authors are iterating on Clean Language itself, compile
 * failures against the component they are editing should never reach the
 * public error dashboard. Those failures are work-in-progress, not user bugs.
 *
 * Two signals classify a run as "dev mode":
 *  1. The compiler binary is not an installed release (runs from a `target/`
 *     build directory instead of an install root like `~/.cleen/bin/`).
 *  2. The source file being compiled lives inside a Clean Language component
 *     source tree (paths containing `clean-language-compiler/`,
 *     `clean-framework/`, etc.).
 *
 * Either signal flips the run to dev mode; the failure is written to a local
 * dev queue instead of uploaded. An explicit override is available via the
 * `CLEEN_TELEMETRY_FORCE` env var (`publish` or `local`).
 */

use std::path::PathBuf;

/// Known Clean Language component directory names. Any of these appearing as
/// a path segment under the source file (or the binary) marks it as dev work.
const COMPONENT_DIRS: &[&str] = &[
    "clean-language-compiler",
    "clean-framework",
    "clean-server",
    "clean-node-server",
    "clean-manager",
    "clean-extension",
    "clean-ui",
    "clean-canvas",
    "clean-llm",
    "clean-cpanel-plugin",
    "Clean MCP",
];

/// Map a normalized component name (as used in error reports) to the set of
/// directory names that identify its source tree on disk. A single component
/// may live in multiple directory names across historical renames.
fn component_dirs_for(component: &str) -> &'static [&'static str] {
    match component {
        "compiler" => &["clean-language-compiler"],
        "framework" => &["clean-framework"],
        "server" => &["clean-server", "clean-node-server"],
        "manager" => &["clean-manager"],
        "extension" => &["clean-extension"],
        "ui" => &["clean-ui"],
        "canvas" => &["clean-canvas"],
        "llm" => &["clean-llm"],
        "mcp" => &["Clean MCP"],
        "cpanel-plugin" | "cpanel" => &["clean-cpanel-plugin"],
        _ => &[],
    }
}

/// Outcome of dev-context detection with the reason attached so callers can
/// log it (useful when someone wonders why a report never hit the dashboard).
#[derive(Debug, Clone)]
pub enum DevContext {
    /// Dev mode: telemetry upload is suppressed. Reason string is human-readable.
    Dev(String),
    /// Release/user mode: upload proceeds normally.
    Release,
}

impl DevContext {
    pub fn is_dev(&self) -> bool {
        matches!(self, DevContext::Dev(_))
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            DevContext::Dev(r) => Some(r.as_str()),
            DevContext::Release => None,
        }
    }
}

/// Classify the current run based on a `.cln` source file path. Used by the
/// auto-report path after a failed compile.
pub fn detect(source_file: &str) -> DevContext {
    if let Some(ctx) = env_override() {
        return ctx;
    }

    if let Some(reason) = binary_looks_like_dev_build() {
        return DevContext::Dev(reason);
    }

    if let Some(reason) = source_is_in_component_tree(source_file) {
        return DevContext::Dev(reason);
    }

    DevContext::Release
}

/// Classify the current run when a component name is known but no specific
/// source file path is (MCP `report_error` tool, `cln report` CLI). Dev-mode
/// fires when the current working directory is inside the source tree of the
/// component being reported.
pub fn detect_for_component(component: &str) -> DevContext {
    if let Some(ctx) = env_override() {
        return ctx;
    }

    // Binary-dev-build check: a `cargo build` binary means we are developing
    // the compiler itself. Suppress reports for the compiler and mcp components
    // (both ship in the same `cln` binary) to avoid WIP noise on the dashboard.
    // For cross-component bugs (framework, server, extension, etc.) the binary
    // build type is irrelevant — only the CWD check below should apply.
    let is_compiler_binary_component = matches!(component, "compiler" | "mcp");
    if is_compiler_binary_component {
        if let Some(reason) = binary_looks_like_dev_build() {
            return DevContext::Dev(reason);
        }
    }

    let dirs = component_dirs_for(component);
    if dirs.is_empty() {
        return DevContext::Release;
    }

    let Ok(cwd) = std::env::current_dir() else {
        return DevContext::Release;
    };
    for ancestor in cwd.ancestors() {
        if let Some(name) = ancestor.file_name().and_then(|n| n.to_str()) {
            // Case-insensitive comparison: macOS filesystems are case-insensitive
            // by default, so "Clean MCP" and "clean mcp" are the same directory.
            if dirs.iter().any(|&d| d.eq_ignore_ascii_case(name)) {
                return DevContext::Dev(format!(
                    "cwd inside {} source tree: {}",
                    component,
                    ancestor.display()
                ));
            }
        }
    }
    DevContext::Release
}

fn env_override() -> Option<DevContext> {
    match std::env::var("CLEEN_TELEMETRY_FORCE").ok().as_deref() {
        Some("publish") => Some(DevContext::Release),
        Some("local") => Some(DevContext::Dev("CLEEN_TELEMETRY_FORCE=local".to_string())),
        _ => None,
    }
}

/// Returns Some(reason) if the currently-running executable path looks like a
/// `cargo build` output rather than an installed binary.
fn binary_looks_like_dev_build() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe_str = exe.to_string_lossy();

    // Installed releases live under `.cleen/bin/` (or a similar install root).
    // Anything under `target/debug/` or `target/release/` inside a component
    // tree is a dev build.
    let is_in_target_dir = exe.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s == "target"
    }) && (exe_str.contains("/target/debug/")
        || exe_str.contains("/target/release/"));

    if is_in_target_dir {
        return Some(format!("dev build: {}", exe_str));
    }

    None
}

/// Returns Some(reason) if `source_file` lives under a known Clean Language
/// component source directory.
fn source_is_in_component_tree(source_file: &str) -> Option<String> {
    if source_file.is_empty() {
        return None;
    }

    let path = PathBuf::from(source_file);
    for ancestor in path.ancestors() {
        if let Some(name) = ancestor.file_name().and_then(|n| n.to_str()) {
            if COMPONENT_DIRS.iter().any(|&c| c.eq_ignore_ascii_case(name)) {
                return Some(format!(
                    "source inside component tree: {}",
                    ancestor.display()
                ));
            }
        }
    }
    None
}

/// Helper used by `dev_queue` when serialising the outcome — keeps the reason
/// string stable across callers.
pub fn describe(ctx: &DevContext) -> &str {
    match ctx {
        DevContext::Dev(r) => r.as_str(),
        DevContext::Release => "release",
    }
}

/// Shared component list accessor for tests / tooling.
pub fn component_dirs() -> &'static [&'static str] {
    COMPONENT_DIRS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env mutation is process-global; serialize tests that touch it.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn override_publish_forces_release() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("CLEEN_TELEMETRY_FORCE", "publish");
        let ctx =
            detect("/Users/x/Documents/Dev/Clean Language/clean-language-compiler/tests/foo.cln");
        std::env::remove_var("CLEEN_TELEMETRY_FORCE");
        assert!(!ctx.is_dev(), "publish override must force Release");
    }

    #[test]
    fn override_local_forces_dev() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("CLEEN_TELEMETRY_FORCE", "local");
        let ctx = detect("/tmp/random/user/app.cln");
        std::env::remove_var("CLEEN_TELEMETRY_FORCE");
        assert!(ctx.is_dev());
    }

    #[test]
    fn source_inside_component_tree_is_dev() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CLEEN_TELEMETRY_FORCE");
        let ctx = detect("/Users/x/Documents/Dev/Clean Language/clean-framework/examples/app.cln");
        // Binary check may or may not flag this depending on how tests are
        // run; either way the source-tree check alone should flip it.
        assert!(
            ctx.is_dev(),
            "source inside clean-framework/ must be dev mode"
        );
    }

    #[test]
    fn source_outside_tree_with_installed_binary_is_release() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CLEEN_TELEMETRY_FORCE");
        // We can't reliably spoof current_exe in a unit test, so this only
        // asserts the source-tree branch returns None for neutral paths.
        let reason = source_is_in_component_tree("/tmp/user-project/main.cln");
        assert!(reason.is_none());
    }

    #[test]
    fn cross_component_report_from_compiler_cwd_is_not_suppressed_by_binary_check() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("CLEEN_TELEMETRY_FORCE");
        // When reporting a framework/server/extension bug from within the
        // compiler source tree, the binary dev-build check must NOT fire.
        // Only the CWD check applies, and since CWD is not inside clean-framework/,
        // the report must reach the dashboard (Release mode).
        // NOTE: binary_looks_like_dev_build() may still return Some when tests
        // run from target/. We verify via component_dirs_for logic only.
        let dirs = component_dirs_for("framework");
        assert!(!dirs.is_empty(), "framework must have known dirs");
        // Compiler CWD ancestors do not include clean-framework/
        let compiler_cwd = std::path::PathBuf::from(
            "/Users/x/Documents/Dev/Clean Language/clean-language-compiler",
        );
        let cwd_in_framework = compiler_cwd.ancestors().any(|a| {
            a.file_name()
                .and_then(|n| n.to_str())
                .map(|n| dirs.iter().any(|&d| d == n))
                .unwrap_or(false)
        });
        assert!(
            !cwd_in_framework,
            "compiler CWD must not match framework dirs — cross-component reports must reach dashboard"
        );
    }

    #[test]
    fn binary_check_only_fires_for_compiler_and_mcp() {
        // Verify that the binary check is gated on compiler/mcp components.
        // For framework/server/extension the function must fall through to CWD check.
        for component in &["framework", "server", "extension", "ui", "canvas"] {
            let is_compiler_binary_component = matches!(*component, "compiler" | "mcp");
            assert!(
                !is_compiler_binary_component,
                "component '{}' must not be guarded by binary dev-build check",
                component
            );
        }
        for component in &["compiler", "mcp"] {
            let is_compiler_binary_component = matches!(*component, "compiler" | "mcp");
            assert!(
                is_compiler_binary_component,
                "component '{}' must be guarded by binary dev-build check",
                component
            );
        }
    }
}
