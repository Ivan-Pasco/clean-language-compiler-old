//! Plugin generic shorthand (plugin-contract.md §4.5, type-system.md §10):
//! a plugin type that declares `instance_class = "PagedResult_{T}"` makes
//! `PagedResult<Item>` name the concrete class `PagedResult_Item` the plugin
//! emits. Before the rule, `Name<Arg>` lowered to an inferred type and every
//! field access on it failed with "Type Unknown does not have field".
//!
//! Needs frame.data in ~/.cleen/plugins (it declares the template and emits
//! the per-model classes); skips when it is not installed.

use std::path::PathBuf;

fn frame_data_installed() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(&home)
        .join(".cleen/plugins/frame.data/plugin.wasm")
        .exists()
}

fn compile(source: &str) -> Result<(), String> {
    let tmp = tempfile::TempDir::new().map_err(|e| e.to_string())?;
    let src = tmp.path().join("main.cln");
    std::fs::write(&src, source).map_err(|e| e.to_string())?;
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_cln"))
        .args([
            "compile",
            src.to_str().unwrap(),
            "-o",
            tmp.path().join("out.wasm").to_str().unwrap(),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

const MODEL: &str = "plugins:\n\tframe.data\n\ndata Item:\n\tfields:\n\t\tinteger id primary generated\n\t\tstring name required\n\n";

#[test]
fn shorthand_names_the_plugin_emitted_class() {
    if !frame_data_installed() {
        eprintln!("skipping: frame.data not installed");
        return;
    }
    let source = format!(
        "{MODEL}start:\n\tPagedResult<Item> page = PagedResult_Item()\n\tpage.totalCount = 3\n\tPagedResult_Item same = page\n\tCursorResult<Item> cur = CursorResult_Item()\n\tcur.nextCursor = \"7\"\n\tprintl(same.totalCount.toString() + cur.nextCursor)\n"
    );
    compile(&source).unwrap_or_else(|e| panic!("shorthand did not resolve:\n{e}"));
}

#[test]
fn shorthand_without_emitted_class_is_undefined() {
    if !frame_data_installed() {
        eprintln!("skipping: frame.data not installed");
        return;
    }
    // No `data Missing:` model, so frame.data emits no PagedResult_Missing.
    let source = format!("{MODEL}start:\n\tPagedResult<Missing> page = PagedResult_Item()\n\tprintl(page.totalCount.toString())\n");
    let err = compile(&source).expect_err("an unknown instance class must not compile");
    assert!(
        err.contains("PagedResult_Missing"),
        "error should name the concrete class:\n{err}"
    );
}
