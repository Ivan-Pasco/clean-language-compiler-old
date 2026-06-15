//! Architectural boundary tests — prevent plugin-domain knowledge from
//! re-entering the compiler core.
//!
//! Background: in 0.30.279–0.30.283 the compiler accumulated ~1000 lines
//! of HTML/component-registry/route-derivation logic that should have
//! lived in plugin WASM (frame.ui). The dead pile was removed in 0.30.284
//! (ARCH-PLUGIN-LOGIC-IN-COMPILER). These tests pin the boundary so the
//! pile cannot reappear by accident.
//!
//! Two layers of enforcement:
//!
//! 1. `no_dead_compiler_side_html_pile` — name-based regression guard.
//!    Specific function names that were removed must never come back.
//!    Renaming would silently bypass — but renaming dead code we just
//!    deleted is implausible without intent.
//!
//! 2. `no_new_plugin_bridge_knowledge_outside_abstraction_boundary` —
//!    forward guard. Plugin bridge prefixes (`_req_`, `_res_`, `_db_`,
//!    `_auth_`, `_session_`, `_jwt_`, `_ui_`, `_http_route`) must not be
//!    hardcoded as string literals outside an explicit list of files
//!    (`EXEMPT_FILES`). That list IS the architectural boundary — adding
//!    a file to it requires a tracking report.

use std::path::PathBuf;
use walkdir::WalkDir;

/// Compiler-side function names that were the dead HTML pile removed in
/// 0.30.284. They must never reappear in any form in compiler source.
const REMOVED_DEAD_PILE_NAMES: &[&str] = &[
    "process_html_pages_to_clean",
    "extract_html_metadata",
    "html_to_render_body",
    "build_component_registry",
    "file_path_to_route",
    "class_name_to_tag",
    "html_block_to_code_rust",
    "strip_common_indent",
];

/// String prefixes for plugin bridge functions. Hardcoding these as
/// string literals (`"_req_*"`, `"_auth_*"`) in compiler code is the
/// signature of plugin-domain knowledge leaking into the compiler.
/// The plugin's `plugin.toml` `[bridge]` section is the source of truth.
const PLUGIN_BRIDGE_PREFIXES: &[&str] = &[
    "_req_",
    "_res_",
    "_db_",
    "_auth_",
    "_session_",
    "_jwt_",
    "_ui_",
    "_http_route",
];

/// Files where plugin-bridge string literals are allowed. This list IS
/// the documented architectural boundary. Adding a file to it requires
/// either:
///   (a) the file legitimately owns the abstraction (e.g. plugin loader,
///       reachability gating), OR
///   (b) it's an existing known violation tracked as a separate bug.
///
/// New compiler features must NOT add files here without justification.
const EXEMPT_FILES: &[&str] = &[
    // Plugin abstraction layer — owns the contract:
    "src/plugins/",
    // Binaries: debug tools and test runners may register host stubs:
    "src/bin/",
    // KNOWN VIOLATION (tracked under BUILTIN-NAMESPACE-OVERREACH):
    // MCP introspection lists plugin bridges directly instead of reading
    // from plugin.toml. Refactor would consolidate registry sources.
    "src/mcp/server.rs",
    // KNOWN VIOLATION (tracked under BUILTIN-NAMESPACE-OVERREACH):
    // wasmtime test-runner stubs hardcode bridge names so .wasm fixtures
    // can execute without a full host. Should be generated from plugin.toml.
    "src/runtime/host_functions.rs",
    // KNOWN VIOLATION (to be filed as separate report):
    // `is_reachability_gated_import` hardcodes server-only bridge prefixes.
    // Correct: consult plugin.toml `hosts = [...]` per bridge function.
    "src/codegen/mod.rs",
    // Doc-comment example only — no executable plugin knowledge:
    "src/hir/mod.rs",
    // KNOWN VIOLATION (BUILTIN-NAMESPACE-OVERREACH, sub-finding A):
    // Resolver pre-registers `req.*`, `_req_*`, `auth.*`, `session.*` as
    // hardcoded builtin function signatures instead of consuming plugin.toml
    // `[bridge]` declarations. Fixing requires the resolver to receive a
    // plugin registry handle during symbol-table seeding.
    "src/resolver/symbol_table.rs",
    // Note: src/codegen/mir_codegen/utilities.rs (formerly sub-finding B) was
    // cleared in 0.30.289 — `register_http_server_wrappers` deleted (0.30.288)
    // and the Layer-3 prefix classifier replaced with a bridge_functions lookup.
    // KNOWN VIOLATION (BUILTIN-NAMESPACE-OVERREACH, sub-finding C):
    // Codegen module builder hardcodes `db.begin/commit/rollback` →
    // `_db_begin/_db_commit/_db_rollback` aliasing instead of reading
    // language→bridge mappings from plugin.toml. Same refactor.
    "src/codegen/codegen_module_builder.rs",
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn is_exempt(rel_path: &str) -> bool {
    EXEMPT_FILES
        .iter()
        .any(|prefix| rel_path.starts_with(prefix))
}

#[test]
fn no_dead_compiler_side_html_pile() {
    let src = manifest_dir().join("src");
    let mut violations = Vec::new();

    for entry in WalkDir::new(&src).into_iter().filter_map(Result::ok) {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = entry
            .path()
            .strip_prefix(manifest_dir())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");

        for name in REMOVED_DEAD_PILE_NAMES {
            if content.contains(name) {
                violations.push(format!("{rel}: reintroduced dead-pile name `{name}`"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Architectural regression: dead compiler-side HTML/plugin logic has reappeared.\n\
         These names were removed in 0.30.284 (ARCH-PLUGIN-LOGIC-IN-COMPILER) and must\n\
         not return in any form. Plugin lifecycle hooks belong in plugin WASM.\n\n\
         Violations:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn no_new_plugin_bridge_knowledge_outside_abstraction_boundary() {
    let manifest = manifest_dir();
    let src = manifest.join("src");
    let mut violations = Vec::new();

    for entry in WalkDir::new(&src).into_iter().filter_map(Result::ok) {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(&manifest)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if is_exempt(&rel) {
            continue;
        }

        let content = match std::fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Skip everything after the first `#[cfg(test)]` — Rust convention
        // places test modules at the bottom of the file, and test code is
        // allowed to reference bridge names in assertions.
        let scan_end = content.find("#[cfg(test)]").unwrap_or(content.len());
        let scannable = &content[..scan_end];

        for (idx, line) in scannable.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
                continue;
            }
            for prefix in PLUGIN_BRIDGE_PREFIXES {
                let quoted = format!("\"{prefix}");
                if line.contains(&quoted) {
                    violations.push(format!(
                        "{rel}:{} — hardcodes plugin bridge `\"{prefix}*\"`\n        line: {}",
                        idx + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Architectural boundary violation: compiler code outside the abstraction\n\
         boundary hardcodes plugin bridge function names.\n\n\
         Plugin bridge functions are declared in plugin.toml `[bridge]`. The\n\
         compiler must consume those declarations, not hardcode names. See\n\
         `.claude/rules/compiler-work.md` § \"No Plugin Logic in the Compiler\".\n\n\
         If this is legitimate (e.g. the file owns the plugin abstraction),\n\
         add the file to `EXEMPT_FILES` in this test with a comment stating WHY,\n\
         and file an architectural report if it represents new plugin-domain\n\
         knowledge.\n\n\
         Violations:\n  {}",
        violations.join("\n\n  ")
    );
}
