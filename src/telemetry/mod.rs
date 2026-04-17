/*!
 * Error Reporting and Telemetry Module
 *
 * Provides opt-in error reporting, local report storage, and fix notification.
 * Privacy-first: all telemetry is opt-in and never sends user source code.
 *
 * # Architecture
 *
 * - `config`: Telemetry preferences stored in ~/.cleen/telemetry/config.toml
 * - `report`: ErrorReport schema and local ReportStore for fix tracking
 * - `queue`: Offline queue for reports when backend is unreachable
 * - `submit`: HTTP submission to the Error API (graceful fallback)
 */

pub mod config;
pub mod queue;
pub mod report;
pub mod submit;

pub use config::{ConsentLevel, TelemetryConfig};
pub use queue::PendingQueue;
pub use report::{ErrorReport, ReportError, ReportStatus, ReportStore, TrackedReport};
pub use submit::{
    batch_check_and_verify, check_report_statuses, fetch_component_bugs, submit_report,
    ComponentBug, ReportVerification, StatusUpdate, SubmitResult,
};

use std::io::{self, BufRead, IsTerminal, Write};

/// Show the first-run telemetry opt-in prompt if the user hasn't been asked yet.
/// Returns true if telemetry was enabled, false if declined or already prompted.
/// Non-blocking: skips the prompt if stdin is not a terminal (CI, pipes, etc.).
pub fn maybe_prompt_telemetry() -> bool {
    let mut config = TelemetryConfig::load();

    // Already prompted — nothing to do
    if config.prompted {
        return config.enabled;
    }

    // Don't prompt in non-interactive contexts (CI, pipes, MCP)
    if !io::stdin().is_terminal() {
        // Enable bug reporting silently in non-interactive mode
        if !config.prompted {
            config.prompted = true;
            config.enabled = true;
            let _ = config.save();
        }
        return true;
    }

    // --- Developer onboarding (first run) ---
    println!();
    println!("  Welcome to Clean Language v{}!", crate::VERSION);
    println!();

    // Step 1: Name (optional)
    print!("  Your name (Enter to skip): ");
    let _ = io::stdout().flush();
    let name = io::stdin()
        .lock()
        .lines()
        .next()
        .and_then(|l| l.ok())
        .unwrap_or_default()
        .trim()
        .to_string();

    if !name.is_empty() {
        config.developer_name = Some(name.clone());
    }

    // Step 2: Email (frame as staying connected to the ecosystem)
    println!();
    println!("  Your email connects you to Clean Language:");
    println!("    - New releases and what's in them");
    println!("    - Bug fixes for issues you report");
    println!("    - Plugin updates for your stack");
    println!("    - Security advisories");
    println!();
    println!("  Stored locally — never shared with third parties.");
    println!();
    print!("  Email (Enter to skip): ");
    let _ = io::stdout().flush();
    let email = io::stdin()
        .lock()
        .lines()
        .next()
        .and_then(|l| l.ok())
        .unwrap_or_default()
        .trim()
        .to_string();

    if !email.is_empty() && email.contains('@') {
        config.contact_email = Some(email);
    }

    // Bug reporting is always enabled — it's a feature, not an opt-in
    config.prompted = true;
    config.enabled = true;
    let _ = config.save();

    // Confirmation
    println!();
    if let Some(ref dev_name) = config.developer_name {
        println!("  Welcome, {}!", dev_name);
    } else {
        println!("  Setup complete.");
    }
    if config.contact_email.is_some() {
        println!("  You'll be notified when bugs you report are fixed.");
    }
    println!("  Update anytime: cln config");
    println!();

    true
}

/// Prompt for email at bug-report time if not yet configured.
/// Called when a compilation error is about to be reported.
/// Returns the email if provided, or None.
pub fn maybe_prompt_email_on_bug() -> Option<String> {
    let mut config = TelemetryConfig::load();

    // Already has email — nothing to do
    if config.contact_email.is_some() {
        return config.contact_email.clone();
    }

    // Don't prompt in non-interactive contexts
    if !io::stdin().is_terminal() {
        return None;
    }

    println!();
    println!("  Get notified when this bug is fixed, plus releases and updates.");
    print!("  Email (Enter to skip): ");
    let _ = io::stdout().flush();

    let email = io::stdin()
        .lock()
        .lines()
        .next()
        .and_then(|l| l.ok())
        .unwrap_or_default()
        .trim()
        .to_string();

    if !email.is_empty() && email.contains('@') {
        config.contact_email = Some(email.clone());
        let _ = config.save();
        println!("  Saved. You can remove it anytime: cln config set email clear");
        Some(email)
    } else {
        None
    }
}

