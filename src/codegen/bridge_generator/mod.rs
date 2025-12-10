//! Bridge generator for platform-specific runtime implementations
//!
//! Generates bridge files that provide platform-specific implementations
//! of I/O, filesystem, and HTTP functions for different targets.
//!
//! The WASM module itself is target-independent, but bridge files differ:
//! - Browser: JavaScript using web APIs (fetch, localStorage, console)
//! - Node.js: JavaScript using Node APIs (fs, http, process)
//! - iOS: Swift using Foundation (URLSession, FileManager)
//! - Android: Kotlin using Android APIs (OkHttp, File)
//! - Server: No bridge (clean-server provides runtime)

mod android;
mod browser;
mod ios;
mod node;

use crate::error::CompilerError;
use std::fs;
use std::path::{Path, PathBuf};

/// Target platform for bridge generation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeTarget {
    /// Server target - no bridge file needed (clean-server provides runtime)
    Server,
    /// Browser target - JavaScript for web browsers
    Browser,
    /// Node.js target - JavaScript for Node.js
    Node,
    /// iOS target - Swift for iOS/macOS
    #[allow(non_camel_case_types)]
    iOS,
    /// Android target - Kotlin for Android
    Android,
}

impl BridgeTarget {
    /// Parse a target string into a BridgeTarget
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "server" | "wasi" | "native" => Some(BridgeTarget::Server),
            "browser" | "web" => Some(BridgeTarget::Browser),
            "node" | "nodejs" => Some(BridgeTarget::Node),
            "ios" | "macos" | "apple" => Some(BridgeTarget::iOS),
            "android" => Some(BridgeTarget::Android),
            _ => None,
        }
    }

    /// Get the file extension for the main bridge file
    pub fn bridge_extension(&self) -> &'static str {
        match self {
            BridgeTarget::Server => "",
            BridgeTarget::Browser => "js",
            BridgeTarget::Node => "mjs",
            BridgeTarget::iOS => "swift",
            BridgeTarget::Android => "kt",
        }
    }

    /// Get human-readable name for this target
    pub fn name(&self) -> &'static str {
        match self {
            BridgeTarget::Server => "Server",
            BridgeTarget::Browser => "Browser",
            BridgeTarget::Node => "Node.js",
            BridgeTarget::iOS => "iOS",
            BridgeTarget::Android => "Android",
        }
    }
}

/// Bridge generator configuration
pub struct BridgeGenerator {
    target: BridgeTarget,
    output_dir: PathBuf,
    wasm_filename: String,
}

/// Result of bridge generation
pub struct BridgeGenerationResult {
    /// List of generated files
    pub generated_files: Vec<PathBuf>,
    /// Target that was used
    pub target: BridgeTarget,
}

impl BridgeGenerator {
    /// Create a new bridge generator
    pub fn new(
        target: BridgeTarget,
        output_dir: impl AsRef<Path>,
        wasm_filename: impl Into<String>,
    ) -> Self {
        Self {
            target,
            output_dir: output_dir.as_ref().to_path_buf(),
            wasm_filename: wasm_filename.into(),
        }
    }

    /// Generate bridge files for the configured target
    pub fn generate(&self) -> Result<BridgeGenerationResult, CompilerError> {
        // Ensure output directory exists
        if !self.output_dir.exists() {
            fs::create_dir_all(&self.output_dir).map_err(|e| {
                CompilerError::runtime_error(
                    format!("Failed to create output directory: {}", e),
                    None,
                    None,
                )
            })?;
        }

        let generated_files = match self.target {
            BridgeTarget::Server => vec![], // No bridge files needed
            BridgeTarget::Browser => self.generate_browser_bridge()?,
            BridgeTarget::Node => self.generate_node_bridge()?,
            BridgeTarget::iOS => self.generate_ios_bridge()?,
            BridgeTarget::Android => self.generate_android_bridge()?,
        };

        Ok(BridgeGenerationResult {
            generated_files,
            target: self.target,
        })
    }

