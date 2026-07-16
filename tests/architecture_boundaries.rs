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
    // KNOWN VIOLATION (tracked as ARCH-CONC002-HARDCODES-HTTP-ROUTE,
    // local report b675e266-0d58-4acb-a685-d67459f9fa41 pending dashboard sync):
    //
    // WHY the file needs the knowledge: CONC002 must decide, per function,
    // whether it is a legitimate request-context site so that `req.*`,
    // `session.*`, and `res.*` calls inside it are allowed. Plugins register
    // handlers by emitting `_http_route(method, path, handler)` or
    // `_http_route_protected(method, path, handler, role)` into `start:`, so
    // any function whose name appears as the third argument of one of those
    // calls is a request-context site. Without recognising this,
    // plugin-emitted handlers (which take zero parameters and have
    // plugin-chosen names like `__debug_capture_handler`) triggered spurious
    // CONC002 for every `req.param` etc. call inside them. The pre-pass in
    // `HirValidator::collect_plugin_registered_handlers` extracts those
    // handler names — which requires matching the two bridge names.
    //
    // RIGHT LONG-TERM FIX: teach plugin.toml `[[bridge]]` to declare
    // `registers_handler_at_arg = <int>` (or add a `[[route_registration]]`
    // section). Expose the aggregated set from the plugin registry via a
    // `handler_registering_bridges()` query. `scan_expr_for_route_registrations`
    // then becomes generic over any bridge that opts in — WebSocket, gRPC,
    // MQTT subscribe bridges, etc. would work with zero compiler changes.
    // Once that lands, drop this exemption entry.
    "src/hir/validation.rs",
    // KNOWN VIOLATION (BUILTIN-NAMESPACE-OVERREACH, sub-finding A — last residue):
    // Resolver pre-registers raw bridge names (`_req_*`, `_server_sleep`) as
    // hardcoded builtin function signatures so direct bridge calls from
    // framework-generated code resolve at parse time. Removing these requires
    // either rewriting how the framework emits direct bridge calls or accepting
    // the architectural impurity — both deserve their own decision.
    //
    // Also still hardcoded: `crypto.sha256` / `crypto.sha512` — these resolve to
    // bridge names that don't exist (`_crypto_sha256` / `_crypto_sha512`) while
    // function-registry.toml declares them as `_crypto_hash_sha256` /
    // `_crypto_hash_sha512` with `aliases = ["crypto.hash_sha256", ...]`. Either
    // rename the registry aliases or update these entries — separate task.
    //
    // Sub-A language-level entries (req.*, auth.*, session.*, server.sleep,
    // jwt.{sign,verify,decode}, db.{query,execute}, crypto.{hashPassword,
    // verifyPassword,randomBytes,randomHex,hmac}, env.get, time.now, now,
    // http.{setCache,noCache}) all removed by 0.30.301 — supplied by frame.*
    // plugin.toml entries with `maps_to` + `params` + `returns`.
    "src/resolver/symbol_table.rs",
    // Note: src/codegen/mir_codegen/utilities.rs (formerly sub-finding B) was
    // cleared in 0.30.289 — `register_http_server_wrappers` deleted (0.30.288)
    // and the Layer-3 prefix classifier replaced with a bridge_functions lookup.
    // KNOWN VIOLATION (BUILTIN-NAMESPACE-OVERREACH, sub-finding C — partial):
    // Codegen module builder still wraps `db.query` and `db.execute` because
    // both need the Clean-string → (ptr+4, len) expand path. The transaction
    // trio (`db.begin/commit/rollback`) was cleared in 0.30.296 — now resolved
    // via plugin manifest `language_to_bridge_map`. Also still hardcodes the
    // `http.*` wrapper family and `_http_route`/`_req_*` direct registrations.
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
