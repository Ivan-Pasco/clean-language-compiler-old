//! Shared fixture for the plugin system CLI tests: installs small v1
//! (string-emission) plugins into a throwaway `$HOME/.cleen/plugins`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// Length-prefixed (4 byte little endian) string as a WAT data literal.
pub fn lp_data(text: &str) -> String {
    let mut out = String::new();
    for b in (text.len() as u32).to_le_bytes() {
        out.push_str(&format!("\\{:02x}", b));
    }
    for b in text.bytes() {
        out.push_str(&format!("\\{:02x}", b));
    }
    out
}

/// A v1 plugin whose `expand_block` always returns `output`.
pub fn plugin_wat(output: &str) -> String {
    format!(
        r#"(module
  (memory (export "memory") 1)
  (data (i32.const 2048) "{}")
  (func (export "expand_block") (param i32 i32 i32) (result i32) (i32.const 2048)))"#,
        lp_data(output)
    )
}

/// Install plugin `name` under `home`. `blocks` are the block names it
/// handles; `bridge` is the raw TOML body of its `functions = [...]` array.
pub fn install_plugin(home: &Path, name: &str, blocks: &[&str], output: &str, bridge: &str) {
    let dir = home.join(".cleen/plugins").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.wasm"),
        wat::parse_str(plugin_wat(output)).unwrap(),
    )
    .unwrap();
    let block_list = blocks
        .iter()
        .map(|b| format!("\"{}\"", b))
        .collect::<Vec<_>>()
        .join(", ");
    let bridge_section = if bridge.is_empty() {
        String::new()
    } else {
        format!("\n[bridge]\nfunctions = [\n{}\n]\n", bridge)
    };
    std::fs::write(
        dir.join("plugin.toml"),
        format!(
            "[plugin]\nname = \"{name}\"\nversion = \"1.0.0\"\ndescription = \"test\"\nauthor = \"test\"\n\n\
             [handles]\nblocks = [{block_list}]\n\n[exports]\nexpand = \"expand_block\"\n\n\
             [lifecycle]\nmodule_helpers_are_roots = true\n\n\
             [build]\nbuilt_with_compiler = \"{ver}\"\n{bridge_section}",
            ver = env!("CARGO_PKG_VERSION"),
        ),
    )
    .unwrap();
}

pub struct Project {
    pub home: TempDir,
    pub work: TempDir,
}

impl Project {
    pub fn new() -> Self {
        Project {
            home: TempDir::new().unwrap(),
            work: TempDir::new().unwrap(),
        }
    }

    pub fn write(&self, file: &str, source: &str) -> PathBuf {
        let path = self.work.path().join(file);
        std::fs::write(&path, source).unwrap();
        path
    }

    pub fn compile(&self, file: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cln"))
            .args(["compile", file, "-o", "out.wasm"])
            .current_dir(self.work.path())
            .env("HOME", self.home.path())
            // The throwaway bridge names are not in the shipped registry.
            .env("CLEAN_PLUGIN_REGISTRY_VALIDATION", "off")
            .output()
            .expect("cln runs")
    }

    pub fn run(&self) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_wasmtime_runner"))
            .arg(self.work.path().join("out.wasm"))
            .output()
            .expect("wasmtime_runner runs");
        assert!(out.status.success(), "{}", text(&out));
        // The runner frames the program output between marker lines.
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        stdout
            .split("--- Output ---\n")
            .nth(1)
            .and_then(|rest| rest.split("--- End Output ---").next())
            .unwrap_or("")
            .to_string()
    }
}

pub fn text(out: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
}
