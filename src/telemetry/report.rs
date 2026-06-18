/*!
 * Error Report Schema and Local Report Store
 *
 * Defines the ErrorReport structure (matching spec Section 3.1) and
 * ReportStore for tracking reported errors locally for fix notifications.
 */

use super::config::TelemetryConfig;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============================================================================
// Error Report Schema (matches ERROR_REPORTING_SPECIFICATION.md Section 3.1)
// ============================================================================

/// A structured error report ready for submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorReport {
    pub schema_version: String,
    pub report_id: String,
    pub timestamp: DateTime<Utc>,
    pub source: ReportSource,
    pub error: ReportError,
    pub reproduction: Option<ReportReproduction>,
    pub ai_context: Option<ReportAiContext>,
    pub user: ReportUser,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSource {
    pub channel: String,
    pub compiler_version: String,
    pub os: String,
    pub arch: String,
    pub runtime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportError {
    pub code: String,
    pub category: String,
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subsystem: Option<String>,
    pub severity: String,
    pub message: String,
    pub file_context: Option<String>,
    /// The specific component the bug actually lives in, when different from
    /// the reporting component (e.g., component="compiler" but
    /// affected_component="framework" for a plugin-compilation bug).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected_component: Option<String>,
    /// Version of the affected component where the bug was observed
    /// (e.g., "2.6.1" for frame.data 2.6.1). Distinct from the compiler
    /// version in ReportSource, which is always the compiler.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportReproduction {
    pub minimal_code: Option<String>,
    pub expected_behavior: Option<String>,
    pub actual_behavior: Option<String>,
    pub spec_reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportAiContext {
    pub analysis: Option<String>,
    pub suggested_component: Option<String>,
    pub suggested_fix: Option<String>,
    pub confidence: Option<String>,
    /// Short plain-English summary for the dashboard tooltip (≤300 chars).
    /// Aimed at non-engineers browsing the dashboard, not the team.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendly_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportUser {
    pub anonymous: bool,
    pub contact: Option<String>,
    pub consent_level: String,
}

impl ErrorReport {
    /// Build a new report with system info auto-populated
    pub fn new(report_id: String, error: ReportError, channel: &str, consent_level: &str) -> Self {
        Self {
            schema_version: "1.0.0".to_string(),
            report_id,
            timestamp: Utc::now(),
            source: ReportSource {
                channel: channel.to_string(),
                compiler_version: crate::VERSION.to_string(),
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
                runtime: None,
            },
            error,
            reproduction: None,
            ai_context: None,
            user: ReportUser {
                anonymous: true,
                contact: None,
                consent_level: consent_level.to_string(),
            },
        }
    }
}

// ============================================================================
// Local Report Tracking (for fix notifications)
// ============================================================================

/// Status of a tracked report
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Reported,
    Acknowledged,
    InProgress,
    Resolved,
    WontFix,
}

impl std::fmt::Display for ReportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reported => write!(f, "reported"),
            Self::Acknowledged => write!(f, "acknowledged"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Resolved => write!(f, "resolved"),
            Self::WontFix => write!(f, "wont_fix"),
        }
    }
}

/// A locally tracked report for fix notification purposes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedReport {
    pub report_id: String,
    pub fingerprint: Option<String>,
    pub error_code: String,
    pub component: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subsystem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    pub summary: String,
    pub reported_at: DateTime<Utc>,
    pub compiler_version: String,
    pub status: ReportStatus,
    pub resolved_in: Option<String>,
    pub fix_description: Option<String>,
    pub fix_pr: Option<String>,
    pub last_checked: DateTime<Utc>,
    pub notified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimal_repro: Option<String>,
}

/// Persistent store of tracked reports in ~/.cleen/telemetry/reported_errors.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReportStore {
    pub reports: Vec<TrackedReport>,
}

impl ReportStore {
    /// Path to the report store file
    fn store_path() -> Option<std::path::PathBuf> {
        TelemetryConfig::telemetry_dir().map(|d| d.join("reported_errors.json"))
    }

