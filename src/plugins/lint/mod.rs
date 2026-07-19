/// Plugin Contract 5 — Lint Extension host-side subsystem.
///
/// Implements the host side of the lint ABI described in
/// `foundation/spec/framework/contracts/lint-extension.md`.
///
/// Phase B scope (compiler-side, this module):
///   - `arena`    — single-call `LintArena` holding a snapshot of the
///                  pre-expansion `Program`, the monotonic `handle`, and the
///                  4 accessor methods that produce the JSON payloads
///                  described in §4.
///   - `bridges`  — 4 WASM host functions (`_ast_list_classes`,
///                  `_ast_class_fields`, `_ast_list_functions`,
///                  `_ast_list_blocks`) registered onto a dedicated lint
///                  linker in `wasm_adapter.rs`.
///   - `pass`     — `run_lint_pass` orchestrates one lint pass over all
///                  plugins in the registry that declare `[exports].lint`.
///
/// Cycle 3 will add diagnostic JSON parsing, cross-plugin code uniqueness
/// enforcement (LINT003), and routing through the compiler renderer.
pub(crate) mod arena;
pub(crate) mod bridges;

pub(crate) use arena::LintArena;
pub(crate) use bridges::register_lint_bridges;

use crate::ast::Program;
use crate::plugins::PluginRegistry;

/// One plugin's contribution to a lint pass.
///
/// Cycle 2 keeps this as raw JSON so the CLI can print it verbatim while
/// Cycle 3 nails down the diagnostic parser. Cycle 3 will replace the
/// `raw_json` variant with a `Diagnostics(Vec<LintDiagnostic>)` variant
/// carrying structured entries.
#[derive(Debug, Clone)]
pub enum PluginLintReport {
    /// Plugin declared `[exports].lint = "..."` and its export ran
    /// successfully. `raw_json` is the string the plugin wrote to memory:
    /// per spec §3, either a JSON array of diagnostic entries or a JSON
    /// error object.
    Ok {
        plugin_name: String,
        raw_json: String,
    },
    /// Plugin trapped, timed out, or returned a malformed LP pointer.
    /// The compiler emits a `LINT001` diagnostic naming the plugin and
    /// continues with other plugins — the build is never blocked.
    Failed {
        plugin_name: String,
        message: String,
    },
}

impl PluginLintReport {
    /// The plugin name attached to this report (regardless of Ok/Failed).
    pub fn plugin_name(&self) -> &str {
        match self {
            PluginLintReport::Ok { plugin_name, .. } => plugin_name,
            PluginLintReport::Failed { plugin_name, .. } => plugin_name,
        }
    }
}

/// Run the Contract 5 lint pass over all plugins in the registry that
/// declare `[exports].lint`. Returns one `PluginLintReport` per such
/// plugin, in registration order.
///
/// `pre_expansion` MUST be the AST snapshot taken *before* plugin block
/// expansion. See `foundation/spec/framework/contracts/lint-extension.md`
/// §6 for the ordering requirement and this module's `arena.rs` for the
/// consequences of passing a post-expansion program.
///
/// `config_level` is `"error"`, `"warning"`, or `"info"` — surfaced to
/// plugins in the context JSON per spec §3.1. Callers driving from
/// `cln lint` / `cln check` typically pass `"warning"` (the default) so
/// plugins emit everything at that severity and above; the compiler
/// filters again on the return path in Cycle 3.
///
/// **Plugin discovery in Phase B:** we walk `registry.handlers().values()`
/// deduped by plugin name. A plugin that ships a lint hook but registers
/// no block handler (a hypothetical "pure lint" plugin) would be invisible
/// here. That gap is acceptable for Phase B because every framework plugin
/// currently shipping a lint hook also handles at least one DSL block; a
/// pure-lint plugin can be surfaced additively without a contract change.
pub(crate) fn run_lint_pass(
    registry: &PluginRegistry,
    pre_expansion: &Program,
    config_level: &str,
) -> Vec<PluginLintReport> {
    let mut reports: Vec<PluginLintReport> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for plugin in registry.iter_unique_plugins() {
        let name = plugin.name();
        if !seen.insert(name) {
            continue;
        }
        // Fast path: plugins that don't declare `[exports].lint` return
        // `Ok(None)` from the default trait impl. Skip them without any
        // WASM instantiation.
        match plugin.lint_project(pre_expansion, config_level) {
            Ok(None) => continue,
            Ok(Some(raw_json)) => reports.push(PluginLintReport::Ok {
                plugin_name: name.to_string(),
                raw_json,
            }),
            Err(err) => reports.push(PluginLintReport::Failed {
                plugin_name: name.to_string(),
                message: err.to_string(),
            }),
        }
    }

    reports
}