/// Report a compilation failure via telemetry (if enabled).
/// Called automatically after a failed `cln compile` invocation.
/// Non-blocking: runs submission in the background thread.
pub fn report_compile_failure(errors: &[crate::error::CompilerError], source_file: &str) {
    let config = TelemetryConfig::load();
    if !config.enabled {
        return;
    }

    // Take the first error as representative
    let first_error = match errors.first() {
        Some(e) => e,
        None => return,
    };

    let (code, category, component, message) = extract_error_info(first_error);

    let report_id = generate_report_id();
    let mut report = ErrorReport::new(
        report_id,
        ReportError {
            code,
            category,
            component,
            severity: "bug".to_string(),
            message,
            file_context: Some(source_file.to_string()),
        },
        "cli_telemetry",
        &config.consent_level.to_string(),
    );

    // Attach email: use stored email, or prompt if first bug report
    if let Some(ref email) = config.contact_email {
        report.user.anonymous = false;
        report.user.contact = Some(email.clone());
    } else if let Some(email) = maybe_prompt_email_on_bug() {
        report.user.anonymous = false;
        report.user.contact = Some(email);
    }

    // Track locally
    let mut store = ReportStore::load();
    store.add_report(&report);
    let _ = store.save();

    // Submit in background thread — don't block the CLI.
    // Capture the server-returned fingerprint so the local record is confirmed.
    let report_id = report.report_id.clone();
    std::thread::spawn(move || {
        let result = submit_report(&report);
        if let Some(fp) = result.fingerprint() {
            let mut store = ReportStore::load();
            store.update_fingerprint(&report_id, fp);
            let _ = store.save();
        }
    });
}

/// Check if the compiler version has changed since last run.
/// If so, check for fix notifications and flush the pending queue.
pub fn check_version_change() {
    let mut config = TelemetryConfig::load();
    let current_version = crate::VERSION;

    let version_changed = config
        .last_seen_version
        .as_deref()
        .map_or(true, |v| v != current_version);

    if version_changed {
        config.last_seen_version = Some(current_version.to_string());
        let _ = config.save();

        // On version change, check for fix notifications
        if config.enabled {
            check_fix_notifications();
            flush_pending_telemetry(true);
        }
    }
}

/// Flush any queued reports and retry reports that were saved locally but never
/// confirmed by the backend (fingerprint never captured).
///
/// Safe to call at any time; does nothing if telemetry is disabled. When `verbose`
/// is true, prints a short summary to stderr for CLI contexts; MCP contexts should
/// pass false so stdout/stderr stay clean.
pub fn flush_pending_telemetry(verbose: bool) {
    let config = TelemetryConfig::load();
    if !config.enabled {
        return;
    }

    // 1) Flush the offline queue: reports that failed to POST are saved as full
    //    ErrorReport JSON under ~/.cleen/telemetry/pending_reports/.
    let mut queue_sent = 0usize;
    if let Some(queue) = PendingQueue::new() {
        if queue.count() > 0 {
            let mut store = ReportStore::load();
            let mut store_dirty = false;
            let flush_result = queue.flush(|report| {
                let result = submit_report(report);
                if let Some(fp) = result.fingerprint() {
                    store.update_fingerprint(&report.report_id, fp);
                    store_dirty = true;
                }
                matches!(
                    result,
                    SubmitResult::Submitted { .. }
                        | SubmitResult::AlreadyFixed { .. }
                        | SubmitResult::Known { .. }
                )
            });
            if store_dirty {
                let _ = store.save();
            }
            queue_sent = flush_result.sent;
        }
    }

    // 2) Retry reports that live in reported_errors.json with no fingerprint.
    //    Historically these were accepted by the server but the client discarded
    //    the returned fingerprint, so the local record was never confirmed.
    //    Re-POST is safe: the server deduplicates by content hash.
    let mut retry_confirmed = 0usize;
    let store = ReportStore::load();
    let unposted = store.unposted_report_ids();
    if !unposted.is_empty() {
        let mut store = store;
        for report_id in unposted {
            let tracked = match store
                .get_all_reports()
                .iter()
                .find(|r| r.report_id == report_id)
                .cloned()
            {
                Some(r) => r,
                None => continue,
            };

            let minimal = ErrorReport {
                schema_version: "1.0.0".to_string(),
                report_id: tracked.report_id.clone(),
                timestamp: tracked.reported_at,
                source: report::ReportSource {
                    channel: "retry".to_string(),
                    compiler_version: tracked.compiler_version.clone(),
                    os: std::env::consts::OS.to_string(),
                    arch: std::env::consts::ARCH.to_string(),
                    runtime: None,
                },
                error: ReportError {
                    code: tracked.error_code.clone(),
                    category: "unknown".to_string(),
                    component: "compiler".to_string(),
                    severity: "bug".to_string(),
                    message: tracked.summary.clone(),
                    file_context: None,
                },
                reproduction: None,
                ai_context: None,
                user: report::ReportUser {
                    anonymous: config.contact_email.is_none(),
                    contact: config.contact_email.clone(),
                    consent_level: config.consent_level.to_string(),
                },
            };

            let result = submit_report(&minimal);
            if let Some(fp) = result.fingerprint() {
                store.update_fingerprint(&tracked.report_id, fp);
                retry_confirmed += 1;
            }
        }
        if retry_confirmed > 0 {
            let _ = store.save();
        }
    }

    if verbose {
        if queue_sent > 0 {
            eprintln!(
                "Sent {} queued error report(s) to the Clean Language team.",
                queue_sent
            );
        }
        if retry_confirmed > 0 {
            eprintln!(
                "Confirmed {} previously-reported error(s) with the server.",
                retry_confirmed
            );
        }
    }
}

