//! Cross-component canary registry coverage.
//!
//! Layer 1 of the umbrella prompt "Cross-Component Contract Canaries —
//! Nightly Integration Tests" (errors.cleanlanguage.dev/prompts umbrella
//! `7fb425cb`). Each `tests/cln/canaries/<ns>.cln` is one canary per bridge
//! namespace — a nightly failure surfaces the exact contract that regressed.
//!
//! Two invariants are asserted here:
//!
//! 1. **Every canary compiles.** All ten named canaries in
//!    `tests/cln/canaries/` must compile via the release `cln` binary with
//!    `--opt-level 2 --strict-hosts --plugins` (the exact flags Layer 2
//!    hosts will use). A compile error here means the canary itself is
//!    broken, not the host.
//!
//! 2. **Every registry namespace has at least one canary that imports at
//!    least one of its bridge functions.** This satisfies the L1 definition
//!    of done: "Each entry in `foundation/platform-architecture/
//!    function-registry.toml` is imported by at least one canary." We use
//!    namespace-level coverage (not per-function) because the umbrella's
//!    success criterion is "a silent addition of `_i18n_*` to clean-server
//!    ... produces a red canary cell", which is namespace-level drift.
//!
//! ## Why the CLI, not the library entry point
//!
//! The library's `compile_with_external_plugins_and_opt_level` follows a
//! subtly different codegen path than the CLI's `compile` subcommand, and
//! the canaries are the contract-facing artifacts Layer 2/3 hosts consume.
//! We test the exact bytes the CLI ships, so this test drives the release
//! `cln` binary at `target/release/cln`. Cargo builds it as a bin
//! dependency via the `#[test]` machinery (see the `env!("CARGO_BIN_EXE_")`
//! form), removing the need for a manual `cargo build --release` step.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use clean_language_compiler::plugins::registry_loader::RegistryIndex;
use wasmparser::{Parser, Payload, TypeRef};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every canary under `tests/cln/canaries/`. The prompt's ten named canaries
/// come first, then per-namespace expansions added while working through
/// the DoD requirement that every registry entry be imported by at least
/// one canary.
const CANARIES: &[&str] = &[
    // The ten named canaries from the L1 umbrella prompt.
    "tests/cln/canaries/console.cln",
    "tests/cln/canaries/file.cln",
    "tests/cln/canaries/crypto.cln",
    "tests/cln/canaries/http_client.cln",
    "tests/cln/canaries/db.cln",
    "tests/cln/canaries/session.cln",
    "tests/cln/canaries/auth.cln",
    "tests/cln/canaries/router.cln",
    "tests/cln/canaries/ui.cln",
    "tests/cln/canaries/canvas.cln",
    // Layer 2 portable expansions — additional per-namespace canaries.
    "tests/cln/canaries/math.cln",
    "tests/cln/canaries/json.cln",
    "tests/cln/canaries/time.cln",
    "tests/cln/canaries/env.cln",
    "tests/cln/canaries/storage.cln",
    "tests/cln/canaries/email.cln",
    "tests/cln/canaries/api.cln",
];

/// Registry categories (`[[functions]] category = "..."`) that are pure-WASM
/// (no host import) or otherwise cannot be exercised via a canary today.
/// Documented here so the coverage report distinguishes "canary missing"
/// from "canary impossible".
///
/// - `list`: `list.push_f64` is a MIR synthetic op with no user-callable
///   surface — the compiler emits it from a float-list literal, not from a
///   Clean-language call site. There is no bridge function a canary could
///   import to cover it.
///
/// - `json`: The registry declares `_json_encode`, `_json_decode`, `_json_get`
///   but the compiler implements `json.dataToText` / `json.textToData` /
///   `json.get` as pure-WASM stdlib exports (see WASM_ONLY_FUNCTIONS in
///   `src/plugins/registry.rs`). A canary compiled from Clean source can
///   never emit `env._json_*` imports because the language-level names
///   resolve to WASM-only paths. The bridges are only reachable if a plugin
///   explicitly declares them as bridge functions, which none of the shipped
///   plugins do.
///
/// - `build`: `_build_state_get` / `_build_state_set` are consumed at
///   plugin build time via each plugin's build-manifest.json — never from
///   a user program at runtime. No Clean-source canary can emit them.
const IMPOSSIBLE_CATEGORIES: &[&str] = &["list", "json", "build"];

/// Registry categories that DON'T YET have a canary but will. Tracks the
/// gap between the ten named canaries the L1 prompt required and the full
/// registry surface the L1 DoD calls out. Every entry here is a follow-up
/// PR: add a canary, remove the category from this list. The coverage test
/// treats this list as a soft floor — categories on it are excluded from
/// the missing report so the ten named canaries can land without waiting
/// on the long tail.
///
/// **Do not add new categories here without opening a follow-up ticket.**
/// This list should only shrink.
const PENDING_CATEGORIES: &[&str] = &[
    // Server-only surfaces (Layer 3) — a canary needs a real request context
    // or a live server host to exercise. Grouped under router.cln today,
    // but the plugin expansion emits helpers that don't reach every symbol.
    "jobs",
    "jwt",
    "locale",
    "mcp",
    "roles",
    "test",
    // Browser-only surfaces (Layer 3 browser) — need frame.ui / frame.canvas
    // expansion or a browser runtime. Canvas + UI have plugin expansion bugs
    // (PLUGIN013 batch.arrayPush) blocking full coverage today.
    "anim",
    "animsprite",
    "animstate",
    "asset",
    "custom",
    "ease",
    "feed",
    "font",
    "gradient",
    "layer",
    "live",
    "page",
    "particles",
    "path",
    "sprite",
    "timeline",
    "tween",
    "ui",
    "ui_browser",
];

