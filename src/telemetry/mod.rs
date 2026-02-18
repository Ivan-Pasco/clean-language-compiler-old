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
pub use submit::{check_report_statuses, submit_report, SubmitResult};

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
        return false;
    }

    // Show the prompt
    println!();
    println!("Help improve Clean Language!");
    println!();
    println!("  When the compiler encounters an error, anonymous reports can be");
    println!("  sent to help the team fix bugs faster. No source code is ever sent.");
    println!();
    println!("  You can change this anytime with: cln config set telemetry on/off");
    println!();
    print!("  Enable anonymous error reporting? [y/n] ");
    let _ = io::stdout().flush();

    let stdin = io::stdin();
    let response = stdin
        .lock()
        .lines()
        .next()
        .and_then(|l| l.ok())
        .unwrap_or_default();

    let enabled = matches!(
        response.trim().to_lowercase().as_str(),
        "y" | "yes" | "si" | "s"
    );

    config.prompted = true;
    config.enabled = enabled;
    let _ = config.save();

    if enabled {
        println!("  Telemetry enabled. Thank you!");
        println!();

        // Ask for optional email for fix notifications
        print!("  Get notified when your bugs are fixed? (email or Enter to skip) ");
        let _ = io::stdout().flush();

        let email_response = io::stdin()
            .lock()
            .lines()
            .next()
            .and_then(|l| l.ok())
            .unwrap_or_default();

        let email = email_response.trim().to_string();
        if !email.is_empty() && email.contains('@') {
            config.contact_email = Some(email);
            let _ = config.save();
            println!("  Email saved. You'll be notified when reported bugs are fixed.");
        }
    } else {
        println!("  No problem. You can enable it later with: cln config set telemetry on");
    }
    println!();

    enabled
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

    // Attach stored email if available
    if let Some(ref email) = config.contact_email {
        report.user.anonymous = false;
        report.user.contact = Some(email.clone());
    }

    // Track locally
    let mut store = ReportStore::load();
    store.add_report(&report);
    let _ = store.save();

    // Submit in background thread — don't block the CLI
    std::thread::spawn(move || {
        let _ = submit_report(&report);
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

            // Also try to flush any pending reports
            if let Some(queue) = PendingQueue::new() {
                if queue.count() > 0 {
                    let result = queue.flush(|report| {
                        matches!(submit_report(report), SubmitResult::Submitted { .. })
                    });
                    if result.sent > 0 {
                        eprintln!(
                            "Sent {} queued error report(s) to the Clean Language team.",
                            result.sent
                        );
                    }
                }
            }
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
