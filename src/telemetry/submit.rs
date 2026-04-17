/*!
 * Error Report HTTP Submission
 *
 * Handles submitting reports to the Clean Language Error API.
 * Fails gracefully when the backend is unreachable (offline-first design).
 * This is the ONLY file that changes behavior when the backend comes online.
 *
 * NOTE: All reqwest::blocking calls run in std::thread::spawn to avoid
 * panicking when called from within a tokio async runtime context.
 */

use super::queue::PendingQueue;
use super::report::ErrorReport;
use std::time::Duration;

/// Base URL for the Error Reporting API
const API_BASE: &str = "https://errors.cleanlanguage.dev/api/v1";

/// HTTP timeout for submissions (short to avoid blocking)
const SUBMIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Path to the rate-limit backoff file. When the server asks us to back off,
/// we record the "don't retry before" epoch-seconds timestamp here. Every
/// submission checks it first and skips the HTTP round-trip if we're still
/// in the backoff window — this keeps the client from DDoS'ing the server
/// when rate-limiting is in effect across many short-lived CLI invocations.
fn backoff_file() -> Option<std::path::PathBuf> {
    super::config::TelemetryConfig::telemetry_dir().map(|d| d.join("retry_after"))
}

/// Public re-export so the `flush_pending_telemetry` entry point can short-
/// circuit before walking the queue. Same semantics as
/// `backoff_seconds_remaining` but visible outside this module.
pub fn backoff_seconds_remaining_public() -> Option<u64> {
    backoff_seconds_remaining()
}

/// Return Some(seconds_remaining) if we're still inside a server-requested
/// backoff window; None if no active backoff or it has elapsed.
fn backoff_seconds_remaining() -> Option<u64> {
    let path = backoff_file()?;
    let contents = std::fs::read_to_string(&path).ok()?;
    let until_epoch = contents.trim().parse::<u64>().ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now < until_epoch {
        Some(until_epoch - now)
    } else {
        // Window elapsed — clean up the marker so future reads are quick
        let _ = std::fs::remove_file(path);
        None
    }
}

/// Record a backoff window after a `rate_limited` response.
fn record_backoff(retry_after_seconds: u64) {
    let Some(path) = backoff_file() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Cap at 1 hour — even if the server sends a huge value, we shouldn't
    // silently disable telemetry for days. The server can re-ask each cycle.
    let until = now + retry_after_seconds.min(3600);
    let _ = std::fs::write(path, until.to_string());
}

/// Result of a submission attempt
#[derive(Debug)]
pub enum SubmitResult {
    /// Successfully submitted to the backend
    Submitted {
        report_id: String,
        fingerprint: Option<String>,
        tracking_url: String,
    },
    /// Bug was already fixed — server returned fix details
    AlreadyFixed {
        report_id: String,
        fingerprint: Option<String>,
        fixed_in_version: String,
        fix_description: Option<String>,
        message: String,
    },
    /// Bug is known and being worked on
    Known {
        report_id: String,
        fingerprint: Option<String>,
        occurrences: u64,
        current_status: String,
        message: String,
    },
    /// Backend rate-limited this request. Report is queued; no retry before
    /// `retry_after_seconds` elapses.
    RateLimited {
        report_id: String,
        local_path: String,
        retry_after_seconds: u64,
    },
    /// Backend unreachable — queued locally for later
    Queued {
        report_id: String,
        local_path: String,
    },
    /// Failed to submit and failed to queue
    Error { message: String },
}

impl SubmitResult {
    /// Extract the fingerprint from any variant that carries one
    pub fn fingerprint(&self) -> Option<&str> {
        match self {
            Self::Submitted { fingerprint, .. }
            | Self::AlreadyFixed { fingerprint, .. }
            | Self::Known { fingerprint, .. } => fingerprint.as_deref(),
            _ => None,
        }
    }
}

/// Status update from the backend for a tracked report
#[derive(Debug, Clone)]
pub struct StatusUpdate {
    pub report_id: String,
    pub status: String,
    pub fixed_in_version: Option<String>,
    pub fix_description: Option<String>,
    pub fix_commit: Option<String>,
    pub fix_pr: Option<String>,
    pub resolved_at: Option<String>,
    pub message: Option<String>,
}

