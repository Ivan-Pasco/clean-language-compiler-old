/*!
 * Telemetry Configuration
 *
 * The `telemetry` switch stored in ~/.cleen/telemetry/config.toml. When on, a
 * failed compile that points at a toolchain bug is recorded in the local dev
 * queue (dev work) or answered with the GitHub issue command to report it.
 * Nothing is ever sent over the network. Off by default; it is only turned
 * on with `cln config set telemetry on`.
 */

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Telemetry configuration persisted in ~/.cleen/telemetry/config.toml.
///
/// Keys written by older compilers (consent level, anonymous id, contact
/// email, ...) are ignored when the file is read.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Whether telemetry is enabled (default: false)
    #[serde(default)]
    pub enabled: bool,
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

    /// Load config from disk. A missing or unreadable file means the
    /// defaults (telemetry off); loading never writes anything.
    pub fn load() -> Self {
        Self::config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|contents| Self::from_toml(&contents))
            .unwrap_or_default()
    }

    fn from_toml(contents: &str) -> Self {
        toml::from_str(contents).unwrap_or_default()
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
        let contents = toml::to_string_pretty(self).map_err(std::io::Error::other)?;

        std::fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_is_off_by_default() {
        assert!(!TelemetryConfig::default().enabled);
    }

    #[test]
    fn config_files_from_older_compilers_still_load() {
        let old = "enabled = true\nconsent_level = \"error_with_code\"\n\
                   anonymous_id = \"abc\"\nprompted = true\ncontact_email = \"a@b.c\"\n";
        assert!(TelemetryConfig::from_toml(old).enabled);
        assert!(!TelemetryConfig::from_toml("prompted = true\n").enabled);
    }

    #[test]
    fn config_serialization_roundtrip() {
        let config = TelemetryConfig { enabled: true };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(TelemetryConfig::from_toml(&toml_str).enabled);
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
