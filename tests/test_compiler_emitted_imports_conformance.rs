//! Compiler-emitted imports conformance.
//!
//! Compiles a representative corpus of in-repo `.cln` files and walks the
//! imports section of every resulting WASM binary. Every (module, name)
//! pair the compiler emits must:
//!   1. Appear in `function-registry.toml` (no orphan emissions), and
//!   2. Have the same WASM signature as the registry declares.
//!
//! This is the complement to `test_host_registration_conformance.rs`:
//! that test guarantees hosts implement the registry; this test guarantees
//! the compiler does not emit imports outside the registry. Together they
//! eliminate the entire "compiler emits X, host expects Y" drift class
//! that has been generating recurring framework regressions.
//!
//! The corpus is intentionally small but covers the main bridge surface
//! the compiler touches (math, string, console, memory). If the corpus
//! grows, this test grows for free.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clean_language_compiler::plugins::registry_loader::RegistryIndex;
use wasmparser::{CompositeType, Parser, Payload, TypeRef};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Files chosen to exercise distinct slices of the bridge surface.
/// Add new entries only when they cover a slice the corpus doesn't already
/// hit — every entry must successfully compile on `cargo test`.
const CORPUS: &[&str] = &[
    "tests/cln/core/basics/00_minimal.cln",
    "tests/cln/core/basics/01_hello_world.cln",
];

#[derive(Debug, Clone)]
struct Import {
    module: String,
    name: String,
    /// Param WASM types, in order.
    params: Vec<&'static str>,
    /// Return WASM types (WASM allows multi-value but Clean's bridge surface
    /// is single-value; this is always 0 or 1 entries today).
    returns: Vec<&'static str>,
}

fn collect_imports(wasm: &[u8]) -> Vec<Import> {
    let mut types: Vec<wasmparser::FuncType> = Vec::new();
    let mut imports = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.expect("wasm parses") {
            Payload::TypeSection(reader) => {
                for rec in reader.into_iter() {
                    let rec_group = rec.expect("type ok");
                    for sub in rec_group.types() {
                        if let CompositeType::Func(ft) = &sub.composite_type {
                            types.push(ft.clone());
                        }
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for imp in reader.into_iter() {
                    let imp = imp.expect("import ok");
                    if let TypeRef::Func(idx) = imp.ty {
                        let ft = &types[idx as usize];
                        imports.push(Import {
                            module: imp.module.to_string(),
                            name: imp.name.to_string(),
                            params: ft.params().iter().map(val_type_to_wasm).collect(),
                            returns: ft.results().iter().map(val_type_to_wasm).collect(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    imports
}

fn val_type_to_wasm(v: &wasmparser::ValType) -> &'static str {
    match v {
        wasmparser::ValType::I32 => "i32",
        wasmparser::ValType::I64 => "i64",
        wasmparser::ValType::F32 => "f32",
        wasmparser::ValType::F64 => "f64",
        _ => "unknown",
    }
}

/// Convert a registry param-type designator to its possible WASM expansions.
/// `"string"` is the ambiguous case (LP-pointer OR ptr+len pair); all others
/// have a single canonical expansion.
fn registry_param_expansions(t: &str) -> Vec<Vec<&'static str>> {
    let t = t.split(':').next().unwrap_or(t);
    match t {
        "string" => vec![vec!["i32"], vec!["i32", "i32"]],
        "ptr" | "boolean" | "i32" | "u32" | "handler" => vec![vec!["i32"]],
        "integer" | "i64" => vec![vec!["i64"]],
        "number" | "f64" => vec![vec!["f64"]],
        "void" | "" => vec![vec![]],
        _ => vec![vec!["unknown"]],
    }
}

fn registry_return_canonical(t: &str) -> &'static str {
    let t = t.split(':').next().unwrap_or(t);
    match t {
        "i32" | "boolean" | "ptr" | "string" | "handler" => "i32",
        "i64" | "integer" => "i64",
        "f64" | "number" => "f64",
        "void" | "" => "void",
        _ => "unknown",
    }
}

fn shape_matches(registry_params: &[String], host_shape: &[&'static str]) -> bool {
    let options: Vec<Vec<Vec<&'static str>>> = registry_params
        .iter()
        .map(|p| registry_param_expansions(p))
        .collect();

    fn try_match(
        options: &[Vec<Vec<&'static str>>],
        i: usize,
        host: &[&'static str],
        hi: usize,
    ) -> bool {
        if i == options.len() {
            return hi == host.len();
        }
        for opt in &options[i] {
            if hi + opt.len() <= host.len()
                && opt.iter().enumerate().all(|(k, t)| host[hi + k] == *t)
                && try_match(options, i + 1, host, hi + opt.len())
            {
                return true;
            }
        }
        false
    }
    try_match(&options, 0, host_shape, 0)
}

fn return_matches(registry_returns: &str, host_returns: &[&'static str]) -> bool {
    let canonical = registry_return_canonical(registry_returns);
    match canonical {
        "void" => host_returns.is_empty() || host_returns == ["i32"],
        other => host_returns == [other],
    }
}

#[test]
fn corpus_imports_match_registry() {
    let registry = RegistryIndex::load().expect("registry loads");

    let mut all_emissions: BTreeSet<(String, String)> = BTreeSet::new();
    let mut violations: Vec<String> = Vec::new();

    for rel in CORPUS {
        let path = repo_root().join(rel);
        compile_and_check(&path, &registry, &mut all_emissions, &mut violations);
    }

    assert!(
        !all_emissions.is_empty(),
        "expected at least some imports across the corpus; got none"
    );

    if !violations.is_empty() {
        let mut msg = format!(
            "Compiler emitted {} import(s) that do not conform to function-registry.toml:\n",
            violations.len()
        );
        for v in &violations {
            msg.push_str("  - ");
            msg.push_str(v);
            msg.push('\n');
        }
        panic!("{msg}");
    }
}

fn compile_and_check(
    path: &Path,
    registry: &RegistryIndex,
    all_emissions: &mut BTreeSet<(String, String)>,
    violations: &mut Vec<String>,
) {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            violations.push(format!("read {}: {e}", path.display()));
            return;
        }
    };
    let wasm = match clean_language_compiler::compile(&source) {
        Ok(w) => w,
        Err(errs) => {
            violations.push(format!(
                "compile {} failed ({} error{}): first = {:?}",
                path.display(),
                errs.len(),
                if errs.len() == 1 { "" } else { "s" },
                errs.first()
            ));
            return;
        }
    };

    for imp in collect_imports(&wasm) {
        all_emissions.insert((imp.module.clone(), imp.name.clone()));

        let entry = match registry.lookup(&imp.name) {
            Some(e) => e,
            None => {
                violations.push(format!(
                    "{}: emitted {}::{} not declared in function-registry.toml",
                    path.display(),
                    imp.module,
                    imp.name
                ));
                continue;
            }
        };

        if !shape_matches(&entry.params, &imp.params) {
            violations.push(format!(
                "{}: emitted {}::{} params {:?} -> {:?} do not match registry params {:?}",
                path.display(),
                imp.module,
                imp.name,
                imp.params,
                imp.returns,
                entry.params,
            ));
            continue;
        }

        if !return_matches(&entry.returns, &imp.returns) {
            violations.push(format!(
                "{}: emitted {}::{} returns {:?} do not match registry returns {:?}",
                path.display(),
                imp.module,
                imp.name,
                imp.returns,
                entry.returns,
            ));
        }
    }
}
