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

/// Result of a submission attempt
#[derive(Debug)]
pub enum SubmitResult {
    /// Successfully submitted to the backend
    Submitted {
        report_id: String,
        tracking_url: String,
    },
    /// Backend unreachable — queued locally for later
    Queued {
        report_id: String,
        local_path: String,
    },
    /// Failed to submit and failed to queue
    Error { message: String },
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
/// Falls back to local queue if the backend is unreachable.
pub fn submit_report(report: &ErrorReport) -> SubmitResult {
    let report_id = report.report_id.clone();

    // Run HTTP in a separate thread to avoid tokio runtime conflict
    let report_clone = report.clone();
    let http_result = std::thread::spawn(move || try_http_submit(&report_clone))
        .join()
        .unwrap_or(Err("Thread panicked".to_string()));

    match http_result {
        Ok(result) => result,
        Err(_) => {
            // Backend unreachable — queue locally
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
    }
}

/// Check the backend for status updates on tracked reports.
/// Returns empty vec if backend is unreachable (offline-first).
pub fn check_report_statuses(report_ids: &[String]) -> Vec<StatusUpdate> {
    if report_ids.is_empty() {
        return Vec::new();
    }

    // Run HTTP in a separate thread to avoid tokio runtime conflict
    let ids = report_ids.to_vec();
    std::thread::spawn(move || check_statuses_inner(&ids))
        .join()
        .unwrap_or_default()
}

/// Inner function that performs the actual HTTP status checks
fn check_statuses_inner(report_ids: &[String]) -> Vec<StatusUpdate> {
    let client = match build_client() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut updates = Vec::new();

    for report_id in report_ids {
        let url = format!("{}/reports/status?id={}", API_BASE, report_id);
        match client.get(&url).send() {
            Ok(response) if response.status().is_success() => {
                if let Ok(body) = response.text() {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        updates.push(StatusUpdate {
                            report_id: report_id.clone(),
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
            _ => {
                // Backend unreachable or error for this report — skip silently
            }
        }
    }

    updates
}

/// Build an HTTP client with appropriate timeout
fn build_client() -> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder()
        .timeout(SUBMIT_TIMEOUT)
        .build()
}

/// Try to submit a report via HTTP POST
fn try_http_submit(report: &ErrorReport) -> Result<SubmitResult, String> {
    let client = build_client().map_err(|e| e.to_string())?;

    let url = format!("{}/reports", API_BASE);
    let body = serde_json::to_string(report).map_err(|e| e.to_string())?;

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .map_err(|e| e.to_string())?;

    if response.status().is_success() {
        let report_id = report.report_id.clone();
        let tracking_url = format!("https://errors.cleanlanguage.dev/report/{}", report_id);

        // Try to parse response for server-assigned tracking URL
        if let Ok(body) = response.text() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                let tracking = json
                    .get("tracking_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&tracking_url)
                    .to_string();

                return Ok(SubmitResult::Submitted {
                    report_id,
                    tracking_url: tracking,
                });
            }
        }

        Ok(SubmitResult::Submitted {
            report_id,
            tracking_url,
        })
    } else {
        Err(format!("HTTP {}", response.status()))
    }
}
