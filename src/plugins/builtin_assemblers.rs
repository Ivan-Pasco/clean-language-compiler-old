//! Built-in plugin assemblers.
//!
//! These are Rust implementations of the `assemble` hook that run when no
//! WASM-side hook is provided by the relevant plugin. Each assembler encapsulates
//! framework-specific source transformation logic that MUST eventually migrate
//! to the corresponding WASM plugin. The shim exists purely for backward
//! compatibility — remove it once the framework plugin ships its WASM `assemble`
//! export.
//!
//! # Current shims
//!
//! - `PageCompanionAssembler` — frame.ui/frame.server page companion detection,
//!   function prefixing, and synthetic route module generation. Replaces the
//!   six hardcoded functions that were previously in `multi_file_compiler.rs`.
//!   Remove when frame.ui implements `[exports] assemble`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ast::{FrameworkBlock, Statement};
use crate::plugins::plugin_abi::{
    AssembleInput, AssembleOutput, InjectedSource, TransformedSource,
};
use crate::plugins::{FrameworkPlugin, PluginError, PluginResult};

// ============================================================================
// Page Companion Assembler (frame.ui / frame.server shim)
// ============================================================================

/// Record of a page companion .cln file discovered during shared folder scan.
pub struct PageCompanionRecord {
    pub module_name: String,
    pub route_path: String,
    pub page_name: String,
    pub has_guard: bool,
    pub has_load: bool,
}

/// Derive a unique module name from a companion file path relative to the shared directory.
///
/// Examples:
/// - `app/pages/dashboard.cln` → `pages_dashboard`
/// - `app/pages/blog/[slug].cln` → `pages_blog_slug`
pub fn derive_companion_module_name(path: &Path, base_dir: &Path) -> String {
    let relative = path.strip_prefix(base_dir).unwrap_or(path);
    relative
        .to_str()
        .unwrap_or("")
        .trim_end_matches(".cln")
        .replace('/', "_")
        .replace(['[', ']'], "")
}

/// Prefix companion functions with the module name to avoid symbol collisions.
pub fn prefix_companion_functions(source: &str, module_name: &str) -> String {
    let source = source
        .replace("any load(Request ", "any load(any ")
        .replace("any guard(Request ", "any guard(any ");

    source
        .replace("guard()", &format!("{}_guard()", module_name))
        .replace("load()", &format!("{}_load()", module_name))
        .replace("any guard(", &format!("any {}_guard(", module_name))
        .replace("any load(", &format!("any {}_load(", module_name))
}

/// Derive the URL route path from a .cln page companion file path.
pub fn derive_page_route_from_cln(file_path: &Path, shared_dir: &Path) -> String {
    let relative = file_path.strip_prefix(shared_dir).unwrap_or(file_path);
    let mut route = String::from("/");
    let mut past_pages = false;

    for component in relative.components() {
        if let std::path::Component::Normal(name) = component {
            let name_str = name.to_string_lossy();

            if !past_pages {
                if name_str == "pages" {
                    past_pages = true;
                }
                continue;
            }

            let name_str = name_str.trim_end_matches(".cln");

            if name_str == "index" {
                continue;
            }

            let segment = if name_str.starts_with('[') && name_str.ends_with(']') {
                format!(":{}", &name_str[1..name_str.len() - 1])
            } else {
                name_str.to_string()
            };

            if !route.ends_with('/') {
                route.push('/');
            }
            route.push_str(&segment);
        }
    }

    if route != "/" && route.ends_with('/') {
        route.pop();
    }

    route
}

