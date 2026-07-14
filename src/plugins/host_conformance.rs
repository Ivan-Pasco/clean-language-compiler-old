//! Host registration conformance.
//!
//! Parses every `linker.func_wrap("module", "name", |closure args...| -> ret { ... })`
//! call in a Rust source file and cross-references each registration with the
//! shared [`RegistryIndex`]. The compiler ships two such hosts — the in-process
//! plugin adapter (`src/plugins/wasm_adapter.rs`) and the standalone runner
//! (`src/bin/wasmtime_runner.rs`) — and both must stay in lockstep with the
//! registry signatures the compiler emits as WASM imports. Drift between any
//! pair (host A, host B, registry, compiler-emission) is the root cause of
//! the recurring bridge bugs described in the project diagnostic.
//!
//! Catches:
//!
//! 1. Orphan registrations — a host registers a function the registry doesn't
//!    declare. This means either the registry is missing an entry or the host
//!    has a leftover stub.
//! 2. Type drift — the closure's WASM parameter types or return type don't
//!    match what the registry declares (e.g. `i32` vs `i64`).
//! 3. Semantic argument-order drift — when the registry declares
//!    `param_names = [...]`, the host's closure parameter names must match in
//!    order. This is what catches the `mem_alloc(size, _align)` vs
//!    `mem_alloc(type_id, size)` mismatch that pure-type checking misses
//!    because both shapes are `(i32, i32) -> i32`.
//!
//! Designed to be called from a `#[test]` so CI fails on any drift.

use crate::plugins::registry_loader::{RegistryFunction, RegistryIndex};

/// One issue discovered by [`check_host_source`].
#[derive(Debug, Clone)]
pub struct Issue {
    pub module: String,
    pub name: String,
    pub line: usize,
    pub kind: IssueKind,
}

#[derive(Debug, Clone)]
pub enum IssueKind {
    /// Host registers a name the registry doesn't declare.
    OrphanRegistration,
    /// Registered WASM param types don't match registry params.
    ParamTypeMismatch {
        host: Vec<String>,
        registry: Vec<String>,
    },
    /// Registered return type doesn't match registry return.
    ReturnTypeMismatch { host: String, registry: String },
    /// Registry declares `param_names` and at least one host closure param
    /// name disagrees (in order). The leading `caller` / `_caller` parameter
    /// is excluded — it's the wasmtime context, not a Clean Language param.
    ParamNameMismatch {
        host: Vec<String>,
        registry: Vec<String>,
    },
}

/// Parsed `linker.func_wrap("module", "name", |...|)` invocation.
#[derive(Debug, Clone)]
pub struct Registration {
    pub module: String,
    pub name: String,
    /// Source line (1-based) where the registration begins.
    pub line: usize,
    /// Closure parameter names, excluding the leading `caller` / `_caller`.
    /// Underscore-prefixed names (e.g. `_align`) are preserved as-is.
    pub param_names: Vec<String>,
    /// Closure parameter Rust types in declaration order, again excluding the
    /// leading `caller` parameter.
    pub param_types: Vec<String>,
    /// Rust return type. Empty string means no `-> T` clause (= unit, void).
    pub return_type: String,
}

