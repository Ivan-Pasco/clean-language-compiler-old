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
    /// matches the requested block kind, walking the WHOLE program: top-level
    /// statements, top-level function bodies, class methods, class
    /// constructors, and any control-flow-nested statements within those.
    ///
    /// Each entry carries a `parent_context` field so plugins can anchor
    /// diagnostics to the enclosing function/method:
    ///
    /// ```json
    /// {
    ///   "file": "...",
    ///   "line": N,
    ///   "content": "...",
    ///   "attributes": {...},
    ///   "parent_context": {"kind": "top-level", "name": null, "class": null}
    ///     // OR {"kind": "function", "name": "render", "class": null}
    ///     // OR {"kind": "method", "name": "render", "class": "HomePage"}
    ///     // OR {"kind": "constructor", "name": null, "class": "HomePage"}
    /// }
    /// ```
    ///
    /// **Contract 5 §4 amendment (prompt 5258173d).** Prior versions walked
    /// `Program.statements` only, which returned `[]` for every realistic
    /// frame.ui project because `html:` blocks live inside function bodies
    /// (frame.ui: "html: block at the end of a function is the implicit
    /// return value" — primary usage pattern). FRAME-UI-C005 (onclick handler
    /// resolution) is the motivating diagnostic. Full recursion + parent
    /// context is additive: plugins that ignore `parent_context` continue to
    /// work, plugins that need it can anchor accurately.
    pub fn list_blocks_json(&self, caller_handle: i32, block_name: &str) -> String {
        if !self.check_handle(caller_handle) {
            return ast_handle_invalid(caller_handle);
        }
        let mut out = String::from("[");
        let mut first = true;

        // Top-level statements — parent_context = top-level.
        collect_framework_blocks(
            &self.program.statements,
            block_name,
            &BlockContext::TopLevel,
            &mut out,
            &mut first,
        );

        // Top-level (free) functions — parent_context = function:name.
        for func in &self.program.functions {
            let ctx = BlockContext::Function {
                name: func.name.clone(),
            };
            collect_framework_blocks(&func.body, block_name, &ctx, &mut out, &mut first);
        }

        // Classes — walk constructor + every method body.
        for class in &self.program.classes {
            if let Some(ctor) = &class.constructor {
                let ctx = BlockContext::Constructor {
                    class: class.name.clone(),
                };
                collect_framework_blocks(&ctor.body, block_name, &ctx, &mut out, &mut first);
            }
            for method in &class.methods {
                let ctx = BlockContext::Method {
                    class: class.name.clone(),
                    name: method.name.clone(),
                };
                collect_framework_blocks(&method.body, block_name, &ctx, &mut out, &mut first);
            }
        }

        out.push(']');
        out
    }

    // ─────────────────────────────────────────────────────────────────────
    // Accessor 5 — _ast_block_subblocks(handle, name) -> nested JSON array
    // ─────────────────────────────────────────────────────────────────────

    /// Like `list_blocks_json` but each outer entry additionally carries a
    /// `children` field: a recursively-parsed indentation tree of sub-blocks.
    ///
    /// A **sub-block** is a line at the current indent level whose trimmed
    /// text ends in `:` **and** has nothing after the `:` on the same line.
    /// Everything else at that indent level accumulates into the enclosing
    /// block's `content` string. Depth-limited to 8 to guard pathological
    /// input; realistic plugin DSLs stay under 4.
    ///
    /// Content strings are dedented to level 0 — the plugin sees
    /// `"email string\nbio string"`, not `"\t\t\temail string\n\t\t\tbio string"`.
    /// This differs from `list_blocks_json`, where `content` is verbatim.
    /// See `foundation/spec/framework/contracts/lint-extension.md` §4.5.
    pub fn block_subblocks_json(&self, caller_handle: i32, block_name: &str) -> String {
        if !self.check_handle(caller_handle) {
            return ast_handle_invalid(caller_handle);
        }
        let mut out = String::from("[");
        let mut first = true;

        collect_framework_blocks_subblocks(
            &self.program.statements,
            block_name,
            &BlockContext::TopLevel,
            &mut out,
            &mut first,
        );

        for func in &self.program.functions {
            let ctx = BlockContext::Function {
                name: func.name.clone(),
            };
            collect_framework_blocks_subblocks(&func.body, block_name, &ctx, &mut out, &mut first);
        }

        for class in &self.program.classes {
            if let Some(ctor) = &class.constructor {
                let ctx = BlockContext::Constructor {
                    class: class.name.clone(),
                };
                collect_framework_blocks_subblocks(
                    &ctor.body, block_name, &ctx, &mut out, &mut first,
                );
            }
            for method in &class.methods {
                let ctx = BlockContext::Method {
                    class: class.name.clone(),
                    name: method.name.clone(),
                };
                collect_framework_blocks_subblocks(
                    &method.body,
                    block_name,
                    &ctx,
                    &mut out,
                    &mut first,
                );
            }
        }

        out.push(']');
        out
    }
}