    /// Generate browser (JavaScript) bridge files
    fn generate_browser_bridge(&self) -> Result<Vec<PathBuf>, CompilerError> {
        let bridge_content = browser::generate_bridge_js(&self.wasm_filename);
        let loader_content = browser::generate_loader_js(&self.wasm_filename);

        let bridge_path = self.output_dir.join("bridge.js");
        let loader_path = self.output_dir.join("loader.js");

        fs::write(&bridge_path, bridge_content).map_err(|e| {
            CompilerError::runtime_error(format!("Failed to write bridge.js: {}", e), None, None)
        })?;

        fs::write(&loader_path, loader_content).map_err(|e| {
            CompilerError::runtime_error(format!("Failed to write loader.js: {}", e), None, None)
        })?;

        Ok(vec![bridge_path, loader_path])
    }

    /// Generate Node.js bridge files
    fn generate_node_bridge(&self) -> Result<Vec<PathBuf>, CompilerError> {
        let bridge_content = node::generate_bridge_mjs(&self.wasm_filename);
        let run_content = node::generate_run_mjs(&self.wasm_filename);

        let bridge_path = self.output_dir.join("bridge.mjs");
        let run_path = self.output_dir.join("run.mjs");

        fs::write(&bridge_path, bridge_content).map_err(|e| {
            CompilerError::runtime_error(format!("Failed to write bridge.mjs: {}", e), None, None)
        })?;

        fs::write(&run_path, run_content).map_err(|e| {
            CompilerError::runtime_error(format!("Failed to write run.mjs: {}", e), None, None)
        })?;

        Ok(vec![bridge_path, run_path])
    }

    /// Generate iOS (Swift) bridge file
    fn generate_ios_bridge(&self) -> Result<Vec<PathBuf>, CompilerError> {
        let bridge_content = ios::generate_bridge_swift(&self.wasm_filename);
        let bridge_path = self.output_dir.join("CleanBridge.swift");

        fs::write(&bridge_path, bridge_content).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to write CleanBridge.swift: {}", e),
                None,
                None,
            )
        })?;

        Ok(vec![bridge_path])
    }

    /// Generate Android (Kotlin) bridge file
    fn generate_android_bridge(&self) -> Result<Vec<PathBuf>, CompilerError> {
        let bridge_content = android::generate_bridge_kt(&self.wasm_filename);
        let bridge_path = self.output_dir.join("CleanBridge.kt");

        fs::write(&bridge_path, bridge_content).map_err(|e| {
            CompilerError::runtime_error(
                format!("Failed to write CleanBridge.kt: {}", e),
                None,
                None,
            )
        })?;

        Ok(vec![bridge_path])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_parsing() {
        assert_eq!(
            BridgeTarget::from_str("browser"),
            Some(BridgeTarget::Browser)
        );
        assert_eq!(BridgeTarget::from_str("web"), Some(BridgeTarget::Browser));
        assert_eq!(BridgeTarget::from_str("node"), Some(BridgeTarget::Node));
        assert_eq!(BridgeTarget::from_str("nodejs"), Some(BridgeTarget::Node));
        assert_eq!(BridgeTarget::from_str("ios"), Some(BridgeTarget::iOS));
        assert_eq!(
            BridgeTarget::from_str("android"),
            Some(BridgeTarget::Android)
        );
        assert_eq!(BridgeTarget::from_str("server"), Some(BridgeTarget::Server));
        assert_eq!(BridgeTarget::from_str("unknown"), None);
    }

    #[test]
    fn test_target_extensions() {
        assert_eq!(BridgeTarget::Browser.bridge_extension(), "js");
        assert_eq!(BridgeTarget::Node.bridge_extension(), "mjs");
        assert_eq!(BridgeTarget::iOS.bridge_extension(), "swift");
        assert_eq!(BridgeTarget::Android.bridge_extension(), "kt");
        assert_eq!(BridgeTarget::Server.bridge_extension(), "");
    }
}