/// Attempt to submit an error report to the backend API.
/// Falls back to local queue if the backend is unreachable or rate-limiting.
pub fn submit_report(report: &ErrorReport) -> SubmitResult {
    // Respect any in-flight rate-limit backoff. When the server recently
    // returned `{"error":"rate_limited","retry_after":N}` we recorded the
    // resume-at timestamp on disk; skip the HTTP call entirely until it
    // elapses so a burst of CLI invocations does not DDoS the backend.
    if let Some(seconds_remaining) = backoff_seconds_remaining() {
        return queue_for_rate_limit(report, seconds_remaining);
    }

    // Run HTTP in a separate thread to avoid tokio runtime conflict
    let report_clone = report.clone();
    let http_result = std::thread::spawn(move || try_http_submit(&report_clone))
        .join()
        .unwrap_or(Err("Thread panicked".to_string()));

    match http_result {
        Ok(result) => result,
        Err(reason) => {
            // Backend unreachable or rate-limiting — log visibly and queue locally.
            // Prior behavior silently dropped the error which made upload bugs
            // invisible for days; surface the reason on stderr so users know
            // a retry will happen later (see MCP_UPLOAD_BROKEN lineage).
            if let Some(retry_after) = parse_rate_limited_reason(&reason) {
                eprintln!(
                    "[telemetry] server rate-limited error reports (retry_after={}s); queued locally.",
                    retry_after
                );
                return queue_for_rate_limit(report, retry_after);
            }
            eprintln!(
                "[telemetry] error report upload failed ({}); queued locally for retry.",
                reason
            );
            queue_for_retry(report, &reason)
        }
    }
}

/// If `try_http_submit` came back with our internal "rate_limited" error
/// signature, extract the retry_after seconds. Returns None otherwise.
fn parse_rate_limited_reason(reason: &str) -> Option<u64> {
    if !reason.starts_with("rate_limited") {
        return None;
    }
    let start = reason.find("retry_after=")? + "retry_after=".len();
    let end = reason[start..]
        .find('s')
        .map(|i| start + i)
        .unwrap_or(reason.len());
    reason[start..end].parse::<u64>().ok()
}

/// Queue a rate-limited report. The difference from `queue_for_retry` is that
/// the SubmitResult carries the `retry_after_seconds` so callers (and the
/// MCP tool user) can surface the backoff expectation.
fn queue_for_rate_limit(report: &ErrorReport, retry_after_seconds: u64) -> SubmitResult {
    let report_id = report.report_id.clone();
    match PendingQueue::new() {
        Some(queue) => match queue.enqueue(report) {
            Ok(path) => SubmitResult::RateLimited {
                report_id,
                local_path: path.to_string_lossy().to_string(),
                retry_after_seconds,
            },
            Err(e) => SubmitResult::Error {
                message: format!("Failed to queue rate-limited report: {}", e),
            },
        },
        None => SubmitResult::Error {
            message: "Cannot determine home directory for local queue".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limited_reason_parses_retry_after() {
        assert_eq!(
            parse_rate_limited_reason("rate_limited (retry_after=3600s)"),
            Some(3600)
        );
        assert_eq!(
            parse_rate_limited_reason("rate_limited (retry_after=60s)"),
            Some(60)
        );
    }

    #[test]
    fn non_rate_limit_reason_returns_none() {
        assert_eq!(parse_rate_limited_reason("HTTP 502 Bad Gateway"), None);
        assert_eq!(parse_rate_limited_reason("Connection refused"), None);
        assert_eq!(parse_rate_limited_reason(""), None);
    }
}

/// Helper: enqueue a report for later retry. Used by both the rate-limit path
/// and the generic network-failure path so the disk state is identical.
fn queue_for_retry(report: &ErrorReport, _reason: &str) -> SubmitResult {
    let report_id = report.report_id.clone();
    match PendingQueue::new() {
        Some(queue) => match queue.enqueue(report) {
            Ok(path) => SubmitResult::Queued {
                report_id,
                local_path: path.to_string_lossy().to_string(),
            },
            Err(e) => SubmitResult::Error {
                message: format!("Failed to queue report locally: {}", e),
            },
        },
        None => SubmitResult::Error {
            message: "Cannot determine home directory for local queue".to_string(),
        },
    }
}

/// A verification result for a single report, sent as part of a batch check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReportVerification {
    pub report_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_details: Option<String>,
}

/// Batch check report statuses and optionally send verification results.
/// Single POST /api/v1/reports/check replaces N individual GET calls.
/// Returns empty vec if backend is unreachable (offline-first).
pub fn batch_check_and_verify(
    compiler_version: &str,
    reports: &[ReportVerification],
) -> Vec<StatusUpdate> {
    if reports.is_empty() {
        return Vec::new();
    }

    let version = compiler_version.to_string();
    let reports_clone = reports.to_vec();
    std::thread::spawn(move || batch_check_inner(&version, &reports_clone))
        .join()
        .unwrap_or_default()
}

/// Inner function for batch check + verify
fn batch_check_inner(compiler_version: &str, reports: &[ReportVerification]) -> Vec<StatusUpdate> {
    let client = match build_client() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let body = serde_json::json!({
        "compiler_version": compiler_version,
        "reports": reports,
    });

    let url = format!("{}/reports/check", API_BASE);
    match with_auth(
        client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body.to_string()),
    )
    .send()
    {
        Ok(response) if response.status().is_success() => {
            if let Ok(text) = response.text() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(results) = json.get("results").and_then(|v| v.as_array()) {
                        return results
                            .iter()
                            .filter_map(|r| {
                                Some(StatusUpdate {
                                    report_id: r.get("report_id")?.as_str()?.to_string(),
                                    status: r
                                        .get("status")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown")
                                        .to_string(),
                                    fixed_in_version: r
                                        .get("fixed_in_version")
                                        .or_else(|| r.get("verified_in"))
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    fix_description: r
                                        .get("fix_description")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    fix_commit: r
                                        .get("fix_commit")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    fix_pr: r
                                        .get("fix_pr")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    resolved_at: r
                                        .get("resolved_at")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    message: r
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                })
                            })
                            .collect();
                    }
                }
            }
            Vec::new()
        }
        _ => {
            // Backend unreachable — fall back to legacy per-report check
            check_statuses_legacy(reports)
        }
    }
}