/// Derive the template path relative to the project root for `_ui_render_page`.
pub fn derive_page_name_from_cln(file_path: &Path, project_root: &Path) -> String {
    let relative = file_path.strip_prefix(project_root).unwrap_or(file_path);
    let mut parts: Vec<String> = relative
        .components()
        .filter_map(|c| {
            if let std::path::Component::Normal(name) = c {
                Some(name.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect();

    if parts.is_empty() {
        return "index.html".to_string();
    }

    if let Some(last) = parts.last_mut() {
        if last.ends_with(".cln") {
            *last = format!("{}.html", &last[..last.len() - 4]);
        }
    }
    parts.join("/")
}

/// Build the params object literal from dynamic route segments.
fn route_path_to_params_literal(route_path: &str) -> String {
    let params: Vec<String> = route_path
        .split('/')
        .filter_map(|seg| seg.strip_prefix(':'))
        .map(|name| format!("{}: _req_param(\"{}\")", name, name))
        .collect();

    if params.is_empty() {
        "{}".to_string()
    } else {
        format!("{{ {} }}", params.join(", "))
    }
}

/// Generate the synthetic Clean Language route registration module.
pub fn generate_page_route_source(records: &[PageCompanionRecord]) -> String {
    let mut src = String::new();

    src.push_str("start:\n");
    src.push_str("\tinteger page_route_status = 0\n");

    for record in records {
        let handler_name = format!("__page_handler_{}", record.module_name);
        src.push_str(&format!(
            "\tpage_route_status = _http_route(\"GET\", \"{}\", \"{}\")\n",
            record.route_path, handler_name
        ));
    }

    src.push('\n');
    src.push_str("functions:\n");

    for record in records {
        let handler_name = format!("__page_handler_{}", record.module_name);
        let params_lit = route_path_to_params_literal(&record.route_path);

        src.push_str(&format!("\tstring {}()\n", handler_name));
        src.push_str("\t\tany __req_auth = { loggedIn: _auth_require_auth() == 1, userId: \"\", role: \"\", roles: \"[]\" }\n");
        src.push_str(&format!("\t\tany __req_params = {}\n", params_lit));
        src.push_str("\t\tany __page_req = { auth: __req_auth, params: __req_params, query: {}, body: _req_body(), headers: {}, method: _req_method(), path: _req_path(), ip: _req_ip() }\n");

        if record.has_guard {
            src.push_str(&format!(
                "\t\tany guard_result = {}_guard(__page_req)\n",
                record.module_name
            ));
            src.push_str("\t\tif guard_result != null\n");
            src.push_str("\t\t\treturn \"\"\n");
        }

        if record.has_load {
            src.push_str(&format!(
                "\t\tany data = {}_load(__page_req)\n",
                record.module_name
            ));
            src.push_str("\t\tstring page_json = json.encode(data)\n");
            src.push_str(&format!(
                "\t\treturn _ui_render_page(\"{}\", page_json)\n",
                record.page_name
            ));
        } else {
            src.push_str(&format!(
                "\t\treturn _ui_render_page(\"{}\", \"{{}}\")\n",
                record.page_name
            ));
        }

        src.push('\n');
    }

    src
}

// ============================================================================
// FrameworkPlugin impl for PageCompanionAssembler
// ============================================================================

/// Built-in assembler that handles page companion detection, function prefixing,
/// and synthetic route module generation.
///
/// This is a Rust shim for the logic that will eventually live in the frame.ui
/// WASM plugin's `assemble` export. It implements the `FrameworkPlugin` trait
/// so it can be registered in the `PluginRegistry` alongside WASM plugins.
///
/// It only overrides `assemble()` — all other trait methods return no-op defaults.
pub struct PageCompanionAssembler;

impl FrameworkPlugin for PageCompanionAssembler {
    fn name(&self) -> &'static str {
        "builtin:page-companion-assembler"
    }

    fn handles(&self) -> &'static [&'static str] {
        &[]
    }

    fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        Err(PluginError::UnknownBlockType {
            block_name: "builtin:page-companion-assembler".to_string(),
            location: None,
        })
    }

    fn assemble(&self, input: &AssembleInput) -> PluginResult<AssembleOutput> {
        let project_root = PathBuf::from(&input.project_root);
        let manifest_dir = PathBuf::from(&input.manifest_dir);

        let mut records: Vec<PageCompanionRecord> = Vec::new();
        let mut transformed: Vec<TransformedSource> = Vec::new();

        for file in &input.source_files {
            let file_path = PathBuf::from(&file.path);

            // Determine which shared directory this file belongs to by checking if
            // any ancestor directory is a known shared folder. Since we don't carry
            // shared_dirs in AssembleInput, we infer it from the path: find the
            // first ancestor that contains a "pages" component.
            let shared_dir = find_shared_dir_for_page(&file_path);
            let shared_dir = match shared_dir {
                Some(d) => d,
                None => continue, // not under any pages/ hierarchy
            };

            let is_page_companion = {
                let relative = file_path.strip_prefix(&shared_dir).unwrap_or(&file_path);
                relative
                    .components()
                    .any(|c| matches!(c, std::path::Component::Normal(n) if n == "pages"))
            };

            if !is_page_companion {
                continue;
            }

            let raw_source = &file.content;
            let is_active = raw_source.contains("any load(") || raw_source.contains("any guard(");
            if !is_active {
                continue;
            }

            let module_name = derive_companion_module_name(&file_path, &shared_dir);
            let prefixed = prefix_companion_functions(raw_source, &module_name);
            let route = derive_page_route_from_cln(&file_path, &shared_dir);
            let page_name = derive_page_name_from_cln(&file_path, &project_root);

            records.push(PageCompanionRecord {
                module_name,
                route_path: route,
                page_name,
                has_guard: raw_source.contains("any guard("),
                has_load: raw_source.contains("any load("),
            });

            transformed.push(TransformedSource {
                path: file.path.clone(),
                content: prefixed,
            });
        }

        if transformed.is_empty() {
            return Ok(AssembleOutput::default());
        }

        // Synthetic route registration module is only generated when frame.server
        // is declared — without a server, there is nothing to register routes with.
        let injected = if input.has_frame_server && !records.is_empty() {
            let synthetic_source = generate_page_route_source(&records);
            let synthetic_path = format!("{}/__page_routes_generated.cln", manifest_dir.display());
            vec![InjectedSource {
                virtual_path: synthetic_path,
                content: synthetic_source,
            }]
        } else {
            vec![]
        };

        Ok(AssembleOutput {
            injected_sources: injected,
            transformed_sources: transformed,
        })
    }
}

/// Infer the shared directory for a page companion by walking up the path
/// until we find an ancestor that is the parent of a `pages/` component.
///
/// Example: `/project/app/web/pages/login.cln` → `/project/app/web`
fn find_shared_dir_for_page(file_path: &Path) -> Option<PathBuf> {
    let components: Vec<_> = file_path.components().collect();
    for (i, component) in components.iter().enumerate() {
        if let std::path::Component::Normal(name) = component {
            if *name == "pages" && i > 0 {
                // The shared dir is everything before "pages/"
                let shared: PathBuf = components[..i].iter().collect();
                return Some(shared);
            }
        }
    }
    None
}

/// Construct a `PageCompanionAssembler` wrapped in `Arc<dyn FrameworkPlugin>`.
pub fn page_companion_assembler() -> Arc<dyn FrameworkPlugin> {
    Arc::new(PageCompanionAssembler)
}
