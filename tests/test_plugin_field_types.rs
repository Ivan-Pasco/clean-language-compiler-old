//! A plugin manifest declares the field types of its `types` with the same
//! grammar as a type annotation in source: `string?` and
//! `list<integer>.line.unique` are valid (issue #70).

use std::process::Command;

const MANIFEST: &str = r#"[plugin]
name = "probe.types"
version = "1.0.0"
description = "types probe"

[handles]
blocks = []

[language]
types = [ { name = "Probe", description = "x", fields = [
  {name="nick", type="string?", description="optional"},
  {name="tags", type="list<integer>.line.unique", description="tags"},
] } ]

[lifecycle]
module_helpers_are_roots = true
"#;

/// The smallest valid WebAssembly module: magic and version, no sections.
const EMPTY_WASM: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

#[test]
fn nullable_and_list_behavior_field_types_are_accepted() {
    let dir = std::env::temp_dir().join(format!("cln_plugin_types_{}", std::process::id()));
    let plugin = dir.join("probe.types");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("plugin.toml"), MANIFEST).unwrap();
    std::fs::write(plugin.join("plugin.wasm"), EMPTY_WASM).unwrap();
    let main = dir.join("main.cln");
    std::fs::write(
        &main,
        "plugins:\n\tprobe.types\n\nstart:\n\tprint(\"ok\")\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_cln"))
        .arg("check")
        .arg("--plugin-override")
        .arg(format!("probe.types={}", plugin.display()))
        .arg(&main)
        .output()
        .expect("cln runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        !text.contains("is not a valid type"),
        "field types must be accepted: {text}"
    );
    assert!(output.status.success(), "{text}");
}