/// Check for fix notifications after a compiler update.
/// Shows a message if any previously reported bugs have been resolved.
/// Designed to run after `cleen install` or on version change.
pub fn check_fix_notifications() {
    let config = TelemetryConfig::load();
    if !config.enabled {
        return;
    }

    let mut store = ReportStore::load();
    let unresolved_ids: Vec<String> = store
        .get_all_reports()
        .iter()
        .filter(|r| r.status == ReportStatus::Reported || r.status == ReportStatus::InProgress)
        .map(|r| r.report_id.clone())
        .collect();

    if unresolved_ids.is_empty() {
        return;
    }

    // Check backend for status updates
    let updates = check_report_statuses(&unresolved_ids);

    let mut fixed_count = 0;
    let mut fixed_messages: Vec<String> = Vec::new();

    for update in &updates {
        let new_status = match update.status.as_str() {
            "resolved" => ReportStatus::Resolved,
            "acknowledged" => ReportStatus::Acknowledged,
            "in_progress" => ReportStatus::InProgress,
            "wont_fix" => ReportStatus::WontFix,
            _ => continue,
        };

        if new_status == ReportStatus::Resolved {
            fixed_count += 1;
            let version_info = update
                .fixed_in_version
                .as_deref()
                .map(|v| format!(" in version {}", v))
                .unwrap_or_default();
            let desc = update.fix_description.as_deref().unwrap_or("Bug fixed");
            fixed_messages.push(format!(
                "  \u{2713} {} \u{2014} {}{}",
                update.report_id.get(..8).unwrap_or(&update.report_id),
                desc,
                version_info
            ));
        }

        store.update_status(
            &update.report_id,
            new_status,
            update.fixed_in_version.clone(),
            update.fix_description.clone(),
            update.fix_pr.clone(),
        );
        store.mark_notified(&update.report_id);
    }

    let _ = store.save();

    if fixed_count > 0 {
        println!();
        println!(
            "Good news! {} bug(s) you reported have been fixed:",
            fixed_count
        );
        for msg in &fixed_messages {
            println!("{}", msg);
        }
        println!();
        println!("Run `cln fixes` for full details.");
        println!();
    }
}

/// Extract structured error info from a CompilerError
fn extract_error_info(error: &crate::error::CompilerError) -> (String, String, String, String) {
    match error {
        crate::error::CompilerError::Syntax { context } => (
            context
                .error_code
                .as_deref()
                .unwrap_or("SYN000")
                .to_string(),
            "syntax".to_string(),
            "parser".to_string(),
            context.message.clone(),
        ),
        crate::error::CompilerError::Type { context } => (
            context
                .error_code
                .as_deref()
                .unwrap_or("TYP000")
                .to_string(),
            "semantic".to_string(),
            "type_checker".to_string(),
            context.message.clone(),
        ),
        crate::error::CompilerError::Codegen { context } => (
            context
                .error_code
                .as_deref()
                .unwrap_or("COD000")
                .to_string(),
            "codegen".to_string(),
            "codegen".to_string(),
            context.message.clone(),
        ),
        crate::error::CompilerError::Runtime { context } => (
            context
                .error_code
                .as_deref()
                .unwrap_or("RUN000")
                .to_string(),
            "runtime".to_string(),
            "runtime".to_string(),
            context.message.clone(),
        ),
        crate::error::CompilerError::Memory { context }
        | crate::error::CompilerError::IO { context }
        | crate::error::CompilerError::Validation { context }
        | crate::error::CompilerError::Module { context }
        | crate::error::CompilerError::Testing { context } => (
            context
                .error_code
                .as_deref()
                .unwrap_or("SYS000")
                .to_string(),
            "system".to_string(),
            format!("{:?}", context.error_type).to_lowercase(),
            context.message.clone(),
        ),
        crate::error::CompilerError::LexError(lex_err) => (
            "LEX000".to_string(),
            "syntax".to_string(),
            "lexer".to_string(),
            format!("{}", lex_err),
        ),
        crate::error::CompilerError::PluginError { message, .. } => (
            "PLG000".to_string(),
            "plugin".to_string(),
            "plugin".to_string(),
            message.clone(),
        ),
    }
}

/// Generate a UUID v4-format report ID
fn generate_report_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let a: u32 = rng.gen();
    let b: u16 = rng.gen();
    let c: u16 = (rng.gen::<u16>() & 0x0FFF) | 0x4000;
    let d: u16 = (rng.gen::<u16>() & 0x3FFF) | 0x8000;
    let e: u64 = rng.gen::<u64>() & 0xFFFF_FFFF_FFFF;
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", a, b, c, d, e)
}