    /// Load from disk. Returns empty store on any error.
    pub fn load() -> Self {
        let path = match Self::store_path() {
            Some(p) => p,
            None => return Self::default(),
        };

        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save to disk. Creates directory if needed.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let dir = TelemetryConfig::telemetry_dir().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Cannot determine home directory",
            )
        })?;

        std::fs::create_dir_all(&dir)?;

        let path = dir.join("reported_errors.json");
        let contents = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        std::fs::write(path, contents)
    }

    /// Add a new report to the tracking store
    pub fn add_report(&mut self, report: &ErrorReport) {
        // Avoid duplicates by report_id
        if self.reports.iter().any(|r| r.report_id == report.report_id) {
            return;
        }

        self.reports.push(TrackedReport {
            report_id: report.report_id.clone(),
            fingerprint: None,
            error_code: report.error.code.clone(),
            component: Some(report.error.component.clone()),
            subsystem: report.error.subsystem.clone(),
            severity: Some(report.error.severity.clone()),
            summary: report.error.message.clone(),
            reported_at: report.timestamp,
            compiler_version: report.source.compiler_version.clone(),
            status: ReportStatus::Reported,
            resolved_in: None,
            fix_description: None,
            fix_pr: None,
            last_checked: Utc::now(),
            notified: false,
            minimal_repro: report
                .reproduction
                .as_ref()
                .and_then(|r| r.minimal_code.clone()),
        });
    }

    /// Get reports that have changed since their last_checked timestamp
    pub fn get_updated_reports(&self) -> Vec<&TrackedReport> {
        self.reports
            .iter()
            .filter(|r| !r.notified && r.status != ReportStatus::Reported)
            .collect()
    }

    /// Get all tracked reports
    pub fn get_all_reports(&self) -> &[TrackedReport] {
        &self.reports
    }

    /// Update a report's status by report_id
    pub fn update_status(
        &mut self,
        report_id: &str,
        status: ReportStatus,
        resolved_in: Option<String>,
        fix_description: Option<String>,
        fix_pr: Option<String>,
    ) {
        if let Some(report) = self.reports.iter_mut().find(|r| r.report_id == report_id) {
            report.status = status;
            report.resolved_in = resolved_in;
            report.fix_description = fix_description;
            report.fix_pr = fix_pr;
            report.last_checked = Utc::now();
        }
    }

    /// Mark a report as notified (user has been told about the fix)
    pub fn mark_notified(&mut self, report_id: &str) {
        if let Some(report) = self.reports.iter_mut().find(|r| r.report_id == report_id) {
            report.notified = true;
        }
    }

    /// Store the server-returned fingerprint for a report
    pub fn update_fingerprint(&mut self, report_id: &str, fingerprint: &str) {
        if let Some(report) = self.reports.iter_mut().find(|r| r.report_id == report_id) {
            report.fingerprint = Some(fingerprint.to_string());
            report.last_checked = Utc::now();
        }
    }

    /// Reports that were accepted locally but never confirmed by the backend
    /// (no fingerprint returned). These are candidates for re-submission.
    pub fn unposted_report_ids(&self) -> Vec<String> {
        self.reports
            .iter()
            .filter(|r| r.fingerprint.is_none() && r.status == ReportStatus::Reported)
            .map(|r| r.report_id.clone())
            .collect()
    }

    /// True if a report with the same error_code and message was already submitted
    /// within the last 60 seconds. Prevents burst-submission of identical errors
    /// when a test suite triggers the same compiler failure on many files simultaneously.
    pub fn has_recent_duplicate(&self, error_code: &str, message: &str) -> bool {
        let window = chrono::Duration::seconds(60);
        let now = Utc::now();
        self.reports.iter().any(|r| {
            r.error_code == error_code && r.summary == message && (now - r.reported_at) < window
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_report() -> ErrorReport {
        ErrorReport::new(
            "test-report-001".to_string(),
            ReportError {
                code: "SYN042".to_string(),
                category: "syntax".to_string(),
                component: "compiler".to_string(),
                subsystem: Some("parser".to_string()),
                severity: "bug".to_string(),
                message: "Nested generic types fail to parse".to_string(),
                file_context: None,
                affected_component: None,
                affected_version: None,
            },
            "mcp_ai",
            "error_with_code",
        )
    }

    #[test]
    fn test_error_report_new() {
        let report = make_test_report();
        assert_eq!(report.schema_version, "1.0.0");
        assert_eq!(report.source.os, std::env::consts::OS);
        assert_eq!(report.source.arch, std::env::consts::ARCH);
        assert!(report.user.anonymous);
    }

    #[test]
    fn test_report_store_add_report() {
        let mut store = ReportStore::default();
        let report = make_test_report();

        store.add_report(&report);
        assert_eq!(store.reports.len(), 1);
        assert_eq!(store.reports[0].error_code, "SYN042");
        assert_eq!(store.reports[0].status, ReportStatus::Reported);
    }

    #[test]
    fn test_report_store_dedup() {
        let mut store = ReportStore::default();
        let report = make_test_report();

        store.add_report(&report);
        store.add_report(&report); // same report_id
        assert_eq!(store.reports.len(), 1);
    }

    #[test]
    fn test_report_store_update_status() {
        let mut store = ReportStore::default();
        let report = make_test_report();
        store.add_report(&report);

        store.update_status(
            "test-report-001",
            ReportStatus::Resolved,
            Some("0.16.0".to_string()),
            Some("Fixed bracket depth tracking".to_string()),
            Some("#247".to_string()),
        );

        assert_eq!(store.reports[0].status, ReportStatus::Resolved);
        assert_eq!(store.reports[0].resolved_in.as_deref(), Some("0.16.0"));
    }

    #[test]
    fn test_get_updated_reports() {
        let mut store = ReportStore::default();
        let report = make_test_report();
        store.add_report(&report);

        // Newly reported — not yet "updated"
        assert_eq!(store.get_updated_reports().len(), 0);

        // Mark as resolved
        store.update_status("test-report-001", ReportStatus::Resolved, None, None, None);
        assert_eq!(store.get_updated_reports().len(), 1);

        // Mark as notified
        store.mark_notified("test-report-001");
        assert_eq!(store.get_updated_reports().len(), 0);
    }

    #[test]
    fn test_update_fingerprint_sets_field() {
        let mut store = ReportStore::default();
        let report = make_test_report();
        store.add_report(&report);
        assert!(store.reports[0].fingerprint.is_none());

        store.update_fingerprint("test-report-001", "abc123");
        assert_eq!(store.reports[0].fingerprint.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_unposted_report_ids_returns_only_uncomfirmed() {
        let mut store = ReportStore::default();
        let report = make_test_report();
        store.add_report(&report);

        // Freshly-added report with no fingerprint is unposted
        assert_eq!(store.unposted_report_ids(), vec!["test-report-001"]);

        // After fingerprint is set, it's no longer unposted
        store.update_fingerprint("test-report-001", "fp-xyz");
        assert!(store.unposted_report_ids().is_empty());
    }

    #[test]
    fn test_report_serialization_roundtrip() {
        let mut store = ReportStore::default();
        store.add_report(&make_test_report());

        let json = serde_json::to_string_pretty(&store).unwrap();
        let deserialized: ReportStore = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.reports.len(), 1);
        assert_eq!(deserialized.reports[0].report_id, "test-report-001");
    }
}
