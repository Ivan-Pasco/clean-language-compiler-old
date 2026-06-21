//! Cross-component contract test: framework plugin sources must not emit
//! bridge function calls that don't match the bridge declarations in
//! their own `plugin.toml`.
//!
//! Why this exists
//! ---------------
//! Each framework plugin (`clean-framework/plugins/frame.*`) emits Clean
//! Language source code at compile time — `process_html`, `assemble`,
//! `generate_page_class`, `emit_ssr_helpers`, etc. all concatenate strings
//! that end up being parsed as Clean programs. Those generated programs
//! call bridge functions like `_ui_load_layout`, `_db_query`, `_req_param`,
//! and those bridge functions must be declared in the same plugin's
//! `[bridge.functions]` table (or another plugin's, via the registry) so
//! the compiler resolves them.
//!
//! The canonical bridge naming convention is **snake_case throughout**:
//! `_<namespace>_<one_or_more_snake_words>`. The plugin manifest follows
//! it consistently because TOML keys are read directly. The plugin source,
//! however, is hand-written text inside string literals — easy to typo as
//! `_ui_loadLayout` (snake namespace + camelCase function), which type-checks
//! at the plugin level (it's just a string) and only surfaces as
//! `error[SEM007]: Function '_ui_loadLayout' not found` when a real user
//! tries to compile a project that exercises the buggy code path. This test
//! catches that drift class at `cargo test` time instead of in production.
//!
//! Surfaced bugs at landing time
//! ------------------------------
//! When this test was first written it failed against
//! `clean-framework/plugins/frame.ui/src/main.cln` on two lines:
//!   - L1852: `_ui_loadLayout`        (should be `_ui_load_layout`)
//!   - L3690: `_ui_injectHeadCss`     (should be `_ui_inject_head_css`)
//!
//! Both were the next blockers for shipping page projects after the
//! COMPILER-PLUGIN-ASSEMBLE-HANGS-ON-PAGE-PROJECTS hang and the
//! SEM-PAGE-COMPANION-NAMING-DRIFT (b8b2e7504ff7) were resolved on the
//! compiler side.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Returns the path to the clean-framework plugins directory, or `None` if
/// this checkout is missing the sibling workspace. Mirrors the gating in
/// `test_plugin_registry_contract.rs::framework_plugins_dir` so CI runs
/// that build the compiler in isolation skip the test cleanly.
fn framework_plugins_dir() -> Option<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let candidate = Path::new(manifest_dir)
        .parent()?
        .join("clean-framework")
        .join("plugins");
    if candidate.is_dir() {
        Some(candidate)
    } else {
        None
    }
}

/// One offending identifier found inside a plugin source string literal.
struct Violation {
    plugin: String,
    file: PathBuf,
    line: usize,
    identifier: String,
    /// The canonical snake_case form we *would* expect the source to use.
    suggested: String,
}

/// Convert `_ui_loadLayout` → `_ui_load_layout` by inserting `_` before each
/// embedded uppercase letter and lowercasing it. Idempotent for inputs that
/// are already snake_case.
fn canonical_snake_case(ident: &str) -> String {
    let mut out = String::with_capacity(ident.len() + 4);
    for (i, ch) in ident.chars().enumerate() {
        if ch.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
    }
    out
}

/// Scan one plugin source file for snake-prefixed-then-camelCase identifiers
/// inside string literals. The regex requires **at least two** underscore-
/// separated snake segments before any camel hump (`_<ns>_<word><Camel...>`),
/// which suppresses class-method names like `_getId`, `_setCookie`,
/// `_onClick` that are emitted as object properties rather than bridge calls.
fn scan_file(plugin: &str, file: &Path) -> Vec<Violation> {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut violations = Vec::new();
    for (lineno, raw_line) in src.lines().enumerate() {
        // Drop end-of-line comments — `// ...` text isn't compiled and
        // sometimes documents historical names we don't want to flag.
        let line = strip_trailing_line_comment(raw_line);

        // Walk string literals manually: `"` opens, `\` escapes the next char,
        // `"` closes. We process only what's inside the literal body.
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() {
                    if bytes[j] == b'\\' && j + 1 < bytes.len() {
                        j += 2;
                        continue;
                    }
                    if bytes[j] == b'"' {
                        break;
                    }
                    j += 1;
                }
                let inner = &line[start..j.min(line.len())];
                for ident in extract_camel_bridge_calls(inner) {
                    let suggested = canonical_snake_case(&ident);
                    if suggested != ident {
                        violations.push(Violation {
                            plugin: plugin.to_string(),
                            file: file.to_path_buf(),
                            line: lineno + 1,
                            identifier: ident,
                            suggested,
                        });
                    }
                }
                i = j + 1;
            } else {
                i += 1;
            }
        }
    }
    violations
}

/// Strip a trailing `// ...` comment from a single line, respecting strings.
/// Simple state machine — anything inside `"..."` is preserved verbatim.
fn strip_trailing_line_comment(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_str && i + 1 < bytes.len() => i += 2,
            b'"' => {
                in_str = !in_str;
                i += 1;
            }
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                return line[..i].to_string();
            }
            _ => i += 1,
        }
    }
    line.to_string()
}

