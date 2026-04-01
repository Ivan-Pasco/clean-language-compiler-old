/*!
 * Telemetry Configuration
 *
 * Manages opt-in telemetry preferences stored in ~/.cleen/telemetry/config.toml.
 * All telemetry is disabled by default — users must explicitly opt in.
 */

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Consent level for error reporting — controls what data is included
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConsentLevel {
    /// Only error code, compiler version, OS — no code snippets
    ErrorOnly,
    /// Error code + AI-generated minimal reproduction (default)
    ErrorWithCode,
    /// Full report including AI analysis and suggested fix
    Full,
}

impl Default for ConsentLevel {
    fn default() -> Self {
        Self::ErrorWithCode
    }
}

impl std::fmt::Display for ConsentLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ErrorOnly => write!(f, "error_only"),
            Self::ErrorWithCode => write!(f, "error_with_code"),
            Self::Full => write!(f, "full"),
        }
    }
}

/// Telemetry configuration persisted in ~/.cleen/telemetry/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Whether telemetry is enabled (default: false)
    pub enabled: bool,
    /// What level of detail to include in reports
    pub consent_level: ConsentLevel,
    /// Random anonymous identifier (not linked to any personal info)
    pub anonymous_id: String,
    /// Whether the user has been prompted about telemetry (default: false)
    #[serde(default)]
    pub prompted: bool,
    /// Last compiler version seen (for detecting updates)
    #[serde(default)]
    pub last_seen_version: Option<String>,
    /// Optional contact email for fix notifications
    #[serde(default)]
    pub contact_email: Option<String>,
    /// Developer name (optional, for personalized notifications)
    #[serde(default)]
    pub developer_name: Option<String>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            consent_level: ConsentLevel::default(),
            anonymous_id: generate_anonymous_id(),
            prompted: false,
            last_seen_version: None,
            contact_email: None,
            developer_name: None,
        }
    }
}

impl TelemetryConfig {
    /// Returns the telemetry directory path: ~/.cleen/telemetry/
    pub fn telemetry_dir() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".cleen").join("telemetry"))
    }

    /// Returns the config file path: ~/.cleen/telemetry/config.toml
    fn config_path() -> Option<PathBuf> {
        Self::telemetry_dir().map(|d| d.join("config.toml"))
    }

    /// Load config from disk, creating defaults if file doesn't exist.
    /// Never fails — returns defaults on any error.
    pub fn load() -> Self {
        let path = match Self::config_path() {
            Some(p) => p,
            None => return Self::default(),
        };

        if !path.exists() {
            let config = Self::default();
            // Best-effort save of defaults (creates directory structure)
            let _ = config.save();
            return config;
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save config to disk. Creates the telemetry directory if needed.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let dir = Self::telemetry_dir().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Cannot determine home directory",
            )
        })?;

        std::fs::create_dir_all(&dir)?;

        let path = dir.join("config.toml");
        let contents = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        std::fs::write(path, contents)
    }
}

/// Generate a UUID v4-format anonymous ID using rand.
fn generate_anonymous_id() -> String {
    let mut rng = rand::thread_rng();
    let a: u32 = rng.gen();
    let b: u16 = rng.gen();
    let c: u16 = (rng.gen::<u16>() & 0x0FFF) | 0x4000; // version 4
    let d: u16 = (rng.gen::<u16>() & 0x3FFF) | 0x8000; // variant 1
    let e: u64 = rng.gen::<u64>() & 0xFFFF_FFFF_FFFF; // 48 bits

    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", a, b, c, d, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TelemetryConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.consent_level, ConsentLevel::ErrorWithCode);
        assert!(!config.anonymous_id.is_empty());
        // UUID v4 format: 8-4-4-4-12 hex chars
        assert_eq!(config.anonymous_id.len(), 36);
    }

    #[test]
    fn test_consent_level_display() {
        assert_eq!(ConsentLevel::ErrorOnly.to_string(), "error_only");
        assert_eq!(ConsentLevel::ErrorWithCode.to_string(), "error_with_code");
        assert_eq!(ConsentLevel::Full.to_string(), "full");
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = TelemetryConfig {
            enabled: true,
            consent_level: ConsentLevel::Full,
            anonymous_id: "test-id-1234".to_string(),
            prompted: true,
            last_seen_version: Some("0.30.0".to_string()),
            contact_email: Some("test@example.com".to_string()),
            developer_name: Some("Test Dev".to_string()),
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let deserialized: TelemetryConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(deserialized.enabled, true);
        assert_eq!(deserialized.consent_level, ConsentLevel::Full);
        assert_eq!(deserialized.anonymous_id, "test-id-1234");
    }

    #[test]
    fn test_generate_anonymous_id_format() {
        let id = generate_anonymous_id();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
        // version 4 marker
        assert!(parts[2].starts_with('4'));
    }

    #[test]
    fn test_telemetry_dir() {
        let dir = TelemetryConfig::telemetry_dir();
        assert!(dir.is_some());
        let path = dir.unwrap();
        assert!(path.ends_with("telemetry"));
        assert!(path.to_string_lossy().contains(".cleen"));
    }
}
