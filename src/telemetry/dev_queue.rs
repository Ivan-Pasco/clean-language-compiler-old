/*!
 * Local dev-mode error queue.
 *
 * Compile failures classified as dev-mode by [`dev_context`] are appended here
 * instead of being uploaded to the error dashboard. The queue lives at
 * `~/.cleen/telemetry/dev_queue.json` as a JSON array of entries.
 *
 * Design:
 *  - **Dedup by fingerprint.** Repeating the same failure bumps an occurrence
 *    counter instead of appending a new row, so the queue stays small even
 *    when a dev loop fires a thousand times. The fingerprint is a short SHA-256
 *    prefix of `(component, error_code, canonical_message)`.
 *  - **First seen / last seen stamps.** Lets the CLI sort by recency and age.
 *  - **Best-effort IO.** Never blocks or panics the compile flow. The queue is
 *    a convenience for the developer, not a critical path.
 *  - **Bounded.** Capped at 500 distinct entries; oldest by `last_seen_at` are
 *    trimmed if the cap is exceeded.
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

/// One failure captured while running in dev mode. Deduplicated by
/// `fingerprint`; repeated captures bump `occurrences` and `last_seen_at`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevQueueEntry {
    /// 16-char SHA-256 prefix of (component, error_code, message).
    pub fingerprint: String,
    /// ISO-8601 UTC timestamp of the first capture.
    pub first_seen_at: String,
    /// ISO-8601 UTC timestamp of the most recent capture.
    pub last_seen_at: String,
    /// Number of times this fingerprint has been captured.
    #[serde(default = "default_occurrences")]
    pub occurrences: u32,
    pub error_code: String,
    pub component: String,
    pub message: String,
    pub file_context: Option<String>,
    pub compiler_version: String,
    pub dev_reason: String,
}

fn default_occurrences() -> u32 {
    1
}

/// Result returned by `append` so callers can emit a helpful hint.
#[derive(Debug, Clone)]
pub struct AppendOutcome {
    pub fingerprint: String,
    pub occurrences: u32,
    /// True when this fingerprint existed before and we incremented it.
    pub deduplicated: bool,
}

/// Append a capture to the queue. If `fingerprint` already exists, increments
/// `occurrences` and refreshes `last_seen_at` instead of adding a new row.
/// Returns the fingerprint + current occurrence count so the caller can print
/// a hint like "(E007 ×3)".
pub fn append(entry: DevQueueEntry) -> AppendOutcome {
    let Some(path) = queue_path() else {
        return AppendOutcome {
            fingerprint: entry.fingerprint.clone(),
            occurrences: entry.occurrences,
            deduplicated: false,
        };
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut entries: Vec<DevQueueEntry> = load_from(&path);

    let outcome = match entries
        .iter_mut()
        .find(|e| e.fingerprint == entry.fingerprint)
    {
        Some(existing) => {
            existing.occurrences = existing.occurrences.saturating_add(1);
            existing.last_seen_at = entry.last_seen_at.clone();
            // Pick up the latest values for fields that may evolve (e.g. the
            // compiler version after an upgrade).
            existing.compiler_version = entry.compiler_version.clone();
            existing.dev_reason = entry.dev_reason.clone();
            if entry.file_context.is_some() {
                existing.file_context = entry.file_context.clone();
            }
            AppendOutcome {
                fingerprint: existing.fingerprint.clone(),
                occurrences: existing.occurrences,
                deduplicated: true,
            }
        }
        None => {
            let outcome = AppendOutcome {
                fingerprint: entry.fingerprint.clone(),
                occurrences: entry.occurrences,
                deduplicated: false,
            };
            entries.push(entry);
            outcome
        }
    };

    if entries.len() > MAX_ENTRIES {
        entries.sort_by(|a, b| a.last_seen_at.cmp(&b.last_seen_at));
        let drop_count = entries.len() - MAX_ENTRIES;
        entries.drain(0..drop_count);
    }

    if let Ok(body) = serde_json::to_string_pretty(&entries) {
        let _ = fs::write(&path, body);
    }
    outcome
}

fn load_from(path: &std::path::Path) -> Vec<DevQueueEntry> {
    match fs::read_to_string(path) {
        Ok(body) if !body.trim().is_empty() => serde_json::from_str(&body).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Read every entry. Empty vec if the file is missing or unreadable.
pub fn load() -> Vec<DevQueueEntry> {
    let Some(path) = queue_path() else {
        return Vec::new();
    };
    load_from(&path)
}

/// Total entry count (distinct fingerprints, not total occurrences).
pub fn count() -> usize {
    load().len()
}

/// Remove the entry whose fingerprint starts with `prefix`. Returns true if
/// something was removed. A prefix of 4+ characters is recommended; shorter
/// prefixes may match multiple entries, in which case none are removed and
/// the function returns false.
pub fn clear_by_prefix(prefix: &str) -> Result<bool, String> {
    if prefix.len() < 4 {
        return Err("prefix must be at least 4 characters".to_string());
    }
    let Some(path) = queue_path() else {
        return Ok(false);
    };
    let entries = load_from(&path);
    let matches: Vec<&DevQueueEntry> = entries
        .iter()
        .filter(|e| e.fingerprint.starts_with(prefix))
        .collect();
    if matches.is_empty() {
        return Ok(false);
    }
    if matches.len() > 1 {
        return Err(format!(
            "ambiguous prefix '{}' matches {} entries",
            prefix,
            matches.len()
        ));
    }
    let target = matches[0].fingerprint.clone();
    let remaining: Vec<DevQueueEntry> = entries
        .into_iter()
        .filter(|e| e.fingerprint != target)
        .collect();
    let body = serde_json::to_string_pretty(&remaining).map_err(|e| format!("serialize: {}", e))?;
    fs::write(&path, body).map_err(|e| format!("write: {}", e))?;
    Ok(true)
}

/// Remove every entry.
pub fn clear_all() -> Result<usize, String> {
    let Some(path) = queue_path() else {
        return Ok(0);
    };
    let n = load_from(&path).len();
    fs::write(&path, "[]").map_err(|e| format!("write: {}", e))?;
    Ok(n)
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
    let timestamp = now_iso8601();
    let fingerprint = compute_fingerprint(component, error_code, message);
    DevQueueEntry {
        fingerprint,
        first_seen_at: timestamp.clone(),
        last_seen_at: timestamp,
        occurrences: 1,
        error_code: error_code.to_string(),
        component: component.to_string(),
        message: message.to_string(),
        file_context: file_context.map(|s| s.to_string()),
        compiler_version: compiler_version.to_string(),
        dev_reason: super::dev_context::describe(ctx).to_string(),
    }
}

/// 16-char hex fingerprint derived from `(component, error_code, canonical_message)`.
/// Canonicalisation: trim + lowercase ASCII + collapse runs of whitespace so
/// cosmetic differences in error messages don't defeat dedup. The first
/// parenthesised byte-offset like `offset=0x462c (17964)` is preserved since it
/// is usually the discriminator we want to keep.
///
/// Uses FNV-1a 64-bit — not cryptographic, but the dev queue only needs
/// identity within one machine, and FNV-1a has no external dependency.
fn compute_fingerprint(component: &str, error_code: &str, message: &str) -> String {
    let canonical = canonicalize(message);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset basis
    for bytes in [
        component.as_bytes(),
        b"|",
        error_code.as_bytes(),
        b"|",
        canonical.as_bytes(),
    ] {
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3); // FNV prime
        }
    }
    format!("{:016x}", hash)
}

fn canonicalize(msg: &str) -> String {
    let lower = msg.trim().to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev_ws = false;
    for ch in lower.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out
}

/// ISO-8601 UTC timestamp without pulling a date crate.
fn now_iso8601() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let (year, month, day, hh, mm, ss) = epoch_to_ymdhms(now.as_secs() as i64);
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
        let (y, m, d, hh, mm, ss) = epoch_to_ymdhms(1_577_836_800);
        assert_eq!((y, m, d, hh, mm, ss), (2020, 1, 1, 0, 0, 0));
        let (y, m, d, hh, mm, ss) = epoch_to_ymdhms(1_776_546_434);
        assert_eq!((y, m, d, hh, mm, ss), (2026, 4, 18, 21, 7, 14));
    }

    #[test]
    fn iso_timestamp_has_expected_shape() {
        let ts = now_iso8601();
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn fingerprint_is_stable_for_cosmetic_whitespace() {
        let a = compute_fingerprint("compiler", "E007", "Type mismatch:  expected i32");
        let b = compute_fingerprint("compiler", "E007", "type mismatch: expected i32");
        let c = compute_fingerprint("compiler", "E007", "\tType mismatch: expected i32\n");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn fingerprint_differs_for_different_messages() {
        let a = compute_fingerprint("compiler", "E007", "type mismatch at 0x3d2c");
        let b = compute_fingerprint("compiler", "E007", "type mismatch at 0x462c");
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_differs_for_different_components() {
        let a = compute_fingerprint("compiler", "E007", "foo");
        let b = compute_fingerprint("server", "E007", "foo");
        assert_ne!(a, b);
    }
}
