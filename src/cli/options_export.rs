//! `cln options --export-json`: describe the `cln compile` options for IDE
//! integration. Everything is read from the clap definition of the `compile`
//! subcommand, so the export can only list options that really exist.

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
    /// Values of `--target`.
    pub targets: Vec<CompileOption>,
    /// Values of `--opt-level`.
    pub optimizations: Vec<CompileOption>,
    /// The compiler selects no runtime; kept empty so the schema shape is
    /// stable for existing readers.
    pub runtimes: Vec<CompileOption>,
    /// Boolean switches of `cln compile` (plus the global ones).
    pub flags: Vec<CompileOption>,
    pub presets: Vec<CompilePreset>,
}

impl CompileOptionsSchema {
    /// Build the schema from the `cln` clap command (`Args::command()`).
    pub fn generate(cln: &clap::Command) -> Result<Self, String> {
        let compile = cln
            .find_subcommand("compile")
            .ok_or("the cln command has no `compile` subcommand")?;
        let arg = |id: &str| {
            compile
                .get_arguments()
                .find(|a| a.get_id() == id)
                .ok_or(format!("`cln compile` has no `{id}` argument"))
        };

        let target = arg("target")?;
        let targets = value_options(target);

        let opt_level = arg("opt_level")?;
        let default_level = default_value(opt_level);
        let optimizations = (0..=3u8)
            .map(|level| CompileOption {
                id: level.to_string(),
                label: format!("-O{level}"),
                description: match level {
                    0 => "No optimization (fastest compilation)",
                    1 => "Light optimization",
                    2 => "Standard optimization",
                    _ => "Aggressive optimization (speed + size)",
                }
                .to_string(),
                flag: long_flag(opt_level),
                default: default_level.as_deref() == Some(level.to_string().as_str()),
                available: true,
                mutually_exclusive: None,
            })
            .collect();

        let mut flags: Vec<CompileOption> = compile
            .get_arguments()
            .chain(cln.get_arguments())
            .filter(|a| !a.is_hide_set() && !a.get_action().takes_values())
            .filter(|a| !matches!(a.get_id().as_str(), "help" | "version"))
            .map(|a| CompileOption {
                id: a.get_id().to_string(),
                label: long_flag(a).unwrap_or_else(|| a.get_id().to_string()),
                description: a.get_help().map(|h| h.to_string()).unwrap_or_default(),
                flag: long_flag(a),
                default: false,
                available: true,
                mutually_exclusive: None,
            })
            .collect();
        flags.dedup_by(|a, b| a.id == b.id);

        let presets = vec![
            CompilePreset {
                id: "standard".to_string(),
                label: "Standard compilation".to_string(),
                description: "No additional options".to_string(),
                flags: vec![],
            },
            CompilePreset {
                id: "release".to_string(),
                label: "Release".to_string(),
                description: "Strip always: invariant checks".to_string(),
                flags: vec!["release".to_string()],
            },
        ];
        for preset in &presets {
            for flag in &preset.flags {
                if !flags.iter().any(|f| &f.id == flag) {
                    return Err(format!("preset `{}` uses unknown flag `{flag}`", preset.id));
                }
            }
        }

        Ok(Self {
            version: "1.0.0".to_string(),
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: Utc::now().to_rfc3339(),
            targets,
            optimizations,
            runtimes: Vec::new(),
            flags,
            presets,
        })
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

fn long_flag(arg: &clap::Arg) -> Option<String> {
    arg.get_long().map(|l| format!("--{l}"))
}

fn default_value(arg: &clap::Arg) -> Option<String> {
    arg.get_default_values()
        .first()
        .map(|v| v.to_string_lossy().into_owned())
}

/// One option per possible value of `arg`.
fn value_options(arg: &clap::Arg) -> Vec<CompileOption> {
    let default = default_value(arg);
    arg.get_possible_values()
        .iter()
        .map(|v| CompileOption {
            id: v.get_name().to_string(),
            label: v.get_name().to_string(),
            description: v.get_help().map(|h| h.to_string()).unwrap_or_default(),
            flag: long_flag(arg),
            default: default.as_deref() == Some(v.get_name()),
            available: true,
            mutually_exclusive: None,
        })
        .collect()
}

/// Export compile options as JSON
pub fn export_compile_options(
    cln: &clap::Command,
    output_path: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let schema = CompileOptionsSchema::generate(cln)?;
    let path = output_path.unwrap_or_else(CompileOptionsSchema::get_default_install_path);

    // Ensure parent directory exists
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    schema.export_to_file(&path)?;
    println!("✓ Compile options exported to: {}", path.display());
    Ok(())
}