/// Parse every `linker.func_wrap(...)` invocation in the given source.
///
/// The parser is line-oriented and tolerant of formatting variations. It
/// looks for the literal `linker.func_wrap(` token, reads the first two
/// string literals as `(module, name)`, then locates the closure's `|...|`
/// parameter list and its `-> Type {` return clause.
pub fn parse_registrations(source: &str) -> Vec<Registration> {
    let mut out = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut idx = 0;
    let needle: Vec<char> = "linker.func_wrap(".chars().collect();

    while idx + needle.len() <= chars.len() {
        if chars[idx..idx + needle.len()] == needle[..] {
            let start = idx;
            idx += needle.len();
            // Parse two string-literal arguments.
            let Some((module, after_mod)) = read_string_literal(&chars, idx) else {
                idx += 1;
                continue;
            };
            let Some((name, after_name)) =
                read_string_literal(&chars, skip_to_arg(&chars, after_mod))
            else {
                idx += 1;
                continue;
            };
            // After the second string arg there's a comma, then the closure.
            // Find the opening `|` of the closure.
            let mut cursor = after_name;
            while cursor < chars.len() && chars[cursor] != '|' {
                cursor += 1;
            }
            if cursor >= chars.len() {
                idx = after_name;
                continue;
            }
            // Read closure params up to the matching `|`.
            cursor += 1;
            let params_start = cursor;
            let mut depth_paren = 0;
            let mut depth_angle = 0;
            while cursor < chars.len() {
                let c = chars[cursor];
                match c {
                    '(' => depth_paren += 1,
                    ')' => depth_paren -= 1,
                    '<' => depth_angle += 1,
                    '>' if depth_angle > 0 => depth_angle -= 1,
                    '|' if depth_paren == 0 && depth_angle == 0 => break,
                    _ => {}
                }
                cursor += 1;
            }
            if cursor >= chars.len() {
                idx = after_name;
                continue;
            }
            let params_src: String = chars[params_start..cursor].iter().collect();
            cursor += 1; // past closing |
                         // Optional return clause: `-> Type` before `{`.
            let mut return_type = String::new();
            // Skip whitespace.
            while cursor < chars.len() && chars[cursor].is_whitespace() {
                cursor += 1;
            }
            if cursor + 1 < chars.len() && chars[cursor] == '-' && chars[cursor + 1] == '>' {
                cursor += 2;
                while cursor < chars.len() && chars[cursor].is_whitespace() {
                    cursor += 1;
                }
                let ret_start = cursor;
                let mut depth = 0;
                while cursor < chars.len() {
                    let c = chars[cursor];
                    match c {
                        '<' | '(' | '[' => depth += 1,
                        '>' | ')' | ']' if depth > 0 => depth -= 1,
                        '{' if depth == 0 => break,
                        _ => {}
                    }
                    cursor += 1;
                }
                return_type = chars[ret_start..cursor]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_string();
            }

            // `start` is a char index into `chars`; count '\n' characters up
            // to that point directly (avoids byte-index slicing of `source`,
            // which would panic if a multi-byte character sits before `start`).
            let line = chars[..start.min(chars.len())]
                .iter()
                .filter(|c| **c == '\n')
                .count()
                + 1;

            let (param_names, param_types) = split_closure_params(&params_src);

            out.push(Registration {
                module,
                name,
                line,
                param_names,
                param_types,
                return_type,
            });
            idx = cursor;
        } else {
            idx += 1;
        }
    }
    out
}

/// Read a `"..."` string literal starting at or after `start`. Returns the
/// (unescaped) string and the index just past the closing quote.
fn read_string_literal(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut i = start;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() || chars[i] != '"' {
        return None;
    }
    i += 1;
    let mut s = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            s.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '"' {
            return Some((s, i + 1));
        }
        s.push(c);
        i += 1;
    }
    None
}

/// Advance past a `,` (and whitespace) between two args.
fn skip_to_arg(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            ',' => return i + 1,
            ')' => return i,
            _ => i += 1,
        }
    }
    i
}

/// Split a closure parameter list into (names, types), excluding the leading
/// `caller: Caller<...>` / `_caller` / etc. parameter (which is the wasmtime
/// context, not a Clean Language param).
fn split_closure_params(src: &str) -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut types = Vec::new();
    for part in split_top_level_commas(src) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // Split on the FIRST top-level `:` — Rust closure params are
        // `name: Type` (Type may itself contain `:`-bearing paths like `'a:`,
        // but generic lifetimes are inside `<>` so a top-level `:` is the
        // separator).
        let Some(colon) = find_top_level_colon(part) else {
            // No type annotation (e.g. `|x|`) — record name only.
            names.push(part.to_string());
            types.push(String::new());
            continue;
        };
        let name = part[..colon].trim().to_string();
        let ty = part[colon + 1..].trim().to_string();
        names.push(name);
        types.push(ty);
    }
    // Drop the leading caller parameter. The wasmtime closure convention is
    // that the first arg is always the host context, written as one of:
    //   `mut caller: Caller<'_, S>`
    //   `caller: Caller<'_, S>`
    //   `_caller: Caller<'_, S>`
    //   `_: Caller<'_, S>`
    // We detect it by the TYPE prefix `Caller`, not the name, since the name
    // can be the wildcard `_`.
    let drop_first = types
        .first()
        .map(|t| t.trim_start().starts_with("Caller"))
        .unwrap_or(false);
    if drop_first {
        names.remove(0);
        types.remove(0);
    }
    // Strip `mut ` prefix from names so `mut x` becomes `x` for matching.
    for n in &mut names {
        *n = n.trim_start_matches("mut ").trim().to_string();
    }
    (names, types)
}

