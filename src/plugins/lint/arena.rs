/// Single-call arena for the plugin lint extension (Contract 5).
///
/// Created at the start of every `lint_project` invocation, dropped on
/// return. Owns a clone of the fully-resolved `Program` and answers the
/// 4 read-only AST accessors defined in
/// `foundation/spec/framework/contracts/lint-extension.md` §4:
///
///   - `_ast_list_classes`
///   - `_ast_class_fields`
///   - `_ast_list_functions`
///   - `_ast_list_blocks`
///
/// The arena is single-use: valid only for the duration of one `lint_project`
/// call. Stale reuse — a plugin that stashes the handle and calls back after
/// the return — is naturally impossible because the arena is dropped when the
/// wasmtime `Store` is dropped after the call. Within a call, every accessor
/// validates the `handle` parameter against the arena's `handle()` and
/// returns an `AST-HANDLE-INVALID` JSON error payload on mismatch.
///
/// This mirrors the design of `typed_emission::arena::EmitArena` (single-call,
/// monotonic ctx, dropped on Store drop), minus the consumption tracking and
/// allocation surface — lint is strictly read-only.
use crate::ast::{Program, Statement, Type};

/// Build a JSON error payload for stale-handle detection.
///
/// The plugin sees this string returned from any accessor whose `handle`
/// parameter does not match the arena's active handle. Well-behaved plugins
/// schema-check the response and emit their own diagnostic; malformed
/// plugins that ignore the error simply see an empty walk and contribute no
/// diagnostics — which matches the Contract 5 §5 graceful-degradation stance.
fn ast_handle_invalid(handle: i32) -> String {
    format!("{{\"error\":\"AST-HANDLE-INVALID\",\"handle\":{}}}", handle)
}