/// Parent-context tag emitted alongside each `_ast_list_blocks` entry.
///
/// Rendered into the JSON payload's `parent_context` field. Plugins that
/// need to anchor a diagnostic to the enclosing function (e.g. FRAME-UI-C005
/// reporting the line of the `render()` that contains a bad-onclick html
/// block) read the `name`/`class` fields; plugins that don't care can
/// ignore the whole `parent_context` object.
enum BlockContext {
    TopLevel,
    Function { name: String },
    Method { class: String, name: String },
    Constructor { class: String },
}

impl BlockContext {
    fn to_json(&self) -> String {
        match self {
            BlockContext::TopLevel => {
                "{\"kind\":\"top-level\",\"name\":null,\"class\":null}".to_string()
            }
            BlockContext::Function { name } => format!(
                "{{\"kind\":\"function\",\"name\":\"{}\",\"class\":null}}",
                json_escape(name)
            ),
            BlockContext::Method { class, name } => format!(
                "{{\"kind\":\"method\",\"name\":\"{}\",\"class\":\"{}\"}}",
                json_escape(name),
                json_escape(class)
            ),
            BlockContext::Constructor { class } => format!(
                "{{\"kind\":\"constructor\",\"name\":null,\"class\":\"{}\"}}",
                json_escape(class)
            ),
        }
    }
}

/// Recursively walk a `Vec<Statement>` and append every `FrameworkBlock`
/// whose `name == block_name` to `out` as a JSON object. Descends into
/// every statement variant that owns a `Vec<Statement>` (if/else, while,
/// iterate, for-range, onError, background) so a plugin-visible block
/// buried inside a loop or branch is still discovered.
///
/// `first` is a running "have we emitted anything yet?" flag so the caller
/// can chain multiple walks (top-level → function bodies → class methods)
/// into one JSON array with correct comma separation.
fn collect_framework_blocks(
    stmts: &[Statement],
    block_name: &str,
    parent: &BlockContext,
    out: &mut String,
    first: &mut bool,
) {
    for stmt in stmts {
        collect_framework_blocks_in_stmt(stmt, block_name, parent, out, first);
    }
}

fn collect_framework_blocks_in_stmt(
    stmt: &Statement,
    block_name: &str,
    parent: &BlockContext,
    out: &mut String,
    first: &mut bool,
) {
    match stmt {
        Statement::FrameworkBlock {
            name,
            content,
            attributes,
            location,
        } => {
            if name == block_name {
                if !*first {
                    out.push(',');
                }
                *first = false;
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
                    "{{\"file\":\"{}\",\"line\":{},\"content\":\"{}\",\"attributes\":{},\"parent_context\":{}}}",
                    json_escape(&file),
                    line,
                    json_escape(content),
                    attrs_json,
                    parent.to_json(),
                ));
            }
        }
        Statement::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_framework_blocks(then_branch, block_name, parent, out, first);
            if let Some(else_b) = else_branch {
                collect_framework_blocks(else_b, block_name, parent, out, first);
            }
        }
        Statement::While { body, .. } => {
            collect_framework_blocks(body, block_name, parent, out, first);
        }
        Statement::Iterate { body, .. } => {
            collect_framework_blocks(body, block_name, parent, out, first);
        }
        Statement::RangeIterate { body, .. } => {
            collect_framework_blocks(body, block_name, parent, out, first);
        }
        Statement::StandaloneErrorHandler { body, .. } => {
            collect_framework_blocks(body, block_name, parent, out, first);
        }
        _ => {}
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Accessor 5 helpers — sub-block indentation walker
// ────────────────────────────────────────────────────────────────────────────

