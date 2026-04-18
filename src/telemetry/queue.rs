/*!
 * Pending Report Queue
 *
 * Manages offline report storage in ~/.cleen/telemetry/pending_reports/.
 * Reports are queued when the backend API is unreachable and flushed later.
 */

use super::config::TelemetryConfig;
use super::report::ErrorReport;
use std::path::PathBuf;

/// Maximum number of pending reports to keep (oldest dropped first)
const MAX_PENDING_REPORTS: usize = 50;

/// Result of a flush operation
#[derive(Debug)]
pub struct FlushResult {
    pub sent: usize,
    pub failed: usize,
    pub remaining: usize,
}

/// Queue for reports that couldn't be submitted to the backend
pub struct PendingQueue {
    queue_dir: PathBuf,
}

impl PendingQueue {
    /// Create a new queue pointing to ~/.cleen/telemetry/pending_reports/
    pub fn new() -> Option<Self> {
        let dir = TelemetryConfig::telemetry_dir()?.join("pending_reports");
        Some(Self { queue_dir: dir })
    }

    /// Ensure the queue directory exists
    fn ensure_dir(&self) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(&self.queue_dir)
    }

    /// Save a report as a JSON file in the queue directory
    pub fn enqueue(&self, report: &ErrorReport) -> Result<PathBuf, std::io::Error> {
        self.ensure_dir()?;
        self.enforce_cap()?;

        let filename = format!("{}.json", report.report_id);
        let path = self.queue_dir.join(&filename);

        let contents = serde_json::to_string_pretty(report)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        std::fs::write(&path, contents)?;
        Ok(path)
    }

    /// List all pending reports from the queue directory
    pub fn list_pending(&self) -> Vec<ErrorReport> {
        let entries = match std::fs::read_dir(&self.queue_dir) {
            Ok(entries) => entries,
            Err(_) => return Vec::new(),
        };

        let mut reports = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Ok(report) = serde_json::from_str::<ErrorReport>(&contents) {
                        reports.push(report);
                    }
                }
            }
        }

        // Sort by timestamp (oldest first) for FIFO processing
        reports.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        reports
    }

    /// Remove a successfully submitted report from the queue
    pub fn dequeue(&self, report_id: &str) -> Result<(), std::io::Error> {
        let filename = format!("{}.json", report_id);
        let path = self.queue_dir.join(filename);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Count pending reports
    pub fn count(&self) -> usize {
        match std::fs::read_dir(&self.queue_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
                .count(),
            Err(_) => 0,
        }
    }

    /// Enforce the maximum queue size by removing oldest reports
    fn enforce_cap(&self) -> Result<(), std::io::Error> {
        let entries = match std::fs::read_dir(&self.queue_dir) {
            Ok(entries) => entries,
            Err(_) => return Ok(()),
        };

        let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(metadata) = path.metadata() {
                    let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
                    files.push((path, modified));
                }
            }
        }

        // If under cap, nothing to do
        if files.len() < MAX_PENDING_REPORTS {
            return Ok(());
        }

        // Sort by modification time (oldest first)
        files.sort_by(|a, b| a.1.cmp(&b.1));

        // Remove oldest files to get back under cap
        let to_remove = files.len() - MAX_PENDING_REPORTS + 1; // +1 to make room for the new one
        for (path, _) in files.iter().take(to_remove) {
            let _ = std::fs::remove_file(path);
        }

        Ok(())
    }

    /// Attempt to flush all pending reports via the submit function.
    /// Returns how many were sent, failed, and remain.
    pub fn flush<F>(&self, mut submit_fn: F) -> FlushResult
    where
        F: FnMut(&ErrorReport) -> bool,
    {
        let reports = self.list_pending();
        let mut sent = 0;
        let mut failed = 0;

        for report in &reports {
            if submit_fn(report) {
                let _ = self.dequeue(&report.report_id);
                sent += 1;
            } else {
                failed += 1;
            }
        }

        FlushResult {
            sent,
            failed,
            remaining: self.count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::report::{ReportError, ReportSource, ReportUser};
    use chrono::Utc;

    fn make_test_report(id: &str) -> ErrorReport {
        ErrorReport {
            schema_version: "1.0.0".to_string(),
            report_id: id.to_string(),
            timestamp: Utc::now(),
            source: ReportSource {
                channel: "test".to_string(),
                compiler_version: "0.30.0".to_string(),
                os: "test".to_string(),
                arch: "test".to_string(),
                runtime: None,
            },
            error: ReportError {
                code: "SYN001".to_string(),
                category: "syntax".to_string(),
                component: "compiler".to_string(),
                subsystem: Some("parser".to_string()),
                severity: "bug".to_string(),
                message: "Test error".to_string(),
                file_context: None,
            },
            reproduction: None,
            ai_context: None,
            user: ReportUser {
                anonymous: true,
                contact: None,
                consent_level: "error_with_code".to_string(),
            },
        }
    }

    #[test]
    fn test_queue_operations() {
        let temp_dir = std::env::temp_dir().join("cln_test_queue");
        let _ = std::fs::remove_dir_all(&temp_dir);

        let queue = PendingQueue {
            queue_dir: temp_dir.clone(),
        };

        // Enqueue
        let report = make_test_report("test-q-001");
        let path = queue.enqueue(&report).unwrap();
        assert!(path.exists());
        assert_eq!(queue.count(), 1);

        // List
        let pending = queue.list_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].report_id, "test-q-001");

        // Dequeue
        queue.dequeue("test-q-001").unwrap();
        assert_eq!(queue.count(), 0);

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_queue_flush() {
        let temp_dir = std::env::temp_dir().join("cln_test_flush");
        let _ = std::fs::remove_dir_all(&temp_dir);

        let queue = PendingQueue {
            queue_dir: temp_dir.clone(),
        };

        queue.enqueue(&make_test_report("flush-001")).unwrap();
        queue.enqueue(&make_test_report("flush-002")).unwrap();

        // Submit first succeeds, second fails
        let mut count = 0;
        let result = queue.flush(|_| {
            count += 1;
            count == 1 // only first succeeds
        });

        assert_eq!(result.sent, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.remaining, 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
