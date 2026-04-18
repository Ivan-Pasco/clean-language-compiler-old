/*!
 * Local dev-mode error queue.
 *
 * Compile failures classified as dev-mode by [`dev_context`] are appended here
 * instead of being uploaded to the error dashboard. The queue is a plain JSON
 * array on disk at `~/.cleen/telemetry/dev_queue.json`. The CLI / tooling can
 * read it with `cln dev-queue` (future work) but for now it's a durable record
 * that a dev-time failure occurred without polluting the public tracker.
 *
 * Design goals:
 *  - Cheap: append-only, no locking, tolerant of concurrent writers (last
 *    writer wins is acceptable for a best-effort dev log).
 *  - Transparent: the on-disk shape is JSON so humans and tools can inspect it.
 *  - Bounded: entries over 500 are trimmed from the front so the file can't
 *    grow unbounded across dev loops.
 */

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use super::config::TelemetryConfig;
use super::dev_context::DevContext;

const MAX_ENTRIES: usize = 500;

fn queue_path() -> Option<PathBuf> {
    TelemetryConfig::telemetry_dir().map(|d| d.join("dev_queue.json"))
}

/// One failure captured while running in dev mode. Mirrors the shape of a
/// minimal server-bound report so future tooling can optionally promote an
/// entry by re-uploading it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevQueueEntry {
    pub timestamp: String,
    pub error_code: String,
    pub component: String,
    pub message: String,
    pub file_context: Option<String>,
    pub compiler_version: String,
    pub dev_reason: String,
}

/// Append `entry` to the dev queue. Silently no-ops on IO errors — the dev
/// queue is best-effort and must never break the compile flow.
pub fn append(entry: DevQueueEntry) {
    let Some(path) = queue_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut entries: Vec<DevQueueEntry> = match fs::read_to_string(&path) {
        Ok(body) if !body.trim().is_empty() => serde_json::from_str(&body).unwrap_or_default(),
        _ => Vec::new(),
    };

    entries.push(entry);
    if entries.len() > MAX_ENTRIES {
        let drop_count = entries.len() - MAX_ENTRIES;
        entries.drain(0..drop_count);
    }

    if let Ok(body) = serde_json::to_string_pretty(&entries) {
        let _ = fs::write(&path, body);
    }
}

/// Read the dev queue. Returns an empty Vec if the file is missing or unreadable.
#[allow(dead_code)]
pub fn load() -> Vec<DevQueueEntry> {
    let Some(path) = queue_path() else {
        return Vec::new();
    };
    match fs::read_to_string(&path) {
        Ok(body) if !body.trim().is_empty() => serde_json::from_str(&body).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Build an entry from the same inputs `report_compile_failure` has on hand.
pub fn entry_from(
    ctx: &DevContext,
    error_code: &str,
    component: &str,
    message: &str,
    file_context: Option<&str>,
    compiler_version: &str,
) -> DevQueueEntry {
    let timestamp = chrono_now_iso8601();
    DevQueueEntry {
        timestamp,
        error_code: error_code.to_string(),
        component: component.to_string(),
        message: message.to_string(),
        file_context: file_context.map(|s| s.to_string()),
        compiler_version: compiler_version.to_string(),
        dev_reason: super::dev_context::describe(ctx).to_string(),
    }
}

/// Minimal ISO-8601 timestamp without pulling a date crate. Uses seconds since
/// epoch rendered as an RFC3339-ish UTC string via `SystemTime`.
fn chrono_now_iso8601() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;

    // Convert seconds-since-epoch to UTC date/time components. Uses the
    // standard proleptic Gregorian calendar. The arithmetic is good for any
    // year 1970..=~10000 which is well past what we need.
    let (year, month, day, hh, mm, ss) = epoch_to_ymdhms(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hh, mm, ss
    )
}

fn epoch_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let time = secs.rem_euclid(86_400) as u32;
    let hh = time / 3600;
    let mm = (time % 3600) / 60;
    let ss = time % 60;

    // Days since 1970-01-01. Use Howard Hinnant's civil_from_days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (year, m, d, hh, mm, ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_formatting_matches_known_dates() {
        // 2020-01-01 00:00:00 UTC = 1577836800
        let (y, m, d, hh, mm, ss) = epoch_to_ymdhms(1_577_836_800);
        assert_eq!((y, m, d, hh, mm, ss), (2020, 1, 1, 0, 0, 0));
        // 2026-04-18 21:07:14 UTC = 1776546434
        let (y, m, d, hh, mm, ss) = epoch_to_ymdhms(1_776_546_434);
        assert_eq!((y, m, d, hh, mm, ss), (2026, 4, 18, 21, 7, 14));
    }

    #[test]
    fn iso_timestamp_has_expected_shape() {
        let ts = chrono_now_iso8601();
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
    }
}
