//! Page-companion path helpers shared by the multi-file compilation pipeline.
//!
//! These functions translate file paths under a project's `pages/` hierarchy
//! into the module names, route strings, and source transformations the
//! compiler needs to wire page companions into the build graph. They are pure
//! path / string utilities — they do not parse Clean source.
//!
//! The previous in-tree `PageCompanionAssembler` Rust shim that implemented
//! the `assemble` lifecycle hook was deleted as part of
//! `BUILTIN-NAMESPACE-OVERREACH` step 2: frame.ui declares
//! `[exports].assemble` in its plugin.toml, and the compiler delegates to it
//! via `PluginRegistry::run_assemble_hooks` (see `multi_file_compiler.rs`).

use std::path::Path;

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

/// Derive the canonical module name for a page-companion .cln file.
///
/// **Must match frame.ui's `derive_module_name`** in
/// `clean-framework/plugins/frame.ui/src/main.cln:~4291-4305` exactly —
/// the compiler passes the result into `process_html`'s `companion_json`
/// as `module_name`, and the plugin uses it to synthesize calls into
/// the renamed companion. frame.ui's `assemble` then re-derives the same
/// name (with its own copy of this algorithm) when it renames the
/// user-written `any load(Request req)` to `any <module>_load_impl(any)`.
/// If the two sides disagree, the synthesized call resolves to a name
/// the renamed function never defines and the build fails with
/// SEM007 — exactly the symptom that surfaced as the next blocker after
/// COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS was fixed.
///
/// The algorithm:
///   1. Find the *last* `pages/` segment in the path and keep only what
///      comes after it. (Choosing the last occurrence is intentional:
///      `app/pages/blog/pages.cln` should give `pages_pages`, not
///      `pages_blog_pages` — frame.ui matches on the segment.)
///   2. Strip the `.cln` extension.
///   3. Flatten the rest with `/` → `_`, drop the brackets around dynamic
///      segments (`[slug]` → `slug`), and rewrite identifier-invalid
///      characters (`-` → `_`, `.` → `_`).
///   4. Prefix `pages_`.
///
/// `base_dir` is accepted for API stability with the previous signature
/// but no longer participates: the `pages/` marker is the canonical
/// anchor, and locating it inside the full path is more robust than
/// stripping an arbitrary base prefix (which produced the buggy
/// `app_ui_web_pages_home`-style names whenever the caller passed
/// `manifest_dir` instead of the shared folder root).
///
/// Examples:
/// - `app/pages/dashboard.cln`        → `pages_dashboard`
/// - `app/pages/blog/post.cln`        → `pages_blog_post`
/// - `app/pages/blog/[slug].cln`      → `pages_blog_slug`
/// - `app/ui/web/pages/home.cln`      → `pages_home`
/// - `app/ui/web/pages/blog/index.cln`→ `pages_blog_index`
pub fn derive_companion_module_name(path: &Path, _base_dir: &Path) -> String {
    let path_str = path.to_str().unwrap_or("");

    // Locate the last `pages/` segment in the path.
    // `rfind` mirrors how frame.ui's source plugin walks the path:
    // both ends up using whatever follows the most recent `pages/` boundary.
    let after_pages = match path_str.rfind("pages/") {
        Some(idx) => &path_str[idx + "pages/".len()..],
        // No `pages/` segment — fall back to the bare file stem so callers
        // outside the page hierarchy still get a usable identifier.
        None => path_str.rsplit('/').next().unwrap_or(path_str),
    };

    let stem = after_pages.strip_suffix(".cln").unwrap_or(after_pages);

    let sanitized: String = stem
        .chars()
        .filter(|c| !matches!(c, '[' | ']'))
        .map(|c| match c {
            '/' | '-' | '.' => '_',
            other => other,
        })
        .collect();

    format!("pages_{sanitized}")
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