/// Max sub-block nesting depth. Realistic DSLs stay under 4; 8 is a comfortable
/// safety margin against pathological input while keeping recursion bounded.
const MAX_SUBBLOCK_DEPTH: u8 = 8;

/// One parsed sub-block entry — mirrors the JSON shape produced by
/// `block_subblocks_json`. Kept private to this module; the accessor emits
/// JSON directly rather than returning this type across a bridge boundary.
struct SubBlock {
    name: String,
    line: u32,
    content: String,
    children: Vec<SubBlock>,
}

/// Parallel walker to `collect_framework_blocks` that emits per-block with
/// a `children` field carrying the parsed indentation tree.
fn collect_framework_blocks_subblocks(
    stmts: &[Statement],
    block_name: &str,
    parent: &BlockContext,
    out: &mut String,
    first: &mut bool,
) {
    for stmt in stmts {
        collect_framework_blocks_subblocks_in_stmt(stmt, block_name, parent, out, first);
    }
}

fn collect_framework_blocks_subblocks_in_stmt(
    stmt: &Statement,
    block_name: &str,
    parent: &BlockContext,
    out: &mut String,
    first: &mut bool,
) {
    match stmt {
        Statement::FrameworkBlock {
            name,
            content,
            location,
            ..
        } => {
            if name == block_name {
                if !*first {
                    out.push(',');
                }
                *first = false;
                let (line, _col) = loc_line(location);
                let file = loc_file(location);
                let children = parse_subblocks(content, line, MAX_SUBBLOCK_DEPTH);
                let mut children_json = String::from("[");
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        children_json.push(',');
                    }
                    emit_subblock_json(child, &mut children_json);
                }
                children_json.push(']');
                out.push_str(&format!(
                    "{{\"file\":\"{}\",\"line\":{},\"parent_context\":{},\"children\":{}}}",
                    json_escape(&file),
                    line,
                    parent.to_json(),
                    children_json,
                ));
            }
        }
        Statement::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_framework_blocks_subblocks(then_branch, block_name, parent, out, first);
            if let Some(else_b) = else_branch {
                collect_framework_blocks_subblocks(else_b, block_name, parent, out, first);
            }
        }
        Statement::While { body, .. } => {
            collect_framework_blocks_subblocks(body, block_name, parent, out, first);
        }
        Statement::Iterate { body, .. } => {
            collect_framework_blocks_subblocks(body, block_name, parent, out, first);
        }
        Statement::RangeIterate { body, .. } => {
            collect_framework_blocks_subblocks(body, block_name, parent, out, first);
        }
        Statement::StandaloneErrorHandler { body, .. } => {
            collect_framework_blocks_subblocks(body, block_name, parent, out, first);
        }
        _ => {}
    }
}

fn emit_subblock_json(sb: &SubBlock, out: &mut String) {
    out.push_str(&format!(
        "{{\"name\":\"{}\",\"line\":{},\"content\":\"{}\",\"children\":[",
        json_escape(&sb.name),
        sb.line,
        json_escape(&sb.content)
    ));
    for (i, child) in sb.children.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        emit_subblock_json(child, out);
    }
    out.push_str("]}");
}

/// Count leading spaces/tabs on a line. Tab = 1 unit (Clean uses tab
/// indentation exclusively; the parser upstream would have already rejected
/// mixed tab/space).
fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == '\t' || *c == ' ').count()
}