/// Escape a string for embedding in a JSON string literal.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Render a Type back to source form for the JSON payload.
///
/// Kept intentionally minimal — plugins only need to recognise the surface
/// shapes they emit diagnostics against (frame.data cares about the field
/// type string in E026 / W030 collapsed diagnostics; frame.ui cares about
/// primitive vs. class references for onclick handler checks). This is not
/// a full pretty-printer; complex generic types render as `Object(name)`
/// class refs preferentially and fall back to Debug for anything exotic.
fn type_to_display(t: &Type) -> String {
    match t {
        Type::String => "string".to_string(),
        Type::Integer => "integer".to_string(),
        Type::Number => "number".to_string(),
        Type::Boolean => "boolean".to_string(),
        Type::Void => "void".to_string(),
        Type::Any => "any".to_string(),
        Type::Object(name) => name.clone(),
        Type::Class { name, type_args } if type_args.is_empty() => name.clone(),
        Type::Class { name, type_args } => format!(
            "{}<{}>",
            name,
            type_args
                .iter()
                .map(type_to_display)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::List(inner, _) => format!("list<{}>", type_to_display(inner)),
        other => format!("{:?}", other),
    }
}

/// Render a return-type list for functions. Void renders as `void`;
/// non-void as the type's display form.
fn return_type_display(t: &Type) -> String {
    type_to_display(t)
}

/// Extract `(line, column)` from a `SourceLocation`, defaulting to `(0, 0)`
/// when the AST node has no source anchor.
fn loc_line(loc: &Option<crate::ast::SourceLocation>) -> (u32, u32) {
    match loc {
        Some(l) => (l.line as u32, l.column as u32),
        None => (0, 0),
    }
}

/// Extract the file path from a `SourceLocation` when present.
fn loc_file(loc: &Option<crate::ast::SourceLocation>) -> String {
    match loc {
        Some(l) => l.file.clone(),
        None => String::new(),
    }
}

/// Single-call arena. Constructed once per `lint_project` invocation.
pub(crate) struct LintArena {
    /// Monotonic handle assigned to this arena instance. Every accessor
    /// validates that the caller's handle matches this value.
    handle: i32,

    /// Snapshot of the fully-resolved `Program` at the moment lint runs.
    ///
    /// Owned (cloned) rather than borrowed because `PluginState` cannot
    /// carry a non-`'static` reference across the wasmtime Store boundary
    /// without an unsafe `'static` cast. A Program clone is a few hundred
    /// KB for realistic apps — negligible for a single lint pass.
    program: Program,
}

impl LintArena {
    /// Construct a fresh arena for a single lint_project call.
    ///
    /// `handle` is a monotonic counter maintained by the caller (analogous
    /// to typed_emission's ctx_handle) so that plugins cannot accidentally
    /// reuse a stale value from a prior call, even if the wasmtime Store
    /// were somehow shared across calls.
    pub fn new(handle: i32, program: Program) -> Self {
        Self { handle, program }
    }

    /// The monotonic handle for this call. Bridges validate the caller-
    /// supplied handle against this before returning any data.
    pub fn handle(&self) -> i32 {
        self.handle
    }

    /// Check whether `caller_handle` matches this arena's handle. Bridges
    /// short-circuit to `ast_handle_invalid(caller_handle)` on mismatch.
    fn check_handle(&self, caller_handle: i32) -> bool {
        caller_handle == self.handle
    }

    // ─────────────────────────────────────────────────────────────────────
    // Accessor 1 — _ast_list_classes(handle) -> JSON array of classes
    // ─────────────────────────────────────────────────────────────────────

    /// Return the JSON array of classes in the project:
    /// `[{ "name": "...", "file": "...", "line": N }, ...]`
    pub fn list_classes_json(&self, caller_handle: i32) -> String {
        if !self.check_handle(caller_handle) {
            return ast_handle_invalid(caller_handle);
        }
        let mut out = String::from("[");
        for (i, class) in self.program.classes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let (line, _col) = loc_line(&class.location);
            let file = loc_file(&class.location);
            out.push_str(&format!(
                "{{\"name\":\"{}\",\"file\":\"{}\",\"line\":{}}}",
                json_escape(&class.name),
                json_escape(&file),
                line
            ));
        }
        out.push(']');
        out
    }

    // ─────────────────────────────────────────────────────────────────────
    // Accessor 2 — _ast_class_fields(handle, name) -> JSON array of fields
    // ─────────────────────────────────────────────────────────────────────

    /// Return the JSON array of fields for the named class:
    /// `[{ "name": "...", "type": "...", "line": N }, ...]`
    ///
    /// Unknown class name returns `[]` — a plugin asking about a class the
    /// project doesn't have simply gets no fields, matching the "empty
    /// walk" convention. This is distinct from a stale-handle error.
    pub fn class_fields_json(&self, caller_handle: i32, class_name: &str) -> String {
        if !self.check_handle(caller_handle) {
            return ast_handle_invalid(caller_handle);
        }
        let class = match self.program.classes.iter().find(|c| c.name == class_name) {
            Some(c) => c,
            None => return "[]".to_string(),
        };
        let mut out = String::from("[");
        for (i, field) in class.fields.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            // Fields don't carry independent source locations in this AST;
            // fall back to the class's location line so plugins get *some*
            // anchor to attach a diagnostic to.
            let (line, _col) = loc_line(&class.location);
            out.push_str(&format!(
                "{{\"name\":\"{}\",\"type\":\"{}\",\"line\":{}}}",
                json_escape(&field.name),
                json_escape(&type_to_display(&field.type_)),
                line
            ));
        }
        out.push(']');
        out
    }

    // ─────────────────────────────────────────────────────────────────────
    // Accessor 3 — _ast_list_functions(handle) -> JSON array of functions
    // ─────────────────────────────────────────────────────────────────────

    /// Return the JSON array of top-level functions in the project:
    /// `[{ "name": "...", "file": "...", "line": N, "return_type": "..." }, ...]`
    ///
    /// `return_type` is included because FRAME-UI-C005 needs it to check
    /// whether `onclick="funcName"` names an actual exported handler (a
    /// callable with the right shape). The spec §4 shape mentions only
    /// name/file/line; `return_type` is an additive field — plugins that
    /// don't need it ignore it, plugins that need it get it without a
    /// second accessor call.
    pub fn list_functions_json(&self, caller_handle: i32) -> String {
        if !self.check_handle(caller_handle) {
            return ast_handle_invalid(caller_handle);
        }
        let mut out = String::from("[");
        let mut first = true;
        for func in &self.program.functions {
            if !first {
                out.push(',');
            }
            first = false;
            let (line, _col) = loc_line(&func.location);
            let file = loc_file(&func.location);
            out.push_str(&format!(
                "{{\"name\":\"{}\",\"file\":\"{}\",\"line\":{},\"return_type\":\"{}\"}}",
                json_escape(&func.name),
                json_escape(&file),
                line,
                json_escape(&return_type_display(&func.return_type))
            ));
        }
        out.push(']');
        out
    }

    // ─────────────────────────────────────────────────────────────────────
    // Accessor 4 — _ast_list_blocks(handle, name) -> JSON array of blocks
    // ─────────────────────────────────────────────────────────────────────

    /// Return the JSON array of `FrameworkBlock` occurrences whose `name`
    /// matches the requested block kind:
    /// `[{ "file": "...", "line": N, "content": "..." }, ...]`
    ///
    /// Blocks live in `Program.statements` (top-level) as
    /// `Statement::FrameworkBlock`. Inside classes/functions is possible
    /// via nested blocks; for Phase B we scan top-level statements only —
    /// the 4 residual diagnostics (frame.canvas save/restore matching,
    /// FRAME-UI-C005 onclick handler check) all trigger on top-level DSL
    /// blocks. Nested-block visibility can be added additively without a
    /// contract change.
    pub fn list_blocks_json(&self, caller_handle: i32, block_name: &str) -> String {
        if !self.check_handle(caller_handle) {
            return ast_handle_invalid(caller_handle);
        }
        let mut out = String::from("[");
        let mut first = true;
        for stmt in &self.program.statements {
            if let Statement::FrameworkBlock {
                name,
                content,
                attributes,
                location,
            } = stmt
            {
                if name == block_name {
                    if !first {
                        out.push(',');
                    }
                    first = false;
                    let (line, _col) = loc_line(location);
                    let file = loc_file(location);
                    let mut attrs_json = String::from("{");
                    for (ai, attr) in attributes.iter().enumerate() {
                        if ai > 0 {
                            attrs_json.push(',');
                        }
                        attrs_json.push_str(&format!(
                            "\"{}\":\"{}\"",
                            json_escape(&attr.name),
                            json_escape(attr.value.as_deref().unwrap_or(""))
                        ));
                    }
                    attrs_json.push('}');
                    out.push_str(&format!(
                        "{{\"file\":\"{}\",\"line\":{},\"content\":\"{}\",\"attributes\":{}}}",
                        json_escape(&file),
                        line,
                        json_escape(content),
                        attrs_json
                    ));
                }
            }
        }
        out.push(']');
        out
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Class, Field, FrameworkAttribute, Function, Parameter, SourceLocation, Type};

    fn loc(line: u32, file: &str) -> SourceLocation {
        SourceLocation::new(line as usize, 1, file)
    }

    fn empty_program() -> Program {
        Program::new(None)
    }

    // ── constructor + handle getter ──────────────────────────────────────────

    #[test]
    fn arena_handle_is_stable() {
        // Handle must survive round-trip through .handle() and stay stable
        // across repeated reads. `0` is reserved for future invalid-sentinel
        // use, so we pick a positive value.
        let arena = LintArena::new(17, empty_program());
        assert_ne!(arena.handle(), 0, "0 is reserved as invalid sentinel");
        assert_eq!(arena.handle(), 17);
        assert_eq!(
            arena.handle(),
            arena.handle(),
            "handle must not mutate on read"
        );
    }

    #[test]
    fn arena_validates_own_handle() {
        // The arena accepts its own handle on every accessor and rejects a
        // near-neighbour bogus handle. This locks down the check_handle
        // contract even though production code paths don't call .handle()
        // directly yet (they will once Phase C plugin work lands).
        let arena = LintArena::new(31, empty_program());
        let good = arena.handle();
        let bad = good.wrapping_add(1);

        assert_eq!(arena.list_classes_json(good), "[]");
        assert_eq!(arena.list_functions_json(good), "[]");
        assert_eq!(arena.class_fields_json(good, "X"), "[]");
        assert_eq!(arena.list_blocks_json(good, "data"), "[]");

        assert!(arena.list_classes_json(bad).contains("AST-HANDLE-INVALID"));
        assert!(arena
            .list_functions_json(bad)
            .contains("AST-HANDLE-INVALID"));
        assert!(arena
            .class_fields_json(bad, "X")
            .contains("AST-HANDLE-INVALID"));
        assert!(arena
            .list_blocks_json(bad, "data")
            .contains("AST-HANDLE-INVALID"));
    }

    #[test]
    fn arena_preserves_program_snapshot() {
        // Constructor invariant: the arena holds the program it was given,
        // not an empty one — regression guard for a future refactor that
        // might accidentally drop the snapshot.
        let mut prog = empty_program();
        prog.classes.push(Class::new(
            "Sentinel".to_string(),
            Some(loc(1, "sentinel.cln")),
        ));
        let arena = LintArena::new(2, prog);
        assert!(arena.list_classes_json(2).contains("\"name\":\"Sentinel\""));
    }

    // ── stale-handle detection ───────────────────────────────────────────────

    #[test]
    fn stale_handle_returns_ast_handle_invalid() {
        let arena = LintArena::new(42, empty_program());
        // Wrong handle on each accessor returns the AST-HANDLE-INVALID payload.
        let out = arena.list_classes_json(99);
        assert!(out.contains("AST-HANDLE-INVALID"));
        assert!(out.contains("\"handle\":99"));
        assert!(arena
            .class_fields_json(99, "Order")
            .contains("AST-HANDLE-INVALID"));
        assert!(arena.list_functions_json(99).contains("AST-HANDLE-INVALID"));
        assert!(arena
            .list_blocks_json(99, "data")
            .contains("AST-HANDLE-INVALID"));
    }

    #[test]
    fn correct_handle_returns_empty_array_for_empty_program() {
        let arena = LintArena::new(1, empty_program());
        assert_eq!(arena.list_classes_json(1), "[]");
        assert_eq!(arena.list_functions_json(1), "[]");
        assert_eq!(arena.class_fields_json(1, "Anything"), "[]");
        assert_eq!(arena.list_blocks_json(1, "data"), "[]");
    }

    // ── list_classes ─────────────────────────────────────────────────────────

    #[test]
    fn list_classes_emits_name_file_line() {
        let mut prog = empty_program();
        let mut c = Class::new("Order".to_string(), Some(loc(47, "app/orders.cln")));
        c.fields.push(Field::new("id".to_string(), Type::Integer));
        prog.classes.push(c);
        prog.classes.push(Class::new(
            "Customer".to_string(),
            Some(loc(101, "app/customers.cln")),
        ));

        let arena = LintArena::new(7, prog);
        let out = arena.list_classes_json(7);
        assert!(out.starts_with('['));
        assert!(out.ends_with(']'));
        assert!(out.contains("\"name\":\"Order\""));
        assert!(out.contains("\"file\":\"app/orders.cln\""));
        assert!(out.contains("\"line\":47"));
        assert!(out.contains("\"name\":\"Customer\""));
        assert!(out.contains("\"line\":101"));
    }

    // ── class_fields ─────────────────────────────────────────────────────────

    #[test]
    fn class_fields_emits_name_type_line() {
        let mut prog = empty_program();
        let mut c = Class::new("Order".to_string(), Some(loc(47, "app/orders.cln")));
        c.fields.push(Field::new("id".to_string(), Type::Integer));
        c.fields.push(Field::new("total".to_string(), Type::Number));
        c.fields.push(Field::new(
            "customer".to_string(),
            Type::Object("Customer".to_string()),
        ));
        prog.classes.push(c);

        let arena = LintArena::new(3, prog);
        let out = arena.class_fields_json(3, "Order");
        assert!(out.contains("\"name\":\"id\""));
        assert!(out.contains("\"type\":\"integer\""));
        assert!(out.contains("\"name\":\"total\""));
        assert!(out.contains("\"type\":\"number\""));
        assert!(out.contains("\"name\":\"customer\""));
        assert!(out.contains("\"type\":\"Customer\""));
        assert!(out.contains("\"line\":47"));
    }

    #[test]
    fn class_fields_unknown_class_returns_empty_array() {
        let arena = LintArena::new(1, empty_program());
        assert_eq!(arena.class_fields_json(1, "DoesNotExist"), "[]");
    }

    // ── list_functions ───────────────────────────────────────────────────────

    #[test]
    fn list_functions_emits_name_return_type() {
        let mut prog = empty_program();
        prog.functions.push(Function::new(
            "sendEmail".to_string(),
            vec![Parameter::new("to".to_string(), Type::String)],
            Type::Boolean,
            Vec::new(),
            Some(loc(12, "app/notify.cln")),
        ));
        prog.functions.push(Function::new(
            "recompute".to_string(),
            Vec::new(),
            Type::Void,
            Vec::new(),
            Some(loc(88, "app/notify.cln")),
        ));

        let arena = LintArena::new(5, prog);
        let out = arena.list_functions_json(5);
        assert!(out.contains("\"name\":\"sendEmail\""));
        assert!(out.contains("\"return_type\":\"boolean\""));
        assert!(out.contains("\"line\":12"));
        assert!(out.contains("\"name\":\"recompute\""));
        assert!(out.contains("\"return_type\":\"void\""));
        assert!(out.contains("\"line\":88"));
    }

    // ── list_blocks ──────────────────────────────────────────────────────────

    #[test]
    fn list_blocks_filters_by_name() {
        let mut prog = empty_program();
        prog.statements.push(Statement::FrameworkBlock {
            name: "data".to_string(),
            content: "class User { id: integer }".to_string(),
            attributes: Vec::<FrameworkAttribute>::new(),
            location: Some(loc(5, "app/schema.cln")),
        });
        prog.statements.push(Statement::FrameworkBlock {
            name: "endpoints".to_string(),
            content: "get /health\n\treturn 200".to_string(),
            attributes: Vec::<FrameworkAttribute>::new(),
            location: Some(loc(20, "app/api.cln")),
        });
        prog.statements.push(Statement::FrameworkBlock {
            name: "data".to_string(),
            content: "class Order { id: integer }".to_string(),
            attributes: Vec::<FrameworkAttribute>::new(),
            location: Some(loc(50, "app/orders.cln")),
        });

        let arena = LintArena::new(9, prog);
        let out = arena.list_blocks_json(9, "data");
        // Should contain both `data` blocks and neither the `endpoints` one.
        assert!(out.contains("\"content\":\"class User { id: integer }\""));
        assert!(out.contains("\"content\":\"class Order { id: integer }\""));
        assert!(!out.contains("endpoints"));
        assert!(!out.contains("/health"));
        assert!(out.contains("\"line\":5"));
        assert!(out.contains("\"line\":50"));
    }

    #[test]
    fn list_blocks_escapes_special_chars_in_content() {
        let mut prog = empty_program();
        prog.statements.push(Statement::FrameworkBlock {
            name: "html".to_string(),
            content: "<div class=\"foo\">\n\thello</div>".to_string(),
            attributes: Vec::<FrameworkAttribute>::new(),
            location: Some(loc(1, "app/page.cln")),
        });

        let arena = LintArena::new(11, prog);
        let out = arena.list_blocks_json(11, "html");
        // Quotes and newlines escaped correctly.
        assert!(out.contains("\\\"foo\\\""));
        assert!(out.contains("\\n"));
        assert!(out.contains("\\t"));
    }

    #[test]
    fn list_blocks_exposes_model_name_attribute() {
        let mut prog = empty_program();
        prog.statements.push(Statement::FrameworkBlock {
            name: "data".to_string(),
            content: "    name string\n    age integer\n".to_string(),
            attributes: vec![FrameworkAttribute {
                name: "model_name".to_string(),
                value: Some("Todo".to_string()),
                location: None,
            }],
            location: Some(loc(12, "app/data/models/todo.cln")),
        });

        let arena = LintArena::new(13, prog);
        let out = arena.list_blocks_json(13, "data");
        assert!(
            out.contains("\"attributes\":{\"model_name\":\"Todo\"}"),
            "expected model_name attribute in JSON, got: {}",
            out
        );
    }

    #[test]
    fn list_blocks_empty_attributes_serializes_empty_object() {
        let mut prog = empty_program();
        prog.statements.push(Statement::FrameworkBlock {
            name: "data".to_string(),
            content: "field integer\n".to_string(),
            attributes: Vec::<FrameworkAttribute>::new(),
            location: Some(loc(1, "app/x.cln")),
        });

        let arena = LintArena::new(14, prog);
        let out = arena.list_blocks_json(14, "data");
        assert!(
            out.contains("\"attributes\":{}"),
            "expected empty attributes object, got: {}",
            out
        );
    }

    #[test]
    fn list_blocks_flag_attribute_serializes_empty_value() {
        let mut prog = empty_program();
        prog.statements.push(Statement::FrameworkBlock {
            name: "data".to_string(),
            content: String::new(),
            attributes: vec![FrameworkAttribute {
                name: "readonly".to_string(),
                value: None,
                location: None,
            }],
            location: Some(loc(2, "app/y.cln")),
        });

        let arena = LintArena::new(15, prog);
        let out = arena.list_blocks_json(15, "data");
        assert!(
            out.contains("\"attributes\":{\"readonly\":\"\"}"),
            "flag attribute should serialize with empty-string value, got: {}",
            out
        );
    }

    #[test]
    fn json_escape_handles_control_chars() {
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\rb"), "a\\rb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
        assert_eq!(json_escape("a\x01b"), "a\\u0001b");
    }

    #[test]
    fn type_to_display_renders_class_as_bare_name() {
        // Regression guard: framework hit false-positive FRAME-DATA-I002 on
        // v2.12.180 because Type::Class fell through to the `{:?}` fallback,
        // producing `"Class { name: \"UserData\", type_args: [] }"` instead
        // of the bare name. Prompt 4e60be0f-86c8-11f1-9d55-da25a95a496b.
        let t = Type::Class {
            name: "UserData".to_string(),
            type_args: vec![],
        };
        assert_eq!(type_to_display(&t), "UserData");
    }

    #[test]
    fn type_to_display_renders_generic_class_with_angle_brackets() {
        let t = Type::Class {
            name: "Box".to_string(),
            type_args: vec![Type::String],
        };
        assert_eq!(type_to_display(&t), "Box<string>");

        let t = Type::Class {
            name: "Map".to_string(),
            type_args: vec![Type::String, Type::Integer],
        };
        assert_eq!(type_to_display(&t), "Map<string, integer>");
    }
}