/// Extract candidate bridge-call identifiers from a literal's body: a leading
/// `_`, at least one `_<lowercase>` segment, then content where the *first*
/// uppercase letter trips the camelCase detector. Matches end at the natural
/// identifier boundary (next non-`[A-Za-z0-9_]` char).
fn extract_camel_bridge_calls(literal_body: &str) -> Vec<String> {
    let bytes = literal_body.as_bytes();
    let mut hits = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Look for the start of an identifier: '_' followed by lowercase
        if bytes[i] == b'_' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_lowercase() {
            // Walk the identifier
            let start = i;
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            let ident = &literal_body[start..j];
            // Require at least two underscore-prefixed snake segments
            // (so `_ui_loadLayout` qualifies but `_getId` doesn't).
            let snake_segment_count = ident
                .strip_prefix('_')
                .unwrap_or(ident)
                .split('_')
                .filter(|s| !s.is_empty())
                .count();
            // Has at least one ASCII uppercase letter anywhere?
            let has_camel = ident.bytes().any(|b| b.is_ascii_uppercase());
            if snake_segment_count >= 2 && has_camel {
                hits.push(ident.to_string());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    hits
}

#[test]
fn framework_plugin_sources_use_snake_case_bridge_names() {
    let Some(plugins_dir) = framework_plugins_dir() else {
        eprintln!(
            "skipping framework_plugin_sources_use_snake_case_bridge_names: \
             clean-framework checkout not present alongside the compiler"
        );
        return;
    };

    let mut violations: Vec<Violation> = Vec::new();
    let mut plugins_scanned = BTreeSet::new();

    let entries = std::fs::read_dir(&plugins_dir).expect("read clean-framework/plugins/");
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(plugin_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        // We only care about source plugins (frame.*) that ship a src/
        // directory with .cln sources. Skip anything else (versioned dirs,
        // build artifacts, etc.).
        let src_dir = path.join("src");
        if !src_dir.is_dir() {
            continue;
        }
        plugins_scanned.insert(plugin_name.to_string());
        for src_entry in std::fs::read_dir(&src_dir)
            .expect("read plugin src/")
            .flatten()
        {
            let src_path = src_entry.path();
            if src_path.extension().and_then(|s| s.to_str()) != Some("cln") {
                continue;
            }
            violations.extend(scan_file(plugin_name, &src_path));
        }
    }

    assert!(
        !plugins_scanned.is_empty(),
        "no plugin source directories were scanned — \
         is clean-framework/plugins/ structured as expected?"
    );

    if violations.is_empty() {
        return;
    }

    // Render the failure as a single block so a developer sees every drift
    // in one read instead of one assertion firing at a time.
    let mut msg = String::from(
        "\nFramework plugin source emits bridge-call identifiers that don't \
         match the snake_case bridge naming convention. Each one will fail \
         downstream with SEM007 when a real project exercises the code path.\n\n\
         Convention: every bridge call must be all-lowercase snake_case, \
         matching the `name = \"_..._...\"` entry in the plugin's plugin.toml \
         `[bridge.functions]` table. Camel humps inside emitted source strings \
         never resolve, because TOML keys are already canonical.\n\n",
    );
    for v in &violations {
        msg.push_str(&format!(
            "  {plugin}  {file}:{line}\n    found     `{found}`\n    expected  `{expected}`\n\n",
            plugin = v.plugin,
            file = v
                .file
                .strip_prefix(&plugins_dir)
                .unwrap_or(&v.file)
                .display(),
            line = v.line,
            found = v.identifier,
            expected = v.suggested,
        ));
    }
    panic!("{msg}");
}

// -----------------------------------------------------------------------
// Unit coverage for the helpers — runs without clean-framework present.
// -----------------------------------------------------------------------

#[test]
fn canonical_snake_case_inserts_underscores_before_camel_humps() {
    assert_eq!(canonical_snake_case("_ui_loadLayout"), "_ui_load_layout");
    assert_eq!(
        canonical_snake_case("_ui_injectHeadCss"),
        "_ui_inject_head_css"
    );
    assert_eq!(
        canonical_snake_case("_db_findFirstByEmail"),
        "_db_find_first_by_email"
    );
    // Already-canonical names are returned unchanged.
    assert_eq!(canonical_snake_case("_ui_load_layout"), "_ui_load_layout");
    assert_eq!(canonical_snake_case("print"), "print");
}

#[test]
fn extract_camel_bridge_calls_requires_two_snake_segments() {
    // Two snake segments + camel hump → matches.
    assert_eq!(
        extract_camel_bridge_calls("string x = _ui_loadLayout(name)"),
        vec!["_ui_loadLayout".to_string()]
    );

    // Single snake segment (method-name shape) → ignored.
    let single = extract_camel_bridge_calls("obj._getId()");
    assert!(
        single.is_empty(),
        "single-segment identifiers like `_getId` are likely class methods, \
         not bridge calls, and should not be flagged. Got: {single:?}"
    );

    // All snake_case → matches the leading-`_` shape but lacks camel; ignored.
    assert!(extract_camel_bridge_calls("_ui_load_layout(x)").is_empty());

    // Non-underscore-prefixed names → ignored entirely.
    assert!(extract_camel_bridge_calls("doSomething()").is_empty());
}

#[test]
fn strip_trailing_line_comment_preserves_string_contents() {
    assert_eq!(
        strip_trailing_line_comment("let x = 1 // this is a comment"),
        "let x = 1 "
    );
    // `//` inside a string literal must NOT be treated as a comment.
    assert_eq!(
        strip_trailing_line_comment(r#"let s = "http://example.com" // real comment"#),
        r#"let s = "http://example.com" "#
    );
}