/// True when this line looks like a sub-block header: after trimming, the
/// last character is `:` and there is nothing between the last `:` and the
/// end. `key: value` is NOT a header. `fields:` IS. `foo: ` (trailing space)
/// IS a header (whitespace after `:` doesn't count as content).
fn is_header_line(stripped: &str) -> bool {
    let trimmed = stripped.trim_end();
    trimmed.ends_with(':') && trimmed.len() > 1
}

/// Extract the sub-block name from a header line: everything before the
/// trailing `:`, whitespace-trimmed. `UserData:` → `"UserData"`, `  fields:`
/// (already stripped by caller) → `"fields"`.
fn header_name(stripped: &str) -> String {
    let trimmed = stripped.trim_end();
    trimmed[..trimmed.len() - 1].trim().to_string()
}

/// Parse the indentation tree from a block's raw `content` string.
///
/// `base_line` is the source line of the enclosing block's header — sub-block
/// `line` numbers are computed relative to this.
///
/// Algorithm: scan lines in order. For each non-blank line at the outermost
/// indent found in `content`, if it's a header, consume all subsequent
/// deeper-indented lines as its body and recurse; otherwise, add it to the
/// enclosing block's `content` (handled by our caller, since we don't emit
/// that from here — we only produce SubBlocks). Blank lines are skipped and
/// do not terminate a sub-block's body.
fn parse_subblocks(content: &str, base_line: u32, max_depth: u8) -> Vec<SubBlock> {
    if max_depth == 0 {
        return Vec::new();
    }
    let lines: Vec<&str> = content.split('\n').collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Determine the "base indent" of THIS scope — the smallest indent among
    // non-blank lines. Anything at exactly this level is a candidate for a
    // sub-block header or leaf; deeper is body of the last-emitted sub-block.
    let base_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .min()
        .unwrap_or(0);

    let mut result: Vec<SubBlock> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let stripped = line.trim_start();

        if stripped.is_empty() {
            i += 1;
            continue;
        }

        let cur_indent = indent_of(line);
        // Only consider lines at the base indent as candidates for headers.
        // Anything deeper belongs to a previously-opened sub-block's body and
        // is consumed by the recursive call below.
        if cur_indent > base_indent {
            i += 1;
            continue;
        }

        if is_header_line(stripped) {
            let header_line = base_line + i as u32;
            let name = header_name(stripped);

            // Body: lines below this header whose indent > base_indent.
            // Stop at the next line whose indent <= base_indent AND is not blank.
            let body_start = i + 1;
            let mut j = body_start;
            while j < lines.len() {
                let lj = lines[j];
                if lj.trim().is_empty() {
                    j += 1;
                    continue;
                }
                if indent_of(lj) <= base_indent {
                    break;
                }
                j += 1;
            }

            let body_lines = &lines[body_start..j];
            let (body_content, body_children) =
                partition_body(body_lines, base_line + body_start as u32, max_depth - 1);

            result.push(SubBlock {
                name,
                line: header_line,
                content: body_content,
                children: body_children,
            });
            i = j;
        } else {
            // Non-header line at base indent — this belongs to the ENCLOSING
            // block's content. From THIS function's POV, we ignore it (our
            // caller is responsible for populating its own `content` from
            // its body). This branch only fires when a sub-block scope has
            // mixed leaf-lines-at-base-indent + header-lines-at-base-indent.
            //
            // For the OUTER call (from `block_subblocks_json` on a raw
            // `content` string), leaves at base indent would be silently
            // dropped. That's the intended contract: the plugin's outer
            // block already has its raw `content` via `_ast_list_blocks`.
            // `_ast_block_subblocks` is specifically for STRUCTURED nesting.
            i += 1;
        }
    }

    result
}