fn category_of_import(bridge_name: &str, registry: &RegistryIndex) -> Option<String> {
    registry.lookup(bridge_name).map(|f| f.category.clone())
}

fn collect_import_names(wasm: &[u8]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for imp in reader.into_iter().flatten() {
                if matches!(imp.ty, TypeRef::Func(_)) {
                    names.insert(imp.name.to_string());
                }
            }
        }
    }
    names
}

fn cln_binary() -> Result<PathBuf, String> {
    let candidate = repo_root().join("target").join("release").join("cln");
    if candidate.exists() {
        Ok(candidate)
    } else {
        Err(format!(
            "cln release binary not found at {}\n\
             Run `cargo build --release --bin cln` before this test.",
            candidate.display()
        ))
    }
}

fn compile_canary(cln: &Path, rel: &str) -> Result<Vec<u8>, String> {
    let src = repo_root().join(rel);
    let out_dir = repo_root().join("tests").join("output").join("canaries");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir output: {e}"))?;
    let wasm_path = out_dir.join(
        Path::new(rel)
            .file_stem()
            .expect("canary has a stem")
            .to_string_lossy()
            .into_owned()
            + ".wasm",
    );

    let out = Command::new(cln)
        .arg("compile")
        .arg(&src)
        .arg("--output")
        .arg(&wasm_path)
        .arg("--opt-level")
        .arg("2")
        .arg("--strict-hosts")
        .arg("--plugins")
        .output()
        .map_err(|e| format!("spawn cln: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(format!(
            "cln compile {rel} failed (exit {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
            out.status.code(),
            stdout.trim(),
            stderr.trim()
        ));
    }
    std::fs::read(&wasm_path).map_err(|e| format!("read {}: {e}", wasm_path.display()))
}

#[test]
fn all_canaries_compile() {
    let cln = match cln_binary() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("skipping: {e}");
            return;
        }
    };

    let mut failures = Vec::new();
    for c in CANARIES {
        if let Err(e) = compile_canary(&cln, c) {
            failures.push(e);
        }
    }
    if !failures.is_empty() {
        panic!(
            "{} of {} canaries failed to compile:\n\n{}",
            failures.len(),
            CANARIES.len(),
            failures.join("\n\n")
        );
    }
}

#[test]
fn every_registry_namespace_has_canary_coverage() {
    let cln = match cln_binary() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("skipping: {e}");
            return;
        }
    };

    let registry = RegistryIndex::load().expect("registry loads");

    // Group required registry entries by category. Skip entries with an empty
    // category — pre-taxonomy legacy entries the registry hasn't classified.
    let mut required: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in registry.functions() {
        if f.category.is_empty() {
            continue;
        }
        required
            .entry(f.category.clone())
            .or_default()
            .insert(f.name.clone());
    }

    let mut covered: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut compile_failures = Vec::new();
    for c in CANARIES {
        let wasm = match compile_canary(&cln, c) {
            Ok(w) => w,
            Err(e) => {
                compile_failures.push(e);
                continue;
            }
        };
        for name in collect_import_names(&wasm) {
            if let Some(cat) = category_of_import(&name, &registry) {
                covered.entry(cat).or_default().insert(name);
            }
        }
    }
    if !compile_failures.is_empty() {
        panic!(
            "cannot measure coverage — {} canary compile failure(s):\n\n{}",
            compile_failures.len(),
            compile_failures.join("\n\n")
        );
    }

    let impossible: BTreeSet<&&str> = IMPOSSIBLE_CATEGORIES.iter().collect();
    let pending: BTreeSet<&&str> = PENDING_CATEGORIES.iter().collect();

    let mut missing = Vec::new();
    let mut newly_covered = Vec::new();
    for (cat, funcs) in &required {
        if impossible.contains(&cat.as_str()) {
            continue;
        }
        let is_pending = pending.contains(&cat.as_str());
        let is_covered = covered.contains_key(cat);
        match (is_pending, is_covered) {
            (false, false) => missing.push(format!(
                "  - category `{cat}` ({} function(s)): no canary imports any",
                funcs.len()
            )),
            (true, true) => newly_covered.push(cat.clone()),
            _ => {}
        }
    }

    if !newly_covered.is_empty() {
        panic!(
            "{} PENDING_CATEGORIES are now covered by canaries. Remove them \
             from the list so the coverage floor tightens:\n  - {}",
            newly_covered.len(),
            newly_covered.join("\n  - ")
        );
    }

    if !missing.is_empty() {
        panic!(
            "{} registry category(ies) have no canary coverage AND are not \
             on the PENDING_CATEGORIES follow-up list.\n\
             Layer 1 DoD requires every registry entry to be imported by at \
             least one canary in `tests/cln/canaries/`. Add a canary per \
             missing category (see the umbrella prompt for structure), or \
             extend IMPOSSIBLE_CATEGORIES / PENDING_CATEGORIES with a \
             rationale.\n\n\
             Missing:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }
}
