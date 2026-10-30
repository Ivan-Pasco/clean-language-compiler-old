//! Page-companion path helper shared by the multi-file compilation pipeline.
//!
//! Translates a file path under a project's `pages/` hierarchy into the
//! module name the compiler hands to the owning plugin. It is a pure path /
//! string utility — it does not parse Clean source. Route registration and
//! companion-function renaming are the plugin's job (frame.ui `assemble`).
//!
//! The previous in-tree `PageCompanionAssembler` Rust shim that implemented
//! the `assemble` lifecycle hook was deleted as part of
//! `BUILTIN-NAMESPACE-OVERREACH` step 2: frame.ui declares
//! `[exports].assemble` in its plugin.toml, and the compiler delegates to it
//! via `PluginRegistry::run_assemble_hooks` (see `multi_file_compiler.rs`).

use std::path::Path;

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
/// The `pages/` marker is the canonical anchor: locating it inside the full
/// path is more robust than stripping a caller-supplied base prefix (which
/// produced buggy `app_ui_web_pages_home`-style names whenever the caller
/// passed `manifest_dir` instead of the shared folder root).
///
/// Examples:
/// - `app/pages/dashboard.cln`        → `pages_dashboard`
/// - `app/pages/blog/post.cln`        → `pages_blog_post`
/// - `app/pages/blog/[slug].cln`      → `pages_blog_slug`
/// - `app/ui/web/pages/home.cln`      → `pages_home`
/// - `app/ui/web/pages/blog/index.cln`→ `pages_blog_index`
pub fn derive_companion_module_name(path: &Path) -> String {
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