/// Given a set of body lines (raw, still-indented), split them into the
/// `content` string (dedented leaves) and the `children` (recursively parsed
/// sub-blocks). Called after `parse_subblocks` has identified the body span.
fn partition_body(body_lines: &[&str], base_line: u32, max_depth: u8) -> (String, Vec<SubBlock>) {
    if body_lines.is_empty() {
        return (String::new(), Vec::new());
    }

    // Determine the minimum non-blank indent of the body — this is the "level
    // 0" for content dedent, and the "base indent" for our recursive call.
    let min_indent = body_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .min()
        .unwrap_or(0);

    // Rebuild body content dedented to level 0. Blank lines preserved as
    // empty strings so plugins that split on \n see line-preserving text.
    let mut dedented_lines: Vec<String> = Vec::with_capacity(body_lines.len());
    for l in body_lines {
        if l.trim().is_empty() {
            dedented_lines.push(String::new());
        } else {
            // Only strip up to `min_indent` chars; deeper lines keep their
            // relative indentation so recursive parsing works.
            let strip = min_indent.min(indent_of(l));
            dedented_lines.push(l[strip..].to_string());
        }
    }
    let dedented_body: String = dedented_lines.join("\n");

    // Recursively find sub-blocks WITHIN the body.
    let children = parse_subblocks(&dedented_body, base_line, max_depth);

    // Build `content`: non-header, non-blank lines at level 0 of the dedented
    // body, joined with \n. Everything deeper belongs to a child.
    let mut content_lines: Vec<&str> = Vec::new();
    for l in dedented_lines.iter() {
        if l.trim().is_empty() {
            continue;
        }
        if indent_of(l) > 0 {
            continue; // belongs to a child's body
        }
        if is_header_line(l.trim_start()) {
            continue; // is a header of a child
        }
        content_lines.push(l);
    }
    let content = content_lines.join("\n");

    (content, children)
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

    // ── list_blocks recursion + parent_context (prompt 5258173d) ─────────────

    fn html_block(line: u32, content: &str) -> Statement {
        Statement::FrameworkBlock {
            name: "html".to_string(),
            content: content.to_string(),
            attributes: Vec::<FrameworkAttribute>::new(),
            location: Some(loc(line, "app/pages/home.cln")),
        }
    }

    fn empty_function(name: &str, body: Vec<Statement>) -> Function {
        Function {
            name: name.to_string(),
            type_parameters: vec![],
            type_constraints: vec![],
            parameters: vec![],
            return_type: Type::Void,
            body,
            description: None,
            syntax: crate::ast::FunctionSyntax::Simple,
            visibility: crate::ast::Visibility::Public,
            modifier: crate::ast::FunctionModifier::None,
            location: Some(loc(1, "app/pages/home.cln")),
        }
    }

    #[test]
    fn list_blocks_top_level_still_reports_parent_context_top_level() {
        // Backward-compat: a top-level block gets the new parent_context
        // tag set to top-level, so existing plugins still see their block
        // AND now can ignore/read parent_context.
        let mut prog = empty_program();
        prog.statements.push(html_block(3, "<h1>hello</h1>"));

        let arena = LintArena::new(20, prog);
        let out = arena.list_blocks_json(20, "html");
        assert!(out.contains("\"content\":\"<h1>hello</h1>\""));
        assert!(
            out.contains(
                "\"parent_context\":{\"kind\":\"top-level\",\"name\":null,\"class\":null}"
            ),
            "top-level block missing/wrong parent_context: {}",
            out
        );
    }

    #[test]
    fn list_blocks_recurses_into_top_level_function_bodies() {
        // FRAME-UI-C005 motivating case: html: block lives inside render(),
        // not at the top level. Prior versions returned []. Must now return
        // the block with parent_context.kind = "function".
        let mut prog = empty_program();
        prog.functions.push(empty_function(
            "render",
            vec![html_block(47, "<button onclick=\"greet\">Hi</button>")],
        ));

        let arena = LintArena::new(21, prog);
        let out = arena.list_blocks_json(21, "html");
        assert!(
            out.contains("greet"),
            "html block inside function body was not enumerated: {}",
            out
        );
        assert!(
            out.contains(
                "\"parent_context\":{\"kind\":\"function\",\"name\":\"render\",\"class\":null}"
            ),
            "wrong parent_context for function-body block: {}",
            out
        );
    }

    #[test]
    fn list_blocks_recurses_into_class_methods_and_constructor() {
        // Class methods and constructor bodies are the second-most common
        // location for framework blocks in real projects (component: renders,
        // etc.). Ensure both are walked and get distinct parent_context tags.
        let mut prog = empty_program();
        let mut cls = Class::new("HomePage".to_string(), Some(loc(10, "app/pages/home.cln")));
        cls.methods.push(empty_function(
            "render",
            vec![html_block(15, "<div>method-body</div>")],
        ));
        cls.constructor = Some(crate::ast::Constructor::new(
            vec![],
            vec![html_block(20, "<div>ctor-body</div>")],
            Some(loc(18, "app/pages/home.cln")),
        ));
        prog.classes.push(cls);

        let arena = LintArena::new(22, prog);
        let out = arena.list_blocks_json(22, "html");
        assert!(
            out.contains("method-body"),
            "missed class method body: {}",
            out
        );
        assert!(
            out.contains("ctor-body"),
            "missed constructor body: {}",
            out
        );
        assert!(
            out.contains("\"parent_context\":{\"kind\":\"method\",\"name\":\"render\",\"class\":\"HomePage\"}"),
            "wrong parent_context for method body: {}", out
        );
        assert!(
            out.contains("\"parent_context\":{\"kind\":\"constructor\",\"name\":null,\"class\":\"HomePage\"}"),
            "wrong parent_context for constructor body: {}", out
        );
    }

    #[test]
    fn list_blocks_recurses_through_control_flow_nesting() {
        // A block inside `if` inside `while` inside a function body still
        // gets enumerated with the correct parent_context. This is not the
        // common case but the recursion cost is trivial, and dropping it
        // would silently miss valid diagnostics.
        let mut prog = empty_program();
        let nested = Statement::While {
            condition: crate::ast::Expression::Literal(crate::ast::Value::Boolean(true)),
            body: vec![Statement::If {
                condition: crate::ast::Expression::Literal(crate::ast::Value::Boolean(true)),
                then_branch: vec![html_block(50, "<span>nested</span>")],
                else_branch: None,
                location: None,
            }],
            location: None,
        };
        prog.functions.push(empty_function("draw", vec![nested]));

        let arena = LintArena::new(23, prog);
        let out = arena.list_blocks_json(23, "html");
        assert!(
            out.contains("nested"),
            "control-flow-nested block was not enumerated: {}",
            out
        );
        assert!(
            out.contains("\"parent_context\":{\"kind\":\"function\",\"name\":\"draw\",\"class\":null}"),
            "parent context should point at the enclosing function, not the intermediate if/while: {}", out
        );
    }

    // ── block_subblocks (_ast_block_subblocks — Accessor 5) ──────────────────

    fn data_block(line: u32, content: &str) -> Statement {
        Statement::FrameworkBlock {
            name: "data".to_string(),
            content: content.to_string(),
            attributes: Vec::<FrameworkAttribute>::new(),
            location: Some(loc(line, "app/models.cln")),
        }
    }

    #[test]
    fn subblocks_stale_handle_and_empty() {
        // Contract mirror of the other accessors: bad handle → payload;
        // no matching FrameworkBlocks → empty array (NOT the invalid payload).
        let arena = LintArena::new(11, empty_program());
        assert!(arena
            .block_subblocks_json(99, "data")
            .contains("AST-HANDLE-INVALID"));
        assert_eq!(arena.block_subblocks_json(11, "data"), "[]");
    }

    #[test]
    fn subblocks_no_headers_produces_no_children() {
        // A block whose content is all leaf lines (no `foo:` headers) has
        // no children. The plugin gets a top-level entry with children:[]
        // and would use `_ast_list_blocks` if it needed the raw content.
        let mut prog = empty_program();
        prog.statements
            .push(data_block(1, "just some\nplain text\n"));
        let arena = LintArena::new(1, prog);
        let out = arena.block_subblocks_json(1, "data");
        assert!(out.contains("\"children\":[]"), "got: {}", out);
    }

    #[test]
    fn subblocks_two_level_nesting_dedents_content() {
        // The core E026-enabling shape:
        //   data:
        //       UserData:
        //           fields:
        //               email string
        //               bio string
        // The outer content string (already parsed by the compiler and stored
        // on FrameworkBlock.content) uses tabs. `parse_subblocks` sees the
        // outermost-indent lines as candidates for headers.
        let mut prog = empty_program();
        let content = "\tUserData:\n\t\tfields:\n\t\t\temail string\n\t\t\tbio string\n";
        prog.statements.push(data_block(1, content));

        let arena = LintArena::new(1, prog);
        let out = arena.block_subblocks_json(1, "data");

        // Outer FrameworkBlock entry
        assert!(
            out.contains("\"file\":\"app/models.cln\""),
            "missing file: {}",
            out
        );
        // First-level child: UserData
        assert!(
            out.contains("\"name\":\"UserData\""),
            "missing UserData: {}",
            out
        );
        // Second-level child: fields (must be inside UserData's children)
        assert!(
            out.contains("\"name\":\"fields\""),
            "missing fields: {}",
            out
        );
        // Leaf content is dedented — the leaves must NOT contain tabs
        assert!(
            out.contains("\"content\":\"email string\\nbio string\""),
            "content not dedented to level 0: {}",
            out
        );
    }

    #[test]
    fn subblocks_key_colon_value_is_not_a_header() {
        // A line like `key: value` (colon with content after it) is NOT a
        // sub-block header. It stays in the enclosing block's content string.
        // This protects DSLs that use `<attr>: <value>` at leaf level.
        let mut prog = empty_program();
        let content = "\tsomeAttr: some value\n\tanotherAttr: other value\n";
        prog.statements.push(data_block(1, content));

        let arena = LintArena::new(1, prog);
        let out = arena.block_subblocks_json(1, "data");
        // No sub-blocks named "someAttr" or "anotherAttr" — key-colon-value
        // must remain leaf content.
        assert!(
            !out.contains("\"name\":\"someAttr\""),
            "someAttr was wrongly classified as sub-block header: {}",
            out
        );
        assert!(!out.contains("\"name\":\"anotherAttr\""), "got: {}", out);
        // Children stays empty at the top level.
        assert!(out.contains("\"children\":[]"), "got: {}", out);
    }

    #[test]
    fn subblocks_mixed_headers_and_leaves_at_same_level() {
        // A DSL where an entity's body has both leaf declarations AND nested
        // headers at the same indent level. Leaves feed into the entity's
        // `content`; headers become its `children`. Neither is dropped.
        let mut prog = empty_program();
        let content =
            "\tUserData:\n\t\tid integer\n\t\tfields:\n\t\t\temail string\n\t\ttimestamp\n";
        prog.statements.push(data_block(1, content));

        let arena = LintArena::new(1, prog);
        let out = arena.block_subblocks_json(1, "data");

        // UserData exists, has a `fields:` child.
        assert!(out.contains("\"name\":\"UserData\""), "got: {}", out);
        assert!(out.contains("\"name\":\"fields\""), "got: {}", out);
        // Leaf lines `id integer` and `timestamp` appear in UserData's
        // content — dedented, joined with \n.
        assert!(out.contains("id integer"), "leaf id not preserved: {}", out);
        assert!(
            out.contains("timestamp"),
            "leaf timestamp not preserved: {}",
            out
        );
    }

    #[test]
    fn subblocks_blank_lines_do_not_terminate_a_body() {
        // Blank lines between sibling declarations must not close the
        // enclosing sub-block. Common in hand-written DSL bodies.
        let mut prog = empty_program();
        let content = "\tUserData:\n\t\tfields:\n\t\t\temail string\n\n\t\t\tbio string\n";
        prog.statements.push(data_block(1, content));

        let arena = LintArena::new(1, prog);
        let out = arena.block_subblocks_json(1, "data");
        // Both leaves land in the same fields: content, separated by an
        // empty line preserved in the dedent.
        assert!(
            out.contains("email string") && out.contains("bio string"),
            "blank line terminated the body prematurely: {}",
            out
        );
    }

    #[test]
    fn subblocks_line_numbers_are_absolute() {
        // Sub-block `line` field must be the ABSOLUTE source line of the
        // sub-block header (base_line + offset), not a relative count.
        // Verifies the base_line-plumbing through parse_subblocks + partition_body.
        let mut prog = empty_program();
        // Framework block sits at source line 42; its first line of content
        // is line 42 (header line 42 + first content offset 0). But note the
        // parser stores the block header's line as `location`, and content
        // starts at line 43. So `UserData:` (line 0 of content) = source line 42.
        // Verify the emitted line for UserData matches.
        let content = "\tUserData:\n\t\tfields:\n\t\t\temail string\n";
        prog.statements.push(data_block(42, content));

        let arena = LintArena::new(1, prog);
        let out = arena.block_subblocks_json(1, "data");
        // UserData is at line 42 (base_line = 42, offset i = 0).
        assert!(
            out.contains("\"name\":\"UserData\",\"line\":42"),
            "UserData line off: {}",
            out
        );
        // fields is at line 43.
        assert!(
            out.contains("\"name\":\"fields\",\"line\":43"),
            "fields line off: {}",
            out
        );
    }

    #[test]
    fn subblocks_depth_limit_stops_recursion() {
        // Depth 8 with 9 levels of nesting: the 9th level's header does NOT
        // become a child. This protects against pathological input driving
        // unbounded recursion. Realistic DSLs stay under 4.
        let mut prog = empty_program();
        // Build 9 levels: a: → b: → c: ... i:.
        let mut content = String::new();
        for depth in 0..9 {
            for _ in 0..(depth + 1) {
                content.push('\t');
            }
            let ch = (b'a' + depth as u8) as char;
            content.push(ch);
            content.push_str(":\n");
        }
        prog.statements.push(data_block(1, &content));

        let arena = LintArena::new(1, prog);
        let out = arena.block_subblocks_json(1, "data");
        // Level a through h should appear (8 levels total starting from the
        // outer FrameworkBlock body which is depth-8 recursion). Level i is
        // beyond the depth budget.
        assert!(out.contains("\"name\":\"a\""), "a missing: {}", out);
        assert!(out.contains("\"name\":\"h\""), "h missing: {}", out);
        assert!(
            !out.contains("\"name\":\"i\""),
            "level i should have been dropped by depth limit: {}",
            out
        );
    }

    #[test]
    fn subblocks_multiple_framework_blocks_all_walked() {
        // Two separate data: FrameworkBlocks in the program — both must
        // appear in the top-level array. Same-plugin blocks are additive.
        let mut prog = empty_program();
        prog.statements
            .push(data_block(1, "\tOne:\n\t\tfields:\n\t\t\ta string\n"));
        prog.statements
            .push(data_block(50, "\tTwo:\n\t\tfields:\n\t\t\tb string\n"));
        let arena = LintArena::new(1, prog);
        let out = arena.block_subblocks_json(1, "data");

        assert!(out.contains("\"name\":\"One\""), "One missing: {}", out);
        assert!(out.contains("\"name\":\"Two\""), "Two missing: {}", out);
        // Exactly 2 outer entries — split the JSON by top-level "file" keys.
        // (Two `"file":"app/models.cln"` at top level, since both blocks
        // share the fixture file.)
        let file_count = out.matches("\"file\":\"app/models.cln\"").count();
        assert_eq!(file_count, 2, "expected 2 outer entries, got: {}", out);
    }
}