/// A bug entry returned by GET /api/v1/bugs
#[derive(Debug, Clone)]
pub struct ComponentBug {
    pub fingerprint: String,
    pub error_code: String,
    pub component: String,
    pub severity: String,
    pub message: String,
    pub minimal_repro: Option<String>,
    pub expected_behavior: Option<String>,
    pub actual_behavior: Option<String>,
    pub occurrences: u64,
    pub priority_score: f64,
    pub first_reported_version: Option<String>,
    pub ai_suggested_fix: Option<String>,
    pub report_ids: Vec<String>,
}

/// Fetch open bugs for a component. Single GET call returns everything.
pub fn fetch_component_bugs(component: &str, status: &str) -> Vec<ComponentBug> {
    let comp = component.to_string();
    let stat = status.to_string();
    std::thread::spawn(move || fetch_bugs_inner(&comp, &stat))
        .join()
        .unwrap_or_default()
}

fn fetch_bugs_inner(component: &str, status: &str) -> Vec<ComponentBug> {
    let client = match build_client() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let url = format!(
        "{}/bugs?component={}&status={}",
        API_BASE, component, status
    );
    match with_auth(client.get(&url)).send() {
        Ok(response) if response.status().is_success() => {
            if let Ok(text) = response.text() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(bugs) = json.get("bugs").and_then(|v| v.as_array()) {
                        return bugs
                            .iter()
                            .filter_map(|b| {
                                Some(ComponentBug {
                                    fingerprint: b.get("fingerprint")?.as_str()?.to_string(),
                                    error_code: b
                                        .get("error_code")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    component: b
                                        .get("component")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    severity: b
                                        .get("severity")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("bug")
                                        .to_string(),
                                    message: b
                                        .get("canonical_message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    minimal_repro: b
                                        .get("minimal_repro")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    expected_behavior: b
                                        .get("expected_behavior")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    actual_behavior: b
                                        .get("actual_behavior")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    occurrences: b
                                        .get("occurrences")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(1),
                                    priority_score: b
                                        .get("priority_score")
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(0.0),
                                    first_reported_version: b
                                        .get("first_reported_version")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    ai_suggested_fix: b
                                        .get("ai_suggested_fix")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    report_ids: b
                                        .get("report_ids")
                                        .and_then(|v| v.as_array())
                                        .map(|arr| {
                                            arr.iter()
                                                .filter_map(|v| v.as_str().map(String::from))
                                                .collect()
                                        })
                                        .unwrap_or_default(),
                                })
                            })
                            .collect();
                    }
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Legacy fallback: per-report status check (used when batch endpoint unavailable)
fn check_statuses_legacy(reports: &[ReportVerification]) -> Vec<StatusUpdate> {
    let client = match build_client() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut updates = Vec::new();
    for report in reports {
        let url = format!("{}/reports/status?id={}", API_BASE, report.report_id);
        if let Ok(response) = with_auth(client.get(&url)).send() {
            if response.status().is_success() {
                if let Ok(body) = response.text() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        updates.push(StatusUpdate {
                            report_id: report.report_id.clone(),
                            status: json
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            fixed_in_version: json
                                .get("fixed_in_version")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            fix_description: json
                                .get("fix_description")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            fix_commit: json
                                .get("fix_commit")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            fix_pr: json
                                .get("fix_pr")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            resolved_at: json
                                .get("resolved_at")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            message: json
                                .get("message")
                                .and_then(|v| v.as_str())
                                .map(String::from),
                        });
                    }
                }
            }
        }
    }
    updates
}

/// Backwards-compatible wrapper: check statuses without sending verifications.
pub fn check_report_statuses(report_ids: &[String]) -> Vec<StatusUpdate> {
    let reports: Vec<ReportVerification> = report_ids
        .iter()
        .map(|id| ReportVerification {
            report_id: id.clone(),
            verified: None,
            verification_details: None,
        })
        .collect();
    batch_check_and_verify(crate::VERSION, &reports)
}

/// Build an HTTP client with appropriate timeout
fn build_client() -> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder()
        .timeout(SUBMIT_TIMEOUT)
        .build()
}

/// Load the error-API key from ~/.cleen/error-api-key.
/// Returns None if the file is missing or empty — the server accepts unauthenticated
/// submissions, but sending the key marks the report as trusted.
fn load_api_key() -> Option<String> {
    let path = dirs::home_dir()?.join(".cleen").join("error-api-key");
    let contents = std::fs::read_to_string(path).ok()?;
    let key = contents.trim().to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

/// Attach the Bearer auth header to a request builder if a local API key exists.
fn with_auth(req: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
    match load_api_key() {
        Some(key) => req.header("Authorization", format!("Bearer {}", key)),
        None => req,
    }
}

/// Try to submit a report via HTTP POST
fn try_http_submit(report: &ErrorReport) -> Result<SubmitResult, String> {
    let client = build_client().map_err(|e| e.to_string())?;

    let url = format!("{}/reports", API_BASE);
    let body = serde_json::to_string(report).map_err(|e| e.to_string())?;

    let response = with_auth(
        client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body),
    )
    .send()
    .map_err(|e| e.to_string())?;

    // Detect rate-limiting early, regardless of HTTP status code. The server
    // currently returns HTTP 200 with `{"error":"rate_limited","retry_after":N}`
    // (it SHOULD return 429, but we defend against both shapes).
    let status_code = response.status();
    let body_text = response.text().unwrap_or_default();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body_text) {
        let is_rate_limited = status_code.as_u16() == 429
            || json
                .get("error")
                .and_then(|v| v.as_str())
                .map(|e| e == "rate_limited")
                .unwrap_or(false);
        if is_rate_limited {
            let retry_after = json
                .get("retry_after")
                .and_then(|v| v.as_u64())
                .unwrap_or(60);
            record_backoff(retry_after);
            return Err(format!("rate_limited (retry_after={}s)", retry_after));
        }
    }

    if status_code.is_success() {
        let report_id = report.report_id.clone();
        let tracking_url = format!("https://errors.cleanlanguage.dev/report/{}", report_id);

        {
            let body = &body_text;
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
                let status = json
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("received");

                let fingerprint = json
                    .get("fingerprint")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                // Handle "already_fixed" — bug was resolved in a newer version
                if status == "already_fixed" {
                    return Ok(SubmitResult::AlreadyFixed {
                        report_id,
                        fingerprint,
                        fixed_in_version: json
                            .get("fixed_in_version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        fix_description: json
                            .get("fix_description")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        message: json
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("This bug has been fixed. Update your compiler.")
                            .to_string(),
                    });
                }

                // Handle "known" — bug is open and being tracked
                if status == "known" {
                    return Ok(SubmitResult::Known {
                        report_id,
                        fingerprint,
                        occurrences: json
                            .get("occurrences")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(1),
                        current_status: json
                            .get("current_status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("open")
                            .to_string(),
                        message: json
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("This is a known issue.")
                            .to_string(),
                    });
                }

                // Default: new report accepted
                let tracking = json
                    .get("tracking_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&tracking_url)
                    .to_string();

                return Ok(SubmitResult::Submitted {
                    report_id,
                    fingerprint,
                    tracking_url: tracking,
                });
            }
        }

        Ok(SubmitResult::Submitted {
            report_id,
            fingerprint: None,
            tracking_url,
        })
    } else {
        Err(format!("HTTP {}", status_code))
    }
}