fn find_top_level_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' | b')' | b']' => depth -= 1,
            b':' if depth == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' | b')' | b']' => depth -= 1,
            b',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

/// Check a parsed list of host registrations against the registry.
///
/// `host_label` is used in issue rendering (e.g. "wasm_adapter.rs").
/// `expected_aliases_in_registry`: optional. When set, every entry the host
/// registers under this module must exist in the registry; if `None`, only
/// matched entries are checked (orphans are still reported per-entry).
pub fn check_host_registrations(
    registrations: &[Registration],
    registry: &RegistryIndex,
) -> Vec<Issue> {
    let mut issues = Vec::new();
    for reg in registrations {
        // Look up by `name` directly first. The registry's canonical names
        // either match (`mem_alloc`) or appear as aliases (`string.replace`).
        let registry_entry = registry.lookup(&reg.name);
        let Some(entry) = registry_entry else {
            issues.push(Issue {
                module: reg.module.clone(),
                name: reg.name.clone(),
                line: reg.line,
                kind: IssueKind::OrphanRegistration,
            });
            continue;
        };
        // Module check is informational only — the registry's `module` may be
        // the canonical owner while the host registers under a compat alias
        // module like `env`. The wasmtime linker keys by (module, name) so we
        // trust the host's choice unless a strict mode is added later.
        check_param_types(reg, entry, &mut issues);
        check_return_type(reg, entry, &mut issues);
        check_param_names(reg, entry, &mut issues);
    }
    issues
}

fn check_param_types(reg: &Registration, entry: &RegistryFunction, issues: &mut Vec<Issue>) {
    let host_shape: Vec<String> = reg
        .param_types
        .iter()
        .map(|t| rust_type_to_wasm(t).to_string())
        .collect();
    // Convention: a registry "string" param expands to EITHER (i32, i32) —
    // the expand_strings=true wrapper convention, host reads (ptr, len) —
    // OR a single (i32) — the LP-pointer convention, host reads a single
    // length-prefixed pointer. Both are valid per HOST_BRIDGE.md.
    // We accept whichever expansion the host chose.
    if !param_shapes_match(&entry.params, &host_shape) {
        let registry_canonical: Vec<String> = entry
            .params
            .iter()
            .flat_map(|p| registry_type_to_wasm(p, true))
            .map(|s| s.to_string())
            .collect();
        issues.push(Issue {
            module: reg.module.clone(),
            name: reg.name.clone(),
            line: reg.line,
            kind: IssueKind::ParamTypeMismatch {
                host: host_shape,
                registry: registry_canonical,
            },
        });
    }
}

