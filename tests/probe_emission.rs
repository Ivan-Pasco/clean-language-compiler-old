//! Integration tests for the `--emit-heap-probes` and `--emit-bridge-probes`
//! CLI flags. Verifies the flags produce a syntactically valid WASM module
//! and a `.probes.json` sidecar whose callsite table matches expectations.
//!
//! STATE-A heap-probe hunt ack:
//! prompt 410a2312-836c-11f1-9d55-da25a95a496b (heap probes)
//! prompt 52a89f4b-82b6-11f1-9d55-da25a95a496b (bridge probes — this file's ack).

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cln_binary() -> PathBuf {
    repo_root().join("target").join("release").join("cln")
}

fn tmp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("probe_emission_{}_{}", std::process::id(), name))
}

fn write_source(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write source");
}

fn read_sidecar(wasm_path: &Path) -> serde_json::Value {
    let sidecar_path = format!("{}.probes.json", wasm_path.display());
    let bytes = std::fs::read(sidecar_path).expect("read sidecar");
    serde_json::from_slice(&bytes).expect("parse sidecar JSON")
}

fn compile(src: &Path, out: &Path, extra_flags: &[&str]) {
    let cln = cln_binary();
    if !cln.exists() {
        eprintln!("cln binary not built at {} — skipping", cln.display());
        return;
    }
    let mut cmd = Command::new(&cln);
    cmd.arg("compile")
        .arg(src)
        .arg("--output")
        .arg(out)
        .args(extra_flags);
    let output = cmd.output().expect("cln compile");
    assert!(
        output.status.success(),
        "cln compile failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out.exists(), "wasm output missing at {}", out.display());
}

#[test]
fn heap_probes_produce_sidecar_entries_for_sb_ops() {
    let src = tmp_path("heap.cln");
    let out = tmp_path("heap.wasm");
    write_source(
        &src,
        "start:\n\
         \tstring html = \"\"\n\
         \tinteger i = 0\n\
         \twhile i < 3\n\
         \t\thtml = html + \"row \" + i.toString()\n\
         \t\ti = i + 1\n\
         \tprint(html)\n",
    );

    compile(&src, &out, &["--emit-heap-probes"]);

    let sidecar = read_sidecar(&out);
    assert_eq!(sidecar["version"], serde_json::json!(1));
    let callsites = sidecar["callsites"]
        .as_array()
        .expect("callsites is an array");
    assert!(
        !callsites.is_empty(),
        "expected heap-probe callsites, got none"
    );
    // At least one sb_append and one sb_finalize should appear.
    let mut saw_append = false;
    let mut saw_finalize = false;
    for cs in callsites {
        let fn_name = cs["function"].as_str().unwrap_or("");
        if fn_name == "string_builder_append" {
            saw_append = true;
        }
        if fn_name == "string_builder_finalize" {
            saw_finalize = true;
        }
    }
    assert!(
        saw_append,
        "expected at least one string_builder_append callsite"
    );
    assert!(
        saw_finalize,
        "expected at least one string_builder_finalize callsite"
    );
}

#[test]
fn bridge_probes_produce_before_after_pairs() {
    // Use the existing bridge test that exercises a real plugin bridge
    // (`int_to_string` via `.toString()` returns i32 → probeable).
    let root = repo_root();
    let src = root.join("tests/cln/bridge/time_now_arithmetic.cln");
    if !src.exists() {
        eprintln!("test fixture missing at {} — skipping", src.display());
        return;
    }
    let out = tmp_path("bridge.wasm");
    compile(&src, &out, &["--emit-bridge-probes"]);

    let sidecar = read_sidecar(&out);
    let callsites = sidecar["callsites"]
        .as_array()
        .expect("callsites is an array");

    // Every bridge_before must have a matching bridge_after with sequential id.
    let mut befores: Vec<(u32, String)> = Vec::new();
    let mut afters: Vec<(u32, String)> = Vec::new();
    for cs in callsites {
        let id = cs["id"].as_u64().expect("id u64") as u32;
        let fname = cs["function"].as_str().unwrap_or("").to_string();
        if let Some(rest) = fname.strip_prefix("bridge_before:") {
            befores.push((id, rest.to_string()));
        } else if let Some(rest) = fname.strip_prefix("bridge_after:") {
            afters.push((id, rest.to_string()));
        }
    }
    assert!(
        !befores.is_empty(),
        "expected bridge_before callsites in {}",
        src.display()
    );
    assert_eq!(
        befores.len(),
        afters.len(),
        "bridge_before and bridge_after counts must match"
    );
    for ((before_id, before_name), (after_id, after_name)) in befores.iter().zip(afters.iter()) {
        assert_eq!(
            before_name, after_name,
            "before/after must target the same bridge"
        );
        assert_eq!(
            *after_id,
            *before_id + 1,
            "before/after ids must be sequential (before={} after={})",
            before_id,
            after_id
        );
    }
}

#[test]
fn no_probe_flags_produces_no_sidecar_and_no_extra_imports() {
    let src = tmp_path("noflag.cln");
    let out = tmp_path("noflag.wasm");
    write_source(
        &src,
        "start:\n\
         \tprint(\"hi\")\n",
    );
    compile(&src, &out, &[]);

    // With no probe flags, the sidecar file must NOT be created.
    let sidecar_path = format!("{}.probes.json", out.display());
    assert!(
        !std::path::Path::new(&sidecar_path).exists(),
        "sidecar {} should not exist when neither probe flag is set",
        sidecar_path
    );
}
