use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompileOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub flag: Option<String>,
    pub default: bool,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutually_exclusive: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompilePreset {
    pub id: String,
    pub label: String,
    pub description: String,
    pub flags: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CompileOptionsSchema {
    pub version: String,
    pub compiler_version: String,
    pub generated_at: String,
    pub targets: Vec<CompileOption>,
    pub optimizations: Vec<CompileOption>,
    pub runtimes: Vec<CompileOption>,
    pub flags: Vec<CompileOption>,
    pub presets: Vec<CompilePreset>,
}

impl CompileOptionsSchema {
    /// Create the compile options schema based on current compiler capabilities
    pub fn generate() -> Self {
        Self {
            version: "1.0.0".to_string(),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: Utc::now().to_rfc3339(),
            targets: Self::get_available_targets(),
            optimizations: Self::get_available_optimizations(),
            runtimes: Self::get_available_runtimes(),
            flags: Self::get_available_flags(),
            presets: Self::get_available_presets(),
        }
    }

    fn get_available_targets() -> Vec<CompileOption> {
        vec![
            CompileOption {
                id: "web".to_string(),
                label: "🌐 Web".to_string(),
                description: "WebAssembly for web browsers".to_string(),
                flag: Some("--target".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "nodejs".to_string(),
                label: "🟢 Node.js".to_string(),
                description: "WebAssembly for Node.js runtime".to_string(),
                flag: Some("--target".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "native".to_string(),
                label: "💻 Native".to_string(),
                description: "Native desktop/server applications".to_string(),
                flag: Some("--target".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "embedded".to_string(),
                label: "🔧 Embedded".to_string(),
                description: "Embedded systems with resource constraints".to_string(),
                flag: Some("--target".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "wasi".to_string(),
                label: "🌍 WASI".to_string(),
                description: "WebAssembly System Interface for portable system integration"
                    .to_string(),
                flag: Some("--target".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "auto".to_string(),
                label: "🤖 Auto".to_string(),
                description: "Automatically detect best target".to_string(),
                flag: None,
                default: true,
                available: true,
                mutually_exclusive: None,
            },
        ]
    }

    fn get_available_optimizations() -> Vec<CompileOption> {
        vec![
            CompileOption {
                id: "development".to_string(),
                label: "🔧 Development".to_string(),
                description: "Fast compilation, basic optimizations".to_string(),
                flag: Some("--optimization".to_string()),
                default: true,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "production".to_string(),
                label: "🚀 Production".to_string(),
                description: "Full optimizations for release builds".to_string(),
                flag: Some("--optimization".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "size".to_string(),
                label: "📦 Size".to_string(),
                description: "Optimize for smaller binary size".to_string(),
                flag: Some("--optimization".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "speed".to_string(),
                label: "⚡ Speed".to_string(),
                description: "Optimize for runtime performance".to_string(),
                flag: Some("--optimization".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "debug".to_string(),
                label: "🐛 Debug".to_string(),
                description: "No optimizations, maximum debug info".to_string(),
                flag: Some("--optimization".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
        ]
    }

    fn get_available_runtimes() -> Vec<CompileOption> {
        vec![
            CompileOption {
                id: "auto".to_string(),
                label: "🤖 Auto".to_string(),
                description: "Automatically detect best runtime".to_string(),
                flag: None,
                default: true,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "wasmtime".to_string(),
                label: "⚡ Wasmtime".to_string(),
                description: "Fast and secure WebAssembly runtime".to_string(),
                flag: Some("--runtime".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
            CompileOption {
                id: "wasmer".to_string(),
                label: "🦀 Wasmer".to_string(),
                description: "Universal WebAssembly runtime".to_string(),
                flag: Some("--runtime".to_string()),
                default: false,
                available: true,
                mutually_exclusive: None,
            },
        ]
    }

    fn get_available_flags() -> Vec<CompileOption> {
        vec![
            CompileOption {
                id: "debug".to_string(),
                label: "🐛 Include debug information".to_string(),
                description: "Add debug symbols for debugging".to_string(),
                flag: Some("--debug".to_string()),
                default: false,
                available: true,
                mutually_exclusive: Some(vec![]),
            },
            CompileOption {
                id: "verbose".to_string(),
                label: "💬 Verbose output".to_string(),
                description: "Show detailed compilation information".to_string(),
                flag: Some("--verbose".to_string()),
                default: false,
                available: true,
                mutually_exclusive: Some(vec![]),
            },
        ]
    }

    fn get_available_presets() -> Vec<CompilePreset> {
        vec![
            CompilePreset {
                id: "standard".to_string(),
                label: "📋 Standard compilation".to_string(),
                description: "No additional options".to_string(),
                flags: vec![],
            },
            CompilePreset {
                id: "debug_only".to_string(),
                label: "🐛 Include debug information".to_string(),
                description: "Add debug symbols for debugging".to_string(),
                flags: vec!["debug".to_string()],
            },
            CompilePreset {
                id: "verbose_only".to_string(),
                label: "💬 Verbose output".to_string(),
                description: "Show detailed compilation information".to_string(),
                flags: vec!["verbose".to_string()],
            },
            CompilePreset {
                id: "debug_verbose".to_string(),
                label: "🐛💬 Debug + Verbose".to_string(),
                description: "Include debug info and show verbose output".to_string(),
                flags: vec!["debug".to_string(), "verbose".to_string()],
            },
        ]
    }

    /// Export the schema to a JSON file
    pub fn export_to_file(&self, path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Get the default installation path for the options file
    pub fn get_default_install_path() -> PathBuf {
        // Place in build directory for packaging
        PathBuf::from("./compile-options.json")
    }
}

/// Export compile options as JSON
pub fn export_compile_options(
    output_path: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let schema = CompileOptionsSchema::generate();
    let path = output_path.unwrap_or_else(CompileOptionsSchema::get_default_install_path);

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    schema.export_to_file(&path)?;
    println!("✓ Compile options exported to: {}", path.display());
    Ok(())
}