/// Check that `host_shape` (the actual WASM types the closure binds) can be
/// produced by some expansion of `registry_params`. Each registry param has
/// either a single expansion (e.g. `"i64"` → `[i64]`) or multiple options
/// (e.g. `"string"` → `[i32]` or `[i32, i32]`). We greedily match from left
/// to right with a single backtrack point per param, since at most one
/// expansion is multi-option (string).
fn param_shapes_match(registry_params: &[String], host_shape: &[String]) -> bool {
    // Build per-param option list (each option is a Vec of WASM types).
    let options: Vec<Vec<Vec<&'static str>>> = registry_params
        .iter()
        .map(|p| {
            let lp = registry_type_to_wasm(p, false);
            let expanded = registry_type_to_wasm(p, true);
            if lp == expanded {
                vec![lp]
            } else {
                vec![lp, expanded]
            }
        })
        .collect();

    fn try_match(options: &[Vec<Vec<&'static str>>], i: usize, host: &[String], hi: usize) -> bool {
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

fn check_return_type(reg: &Registration, entry: &RegistryFunction, issues: &mut Vec<Issue>) {
    let host_ret = rust_type_to_wasm(&reg.return_type);
    let registry_ret = match entry.returns.as_str() {
        "i32" | "boolean" | "ptr" | "string" | "handler" => "i32",
        "i64" | "integer" => "i64",
        "f64" | "number" => "f64",
        "void" | "" => "void",
        _ => "unknown",
    };
    // Convention: Clean Language's `external:` declaration emits a `(result
    // i32)` on every import regardless of whether the Clean-side return type
    // is void, so hosts may legally return i32 for a registry "void". The
    // reverse — registry "i32" registered as void — is real drift.
    let ok = match (host_ret, registry_ret) {
        (h, r) if h == r => true,
        ("i32", "void") => true,
        _ => false,
    };
    if !ok {
        issues.push(Issue {
            module: reg.module.clone(),
            name: reg.name.clone(),
            line: reg.line,
            kind: IssueKind::ReturnTypeMismatch {
                host: host_ret.to_string(),
                registry: registry_ret.to_string(),
            },
        });
    }
}

fn check_param_names(reg: &Registration, entry: &RegistryFunction, issues: &mut Vec<Issue>) {
    if entry.param_names.is_empty() {
        return;
    }
    // Expand registry param names to WASM-shape positions, matching the way
    // `params` expands. For a `"string"` param the registry has one name but
    // the WASM-level closure takes two params (ptr, len) by convention; in
    // that case we accept either `<name>_ptr`/`<name>_len` pairs OR a single
    // `<name>` (when expand_strings=false, single lp-ptr).
    let mut expected: Vec<Vec<String>> = Vec::new();
    for (i, p) in entry.params.iter().enumerate() {
        let nm = entry.param_names.get(i).map(|s| s.as_str()).unwrap_or("_");
        match strip_tag(p) {
            "string" => {
                expected.push(vec![
                    nm.to_string(),
                    format!("{nm}_ptr"),
                    format!("_{nm}_ptr"),
                ]);
                expected.push(vec![format!("{nm}_len"), format!("_{nm}_len")]);
            }
            _ => {
                expected.push(vec![nm.to_string(), format!("_{nm}")]);
            }
        }
    }
    let host = &reg.param_names;
    let mut mismatch = false;
    if host.len() != expected.len() {
        mismatch = true;
    } else {
        for (h, accept) in host.iter().zip(expected.iter()) {
            if !accept.iter().any(|a| a == h) {
                mismatch = true;
                break;
            }
        }
    }
    if mismatch {
        issues.push(Issue {
            module: reg.module.clone(),
            name: reg.name.clone(),
            line: reg.line,
            kind: IssueKind::ParamNameMismatch {
                host: host.clone(),
                registry: entry.param_names.clone(),
            },
        });
    }
}

fn rust_type_to_wasm(t: &str) -> &'static str {
    let t = t.trim();
    if t.is_empty() {
        return "void";
    }
    // Strip trailing punctuation noise and outer references.
    let t = t.trim_end_matches([',', ';']).trim();
    match t {
        "i32" | "u32" => "i32",
        "i64" | "u64" => "i64",
        "f32" => "f32",
        "f64" => "f64",
        "()" => "void",
        _ => "unknown",
    }
}

/// Expand a registry param type to WASM types. For `"string"` the expansion
/// depends on the host's chosen convention: `expand_strings=true` yields
/// `(i32, i32)` (ptr, len pair) while `expand_strings=false` yields `(i32)`
/// (single LP-pointer). All other types are unambiguous.
fn registry_type_to_wasm(t: &str, expand_strings: bool) -> Vec<&'static str> {
    match strip_tag(t) {
        "string" if expand_strings => vec!["i32", "i32"],
        "string" | "ptr" | "boolean" | "i32" | "u32" | "handler" => vec!["i32"],
        "integer" | "i64" => vec!["i64"],
        "number" | "f64" => vec!["f64"],
        "void" | "" => vec![],
        _ => vec!["unknown"],
    }
}

fn strip_tag(t: &str) -> &str {
    t.split(':').next().unwrap_or(t)
}

/// Format a list of issues as a single human-readable error message.
/// Returns `None` if the slice is empty.
pub fn format_issues(issues: &[Issue], host_label: &str) -> Option<String> {
    if issues.is_empty() {
        return None;
    }
    let mut s = format!(
        "Host registration conformance failed for {host_label} ({} issue{}):\n",
        issues.len(),
        if issues.len() == 1 { "" } else { "s" }
    );
    for issue in issues {
        s.push_str("  - ");
        s.push_str(&format!(
            "{}::{} (line {}): ",
            issue.module, issue.name, issue.line
        ));
        match &issue.kind {
            IssueKind::OrphanRegistration => {
                s.push_str("not declared in function-registry.toml. Either add it to the registry (with developer approval) or remove the host registration.");
            }
            IssueKind::ParamTypeMismatch { host, registry } => {
                s.push_str(&format!(
                    "param WASM shape {host:?} does not match registry {registry:?}"
                ));
            }
            IssueKind::ReturnTypeMismatch { host, registry } => {
                s.push_str(&format!(
                    "return shape {host:?} does not match registry {registry:?}"
                ));
            }
            IssueKind::ParamNameMismatch { host, registry } => {
                s.push_str(&format!(
                    "param names {host:?} do not match registry param_names {registry:?} (order matters; this catches argument-order drift like mem_alloc(size, _align) vs (type_id, size))"
                ));
            }
        }
        s.push('\n');
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_registration() {
        let src = r#"
            linker.func_wrap(
                "memory_runtime",
                "mem_alloc",
                |mut caller: Caller<'_, PluginState>, type_id: i32, size: i32| -> i32 {
                    let _ = (caller, type_id, size);
                    0
                },
            )?;
        "#;
        let regs = parse_registrations(src);
        assert_eq!(regs.len(), 1, "expected one registration");
        let r = &regs[0];
        assert_eq!(r.module, "memory_runtime");
        assert_eq!(r.name, "mem_alloc");
        assert_eq!(r.param_names, vec!["type_id", "size"]);
        assert_eq!(r.param_types, vec!["i32", "i32"]);
        assert_eq!(r.return_type, "i32");
    }

    #[test]
    fn parses_void_return() {
        let src = r#"
            linker.func_wrap(
                "memory_runtime",
                "mem_retain",
                |_: Caller<'_, PluginState>, _ptr: i32| {},
            )?;
        "#;
        let regs = parse_registrations(src);
        assert_eq!(regs.len(), 1);
        assert_eq!(regs[0].return_type, "");
        assert_eq!(regs[0].param_names, vec!["_ptr"]);
    }

    #[test]
    fn detects_param_order_drift() {
        // Synthetic registry with explicit names.
        let registry = RegistryIndex::from_toml_str(
            r#"
            [meta]
            version = "test"
            [[functions]]
            name = "mem_alloc"
            module = "memory_runtime"
            params = ["i32", "i32"]
            param_names = ["type_id", "size"]
            returns = "i32"
            "#,
        )
        .unwrap();

        // Host registers with WRONG argument order.
        let regs = parse_registrations(
            r#"
            linker.func_wrap("memory_runtime", "mem_alloc",
                |mut caller: Caller<'_, S>, size: i32, _align: i32| -> i32 { 0 })?;
            "#,
        );
        let issues = check_host_registrations(&regs, &registry);
        assert!(
            issues
                .iter()
                .any(|i| matches!(i.kind, IssueKind::ParamNameMismatch { .. })),
            "expected ParamNameMismatch, got {:?}",
            issues
        );
    }

    #[test]
    fn passes_when_param_names_match() {
        let registry = RegistryIndex::from_toml_str(
            r#"
            [meta]
            version = "test"
            [[functions]]
            name = "mem_alloc"
            module = "memory_runtime"
            params = ["i32", "i32"]
            param_names = ["type_id", "size"]
            returns = "i32"
            "#,
        )
        .unwrap();
        let regs = parse_registrations(
            r#"
            linker.func_wrap("memory_runtime", "mem_alloc",
                |mut caller: Caller<'_, S>, type_id: i32, size: i32| -> i32 { 0 })?;
            "#,
        );
        let issues = check_host_registrations(&regs, &registry);
        assert!(issues.is_empty(), "no issues expected, got {issues:?}");
    }

    #[test]
    fn detects_orphan_registration() {
        let registry = RegistryIndex::from_toml_str(
            r#"
            [meta]
            version = "test"
            [[functions]]
            name = "real_one"
            module = "env"
            params = []
            returns = "void"
            "#,
        )
        .unwrap();
        let regs =
            parse_registrations(r#"linker.func_wrap("env", "ghost_fn", |_: Caller<'_, S>| {})?;"#);
        let issues = check_host_registrations(&regs, &registry);
        assert!(matches!(issues[0].kind, IssueKind::OrphanRegistration));
    }

    #[test]
    fn underscore_prefixed_name_accepted() {
        // Host writes `_size` to silence unused warnings; should still match.
        let registry = RegistryIndex::from_toml_str(
            r#"
            [meta]
            version = "test"
            [[functions]]
            name = "noop"
            module = "env"
            params = ["i32"]
            param_names = ["size"]
            returns = "void"
            "#,
        )
        .unwrap();
        let regs = parse_registrations(
            r#"linker.func_wrap("env", "noop", |_: Caller<'_, S>, _size: i32| {})?;"#,
        );
        let issues = check_host_registrations(&regs, &registry);
        assert!(
            issues.is_empty(),
            "underscore prefix should match: {issues:?}"
        );
    }
}
