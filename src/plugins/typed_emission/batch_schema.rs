//! Batch schema deserializer for typed-emission Amendment 3 §3.12 / §3.13.
//!
//! Landing C2 introduces two batch bridges — `_emit_helpers_batch` and
//! `_emit_class_full` — that accept a single JSON spec instead of the
//! per-node handle stream used by §3.10 / §3.11. The batch bridges are a pure
//! ergonomics win for plugins that emit large synthetic code (frame.data
//! generator, frame.auth session helpers, etc.); no prior constraint is
//! relaxed.
//!
//! This module owns:
//! - `serde::Deserialize` structs matching the JSON schema in Amendment 3
//! - conversion from the batch schema to compiler AST nodes
//! - type-string resolution (mirrors §3.10's `_type_*` constructors)
//!
//! Errors surface as `BatchSchemaError` with a byte offset when possible; the
//! calling bridge wraps them in a PLUGIN013 diagnostic with `source="batch_spec"`.
//!
//! Not covered: parametric templates (§12 OQ8), deferred.

use serde::Deserialize;

use crate::ast::{
    AssignmentTarget, BinaryOperator, Class, Expression, Field, Function, Parameter, Statement,
    Type, UnaryOperator, Value, Visibility,
};

/// Hard limit per Amendment 3 §3.12: at most 100 functions per batch call.
pub const BATCH_FUNCTION_LIMIT: usize = 100;

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum BatchSchemaError {
    /// serde failed to parse. Contains position info when available.
    Json {
        message: String,
        byte_offset: Option<usize>,
    },
    /// batch exceeded BATCH_FUNCTION_LIMIT (§3.12).
    BatchLimit { count: usize, limit: usize },
    /// A type-string could not be resolved to a compiler `Type`.
    UnresolvableType(String),
    /// A `kind` field held a value the schema does not recognise.
    /// (serde catches this as a "no variant matches" JSON error, but we lift
    /// it up when possible so PLUGIN013 messages name the bad kind explicitly.)
    UnknownKind(String),
    /// `body_handle` referenced from a class method body but not resolved
    /// (missing handle, wrong kind, already consumed).
    UnresolvedBodyHandle { handle: i32, reason: String },
}

impl BatchSchemaError {
    pub fn message(&self) -> String {
        match self {
            BatchSchemaError::Json { message, .. } => {
                format!("malformed batch JSON: {}", message)
            }
            BatchSchemaError::BatchLimit { count, limit } => format!(
                "batch of {} functions exceeds §3.12 limit of {}",
                count, limit
            ),
            BatchSchemaError::UnresolvableType(t) => {
                format!("unresolvable type reference `{}`", t)
            }
            BatchSchemaError::UnknownKind(k) => {
                format!("unknown statement or expression kind `{}`", k)
            }
            BatchSchemaError::UnresolvedBodyHandle { handle, reason } => {
                format!("body_handle {} could not be resolved: {}", handle, reason)
            }
        }
    }

    pub fn byte_offset(&self) -> Option<usize> {
        match self {
            BatchSchemaError::Json { byte_offset, .. } => *byte_offset,
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Function-batch schema (§3.12)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct BatchSpec {
    pub functions: Vec<BatchFunction>,
}

#[derive(Debug, Deserialize)]
pub struct BatchFunction {
    pub name: String,
    #[serde(default)]
    pub params: Vec<BatchParam>,
    pub return_type: String,
    /// Function body is either an inline statement list OR a reference to a
    /// pre-computed stmt handle allocated by `_emit_stmt_from_source`
    /// (Amendment 12, §3.14 `batch.func2`). Exactly one of `body` /
    /// `body_handle` should be present; the schema itself does not enforce
    /// this — the bridge validates.
    #[serde(default)]
    pub body: Option<Vec<BatchStatement>>,
    #[serde(default)]
    pub body_handle: Option<i32>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BatchParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Class schema (§3.13)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ClassSpec {
    pub name: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub fields: Vec<BatchField>,
    #[serde(default)]
    pub methods: Vec<BatchMethod>,
    /// Capability names this class claims to conform to (`class Foo can Cap1, Cap2`).
    /// Each name must match a capability declared elsewhere in the program (either
    /// user-authored `can Cap:` block or plugin-emitted via `_emit_capability`).
    /// The resolver validates conformance (SEM011/SEM012/SEM013) and populates
    /// `vtable_descriptors`; codegen's `CallCapability` dispatch consults that map.
    ///
    /// Added 2026-07-19 to close the frame.data v3.0.12 gap where the plugin's
    /// `Database.save(Persist target)` failed at codegen with "no class conforms
    /// to capability #N slot 0 — resolver should have prevented this" because
    /// the plugin had no way to declare that its emitted `Widget` model claims
    /// `can Persist`. Companion fix to Amendment 13 `_emit_capability` (which
    /// only declares capabilities; this field is how classes claim them).
    #[serde(default)]
    pub cans: Vec<String>,
    /// Table name for `{table_name}` substitution in Amendment 10 templates.
    /// Optional; unused unless a `from_spec` method references it.
    #[serde(default)]
    pub table_name: Option<String>,
    /// Amendment 10 §3.17: raw method entries prior to `from_spec` expansion.
    /// Populated by [`parse_class_spec_with_entries`]. When present, this
    /// supersedes `methods` — the bridge expands `from_spec` entries against
    /// `fields` / `name` / `table_name` and rebuilds `methods` before class
    /// conversion.
    #[serde(skip)]
    pub method_entries: Option<Vec<BatchMethodEntry>>,
}

/// Amendment 10 §3.17: a method entry inside `ClassSpec.methods` is either an
/// inline method (existing Amendment 3 shape — no `kind` field, or explicit
/// `kind: "inline"`) or a `from_spec` template expanded compiler-side against
/// the class's fields with a whitelisted placeholder substitution vocabulary.
#[derive(Debug)]
pub enum BatchMethodEntry {
    /// Existing shape (Amendment 3 §3.13). Recognised when `kind` is absent or
    /// explicitly `"inline"`. Body handling is unchanged.
    Inline(BatchMethod),
    /// Amendment 10 shape: a template expanded by the compiler.
    FromSpec { template: FromSpecTemplate },
}

// Custom deserializer: distinguish `from_spec` by the presence of `kind:"from_spec"`;
// everything else falls back to the inline shape (Amendment 3 backward compat).
impl<'de> Deserialize<'de> for BatchMethodEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let kind = value.get("kind").and_then(|v| v.as_str());
        match kind {
            Some("from_spec") => {
                #[derive(Deserialize)]
                struct FromSpecEntry {
                    template: FromSpecTemplate,
                }
                let entry: FromSpecEntry =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(BatchMethodEntry::FromSpec {
                    template: entry.template,
                })
            }
            _ => {
                // Absent kind, or kind:"inline" — treat as Amendment 3 shape.
                let m: BatchMethod =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(BatchMethodEntry::Inline(m))
            }
        }
    }
}

/// Amendment 10 §3.17 template. Iterated `iterate_over` times (0/1/N depending
/// on mode) with per-iteration placeholder substitution over `name_template`,
/// parameter names/types, `return_type`, and string fields inside `body`.
#[derive(Debug, Deserialize, Clone)]
pub struct FromSpecTemplate {
    pub name_template: String,
    #[serde(default)]
    pub params: Vec<BatchParam>,
    pub return_type: String,
    pub body: Vec<BatchStatement>,
    pub iterate_over: FromSpecIterationMode,
    #[serde(default)]
    pub filter: Option<FromSpecFilter>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FromSpecIterationMode {
    /// Emit exactly one method. Any `{field_*}` placeholder in the template is
    /// an error (PLUGIN013 "unknown placeholder in iterate_over=none context").
    None,
    /// Emit one method per field on the class (post-filter). Placeholders
    /// `{field_name}`, `{field_type}`, `{field_name.capitalize}` bind per-field.
    Fields,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FromSpecFilter {
    /// Simple string equality on the field type. Only fields whose `type`
    /// matches will be emitted. Future amendments may add other filter kinds.
    #[serde(default)]
    pub r#type: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Amendment 10 §3.17 — placeholder substitution
// ─────────────────────────────────────────────────────────────────────────────

/// Substitution context for a single template instantiation. Values come from
/// the class spec's own fields — never from user-authored source.
pub struct SubstitutionContext<'a> {
    pub model_name: &'a str,
    pub table_name: Option<&'a str>,
    /// Per-field bindings; None when `iterate_over = none`.
    pub field: Option<FieldBinding<'a>>,
}

#[derive(Clone, Copy)]
pub struct FieldBinding<'a> {
    pub name: &'a str,
    pub ty: &'a str,
}

/// Substitute `{...}` placeholders in `input` against `ctx`. Unknown
/// placeholders return an error (PLUGIN013).
pub fn substitute_placeholders(input: &str, ctx: &SubstitutionContext) -> Result<String, String> {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'{' {
            // Find matching '}'.
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'}' {
                end += 1;
            }
            if end >= bytes.len() {
                return Err(format!(
                    "unterminated placeholder starting at byte {} in `{}`",
                    i, input
                ));
            }
            let key = &input[start..end];
            let value = resolve_placeholder(key, ctx)?;
            out.push_str(&value);
            i = end + 1;
        } else {
            out.push(c as char);
            i += 1;
        }
    }
    Ok(out)
}

fn resolve_placeholder(key: &str, ctx: &SubstitutionContext) -> Result<String, String> {
    match key {
        "model_name" => Ok(ctx.model_name.to_string()),
        "model_name.lowercase" => Ok(ctx.model_name.to_ascii_lowercase()),
        "table_name" => ctx
            .table_name
            .map(|s| s.to_string())
            .ok_or_else(|| "table_name not set on class spec".to_string()),
        "field_name" => ctx.field.map(|f| f.name.to_string()).ok_or_else(|| {
            "unknown placeholder `{field_name}` in iterate_over=none context".to_string()
        }),
        "field_type" => ctx.field.map(|f| f.ty.to_string()).ok_or_else(|| {
            "unknown placeholder `{field_type}` in iterate_over=none context".to_string()
        }),
        "field_name.capitalize" => ctx.field.map(|f| capitalize_ascii(f.name)).ok_or_else(|| {
            "unknown placeholder `{field_name.capitalize}` in iterate_over=none context".to_string()
        }),
        other => Err(format!("unknown placeholder `{{{}}}`", other)),
    }
}

fn capitalize_ascii(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => {
            let mut out = String::with_capacity(s.len());
            out.push(c.to_ascii_uppercase());
            out.extend(chars);
            out
        }
        None => String::new(),
    }
}

/// Recursively substitute placeholders inside a statement's string fields.
pub fn substitute_stmt(
    s: &BatchStatement,
    ctx: &SubstitutionContext,
) -> Result<BatchStatement, String> {
    Ok(match s {
        BatchStatement::Call { callee, args } => BatchStatement::Call {
            callee: substitute_placeholders(callee, ctx)?,
            args: args
                .iter()
                .map(|a| substitute_expr(a, ctx))
                .collect::<Result<Vec<_>, _>>()?,
        },
        BatchStatement::MethodCall {
            receiver,
            method,
            args,
        } => BatchStatement::MethodCall {
            receiver: substitute_expr(receiver, ctx)?,
            method: substitute_placeholders(method, ctx)?,
            args: args
                .iter()
                .map(|a| substitute_expr(a, ctx))
                .collect::<Result<Vec<_>, _>>()?,
        },
        BatchStatement::Assign { target, expr } => BatchStatement::Assign {
            target: substitute_placeholders(target, ctx)?,
            expr: substitute_expr(expr, ctx)?,
        },
        BatchStatement::VarDecl { name, ty, expr } => BatchStatement::VarDecl {
            name: substitute_placeholders(name, ctx)?,
            ty: substitute_placeholders(ty, ctx)?,
            expr: match expr {
                Some(e) => Some(substitute_expr(e, ctx)?),
                None => None,
            },
        },
        BatchStatement::If { cond, then, else_ } => BatchStatement::If {
            cond: substitute_expr(cond, ctx)?,
            then: then
                .iter()
                .map(|s| substitute_stmt(s, ctx))
                .collect::<Result<Vec<_>, _>>()?,
            else_: match else_ {
                Some(es) => Some(
                    es.iter()
                        .map(|s| substitute_stmt(s, ctx))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                None => None,
            },
        },
        BatchStatement::While { cond, body } => BatchStatement::While {
            cond: substitute_expr(cond, ctx)?,
            body: body
                .iter()
                .map(|s| substitute_stmt(s, ctx))
                .collect::<Result<Vec<_>, _>>()?,
        },
        BatchStatement::For {
            iter_var,
            iterable,
            body,
        } => BatchStatement::For {
            iter_var: substitute_placeholders(iter_var, ctx)?,
            iterable: substitute_expr(iterable, ctx)?,
            body: body
                .iter()
                .map(|s| substitute_stmt(s, ctx))
                .collect::<Result<Vec<_>, _>>()?,
        },
        BatchStatement::Return { expr } => BatchStatement::Return {
            expr: match expr {
                Some(e) => Some(substitute_expr(e, ctx)?),
                None => None,
            },
        },
        BatchStatement::Block { stmts } => BatchStatement::Block {
            stmts: stmts
                .iter()
                .map(|s| substitute_stmt(s, ctx))
                .collect::<Result<Vec<_>, _>>()?,
        },
    })
}

/// Recursively substitute placeholders inside an expression's string fields.
pub fn substitute_expr(e: &BatchExpr, ctx: &SubstitutionContext) -> Result<BatchExpr, String> {
    Ok(match e {
        BatchExpr::StringLit { value } => BatchExpr::StringLit {
            value: substitute_placeholders(value, ctx)?,
        },
        BatchExpr::IntLit { value } => BatchExpr::IntLit { value: *value },
        BatchExpr::NumberLit { value } => BatchExpr::NumberLit { value: *value },
        BatchExpr::BoolLit { value } => BatchExpr::BoolLit { value: *value },
        BatchExpr::Ident { name } => BatchExpr::Ident {
            name: substitute_placeholders(name, ctx)?,
        },
        BatchExpr::Field { receiver, name } => BatchExpr::Field {
            receiver: Box::new(substitute_expr(receiver, ctx)?),
            name: substitute_placeholders(name, ctx)?,
        },
        BatchExpr::Call { callee, args } => BatchExpr::Call {
            callee: substitute_placeholders(callee, ctx)?,
            args: args
                .iter()
                .map(|a| substitute_expr(a, ctx))
                .collect::<Result<Vec<_>, _>>()?,
        },
        BatchExpr::MethodCall {
            receiver,
            method,
            args,
        } => BatchExpr::MethodCall {
            receiver: Box::new(substitute_expr(receiver, ctx)?),
            method: substitute_placeholders(method, ctx)?,
            args: args
                .iter()
                .map(|a| substitute_expr(a, ctx))
                .collect::<Result<Vec<_>, _>>()?,
        },
        BatchExpr::BinOp { op, lhs, rhs } => BatchExpr::BinOp {
            op: op.clone(),
            lhs: Box::new(substitute_expr(lhs, ctx)?),
            rhs: Box::new(substitute_expr(rhs, ctx)?),
        },
        BatchExpr::UnOp { op, operand } => BatchExpr::UnOp {
            op: op.clone(),
            operand: Box::new(substitute_expr(operand, ctx)?),
        },
        BatchExpr::Index { receiver, index } => BatchExpr::Index {
            receiver: Box::new(substitute_expr(receiver, ctx)?),
            index: Box::new(substitute_expr(index, ctx)?),
        },
        BatchExpr::ArrayLit { elems } => BatchExpr::ArrayLit {
            elems: elems
                .iter()
                .map(|e| substitute_expr(e, ctx))
                .collect::<Result<Vec<_>, _>>()?,
        },
        BatchExpr::ObjectLit { fields } => BatchExpr::ObjectLit {
            fields: fields
                .iter()
                .map(|f| {
                    Ok::<_, String>(BatchObjectField {
                        key: substitute_placeholders(&f.key, ctx)?,
                        value: substitute_expr(&f.value, ctx)?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        },
    })
}

/// Validate that `name` is a valid Clean identifier per the parser's rules
/// (ASCII letter or `_` first, then letters/digits/`_`). Used to check that a
/// substituted `name_template` result is a legal method name.
pub fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    !name.is_empty()
}

/// Expand a single `FromSpecTemplate` against `class_fields` under
/// `SubstitutionContext`. Returns the concrete method list to append to the
/// class spec's `methods` array.
pub fn expand_from_spec(
    template: &FromSpecTemplate,
    class_name: &str,
    table_name: Option<&str>,
    class_fields: &[BatchField],
) -> Result<Vec<BatchMethod>, String> {
    let mut out = Vec::new();

    // Determine iteration set.
    let iterations: Vec<Option<FieldBinding<'_>>> = match template.iterate_over {
        FromSpecIterationMode::None => vec![None],
        FromSpecIterationMode::Fields => {
            let filtered: Vec<&BatchField> = class_fields
                .iter()
                .filter(|f| match &template.filter {
                    Some(FromSpecFilter { r#type: Some(t) }) => &f.ty == t,
                    _ => true,
                })
                .collect();
            filtered
                .into_iter()
                .map(|f| {
                    Some(FieldBinding {
                        name: &f.name,
                        ty: &f.ty,
                    })
                })
                .collect()
        }
    };

    for field in iterations {
        let ctx = SubstitutionContext {
            model_name: class_name,
            table_name,
            field,
        };

        let name = substitute_placeholders(&template.name_template, &ctx)?;
        if !is_valid_identifier(&name) {
            return Err(format!(
                "substituted name_template `{}` is not a valid Clean identifier",
                name
            ));
        }

        let mut params = Vec::with_capacity(template.params.len());
        for p in &template.params {
            let pname = substitute_placeholders(&p.name, &ctx)?;
            if !is_valid_identifier(&pname) {
                return Err(format!(
                    "substituted param name `{}` is not a valid Clean identifier",
                    pname
                ));
            }
            let pty = substitute_placeholders(&p.ty, &ctx)?;
            params.push(BatchParam {
                name: pname,
                ty: pty,
            });
        }

        let return_type = substitute_placeholders(&template.return_type, &ctx)?;
        // Type validation is deferred to `resolve_type` inside `class_to_ast`.

        let mut body = Vec::with_capacity(template.body.len());
        for s in &template.body {
            body.push(substitute_stmt(s, &ctx)?);
        }

        out.push(BatchMethod {
            name,
            params,
            return_type,
            body: Some(body),
            body_handle: None,
        });
    }

    Ok(out)
}

/// Amendment 10 §3.17: parse a class spec preserving `from_spec` method
/// entries. Populates `method_entries` with the raw union; leaves `methods`
/// empty. The bridge expands `method_entries` in a second pass.
pub fn parse_class_spec_with_entries(json: &str) -> Result<ClassSpec, BatchSchemaError> {
    #[derive(Deserialize)]
    struct Raw {
        name: String,
        #[serde(default)]
        parent: Option<String>,
        #[serde(default)]
        fields: Vec<BatchField>,
        #[serde(default)]
        methods: Vec<BatchMethodEntry>,
        #[serde(default)]
        cans: Vec<String>,
        #[serde(default)]
        table_name: Option<String>,
    }
    let raw: Raw = serde_json::from_str(json).map_err(|e| json_err(&e, json))?;
    Ok(ClassSpec {
        name: raw.name,
        parent: raw.parent,
        fields: raw.fields,
        methods: Vec::new(),
        cans: raw.cans,
        table_name: raw.table_name,
        method_entries: Some(raw.methods),
    })
}

#[derive(Debug, Deserialize, Clone)]
pub struct BatchField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    /// Optional field visibility. Values: `"public"` or `"private"`.
    /// Absent / any other value → `"private"` (per the 2026-06-25 spec flip
    /// making fields private by default).
    ///
    /// Added for prompt dea4378416b8 (EMIT-CLASS-FULL-FIELD-VISIBILITY-MISSING):
    /// plugin-emitted data classes had every field private, so external code
    /// reading `entity.slug` after `Entity.first(...)` hit SEM005. This lets
    /// plugins mark specific fields (or all fields for entity types) as
    /// `"public"` without needing extra accessor methods.
    #[serde(default)]
    pub visibility: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatchMethod {
    pub name: String,
    #[serde(default)]
    pub params: Vec<BatchParam>,
    pub return_type: String,
    /// Method body is either an inline statement list OR a reference to a
    /// pre-computed stmt handle allocated by `_emit_stmt_from_source`.
    /// Exactly one of `body` / `body_handle` should be present; the schema
    /// itself does not enforce this — the bridge validates.
    #[serde(default)]
    pub body: Option<Vec<BatchStatement>>,
    #[serde(default)]
    pub body_handle: Option<i32>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Statement schema
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BatchStatement {
    Call {
        callee: String,
        #[serde(default)]
        args: Vec<BatchExpr>,
    },
    /// Statement-form method call: `receiver.method(args)` used for side effects
    /// (e.g. `list.push(x)`). Lowers to `Statement::Expression { expr:
    /// Expression::MethodCall { .. } }`. See prompt 17d864a6 / assemble.md
    /// companion — plugins have no other way to construct receiver-shaped
    /// method calls on function-body locals.
    MethodCall {
        receiver: BatchExpr,
        method: String,
        #[serde(default)]
        args: Vec<BatchExpr>,
    },
    Assign {
        target: String,
        expr: BatchExpr,
    },
    VarDecl {
        name: String,
        #[serde(rename = "type")]
        ty: String,
        #[serde(default)]
        expr: Option<BatchExpr>,
    },
    If {
        cond: BatchExpr,
        then: Vec<BatchStatement>,
        #[serde(rename = "else", default)]
        else_: Option<Vec<BatchStatement>>,
    },
    While {
        cond: BatchExpr,
        body: Vec<BatchStatement>,
    },
    For {
        iter_var: String,
        iterable: BatchExpr,
        body: Vec<BatchStatement>,
    },
    Return {
        #[serde(default)]
        expr: Option<BatchExpr>,
    },
    Block {
        stmts: Vec<BatchStatement>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Expression schema
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BatchExpr {
    StringLit {
        value: String,
    },
    IntLit {
        value: i64,
    },
    NumberLit {
        value: f64,
    },
    BoolLit {
        value: bool,
    },
    Ident {
        name: String,
    },
    Field {
        receiver: Box<BatchExpr>,
        name: String,
    },
    Call {
        callee: String,
        #[serde(default)]
        args: Vec<BatchExpr>,
    },
    /// Expression-form method call: `receiver.method(args)`. Lowers to
    /// `Expression::MethodCall`. Distinct from `Call` (which builds
    /// `Expression::Call` — a bare function call). Plugin authors reach for
    /// this when they need to emit `s.replace(...)` or `list.length()`
    /// inside a plugin-generated function body.
    MethodCall {
        receiver: Box<BatchExpr>,
        method: String,
        #[serde(default)]
        args: Vec<BatchExpr>,
    },
    BinOp {
        op: String,
        lhs: Box<BatchExpr>,
        rhs: Box<BatchExpr>,
    },
    UnOp {
        op: String,
        operand: Box<BatchExpr>,
    },
    Index {
        receiver: Box<BatchExpr>,
        index: Box<BatchExpr>,
    },
    ArrayLit {
        #[serde(default)]
        elems: Vec<BatchExpr>,
    },
    ObjectLit {
        #[serde(default)]
        fields: Vec<BatchObjectField>,
    },
}

#[derive(Debug, Deserialize, Clone)]
pub struct BatchObjectField {
    pub key: String,
    pub value: BatchExpr,
}

// ─────────────────────────────────────────────────────────────────────────────
// Top-level parse entry points
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a JSON string into a `BatchSpec`, translating serde errors into a
/// `BatchSchemaError::Json` carrying the byte offset when serde provides one.
pub fn parse_batch_spec(json: &str) -> Result<BatchSpec, BatchSchemaError> {
    serde_json::from_str::<BatchSpec>(json).map_err(|e| json_err(&e, json))
}

/// Parse a JSON string into a `ClassSpec`.
pub fn parse_class_spec(json: &str) -> Result<ClassSpec, BatchSchemaError> {
    serde_json::from_str::<ClassSpec>(json).map_err(|e| json_err(&e, json))
}

fn json_err(e: &serde_json::Error, source: &str) -> BatchSchemaError {
    // serde_json reports 1-indexed line + column. Translate to a byte offset
    // by walking the source string ourselves. This is the honest fallback
    // path called out in the Landing C2 brief.
    let line = e.line();
    let col = e.column();
    let byte_offset = if line == 0 {
        None
    } else {
        line_col_to_offset(source, line, col)
    };
    let message = e.to_string();

    // Try to pull an "unknown variant" kind name out of serde's message; this
    // is best-effort so PLUGIN013 messages can name the offending kind.
    if let Some(kind) = extract_unknown_variant(&message) {
        return BatchSchemaError::UnknownKind(kind);
    }

    BatchSchemaError::Json {
        message,
        byte_offset,
    }
}

fn line_col_to_offset(source: &str, line: usize, col: usize) -> Option<usize> {
    let mut off = 0usize;
    let mut current_line = 1usize;
    for ch in source.chars() {
        if current_line == line {
            // col is 1-indexed characters within the line.
            let mut byte_within = 0usize;
            for (c, lc) in (1usize..).zip(source[off..].chars()) {
                if c == col {
                    return Some(off + byte_within);
                }
                if lc == '\n' {
                    break;
                }
                byte_within += lc.len_utf8();
            }
            return Some(off + byte_within);
        }
        off += ch.len_utf8();
        if ch == '\n' {
            current_line += 1;
        }
    }
    None
}

fn extract_unknown_variant(msg: &str) -> Option<String> {
    // serde error text: `unknown variant \`foo\`, expected one of ...`
    let marker = "unknown variant `";
    let start = msg.find(marker)? + marker.len();
    let end = msg[start..].find('`')?;
    Some(msg[start..start + end].to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Type-string resolution (mirrors §3.10)
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve a type-string like `"integer"`, `"string"`, `"Array<T>"`, `"User"`
/// to a compiler `Type`. Mirrors what `_type_string`, `_type_integer`, ...
/// produce so the batch path is behaviour-identical to per-node emission.
pub fn resolve_type(name: &str) -> Result<Type, BatchSchemaError> {
    let name = name.trim();
    match name {
        "string" => Ok(Type::String),
        "integer" => Ok(Type::Integer),
        "number" => Ok(Type::Number),
        "boolean" => Ok(Type::Boolean),
        "void" => Ok(Type::Void),
        "any" => Ok(Type::Any),
        _ => {
            // Array<T> and list<T> both → List(resolve(T)).
            //
            // grammar.ebnf uses `list<T>` (lowercase) as the primary Clean
            // Language syntax; `Array<T>` (uppercase) is a legacy alias kept
            // for the batch-spec JSON path. Both are accepted here so plugins
            // can emit type strings that match user-visible language syntax
            // — frame.data's ORM helpers generate `list<Todo>` from a source
            // model name and expect that to parse as `List<Todo>`.
            for prefix in ["Array<", "list<"] {
                if let Some(rest) = name.strip_prefix(prefix) {
                    if let Some(inner) = rest.strip_suffix('>') {
                        let elem = resolve_type(inner)?;
                        return Ok(Type::List(
                            Box::new(elem),
                            crate::ast::ListBehavior::Default,
                        ));
                    }
                    return Err(BatchSchemaError::UnresolvableType(name.to_string()));
                }
            }
            // Anything else that starts with an uppercase letter is treated
            // as a class reference — matches `_type_class_ref` behaviour.
            if name
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false)
            {
                return Ok(Type::Object(name.to_string()));
            }
            Err(BatchSchemaError::UnresolvableType(name.to_string()))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AST construction
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a `BatchExpr` to an AST `Expression`.
pub fn expr_to_ast(e: BatchExpr) -> Result<Expression, BatchSchemaError> {
    Ok(match e {
        BatchExpr::StringLit { value } => Expression::Literal(Value::String(value)),
        BatchExpr::IntLit { value } => Expression::Literal(Value::Integer(value)),
        BatchExpr::NumberLit { value } => Expression::Literal(Value::Number(value)),
        BatchExpr::BoolLit { value } => Expression::Literal(Value::Boolean(value)),
        BatchExpr::Ident { name } => Expression::Variable(name),
        BatchExpr::Field { receiver, name } => Expression::PropertyAccess {
            object: Box::new(expr_to_ast(*receiver)?),
            property: name,
            location: crate::ast::SourceLocation::new(0, 0, "plugin"),
        },
        BatchExpr::Call { callee, args } => {
            let converted: Result<Vec<_>, _> = args.into_iter().map(expr_to_ast).collect();
            Expression::Call(callee, converted?)
        }
        BatchExpr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let converted: Result<Vec<_>, _> = args.into_iter().map(expr_to_ast).collect();
            Expression::MethodCall {
                object: Box::new(expr_to_ast(*receiver)?),
                method,
                arguments: converted?,
                location: crate::ast::SourceLocation::new(0, 0, "plugin"),
            }
        }
        BatchExpr::BinOp { op, lhs, rhs } => {
            let op = map_binop(&op)?;
            Expression::Binary(
                Box::new(expr_to_ast(*lhs)?),
                op,
                Box::new(expr_to_ast(*rhs)?),
            )
        }
        BatchExpr::UnOp { op, operand } => {
            let op = map_unop(&op)?;
            Expression::Unary(op, Box::new(expr_to_ast(*operand)?))
        }
        BatchExpr::Index { receiver, index } => Expression::ListAccess(
            Box::new(expr_to_ast(*receiver)?),
            Box::new(expr_to_ast(*index)?),
        ),
        BatchExpr::ArrayLit { elems } => {
            let converted: Result<Vec<_>, _> = elems.into_iter().map(expr_to_ast).collect();
            Expression::Call("__array_literal__".to_string(), converted?)
        }
        BatchExpr::ObjectLit { fields } => {
            let mut args = Vec::with_capacity(fields.len() * 2);
            for f in fields {
                args.push(Expression::Literal(Value::String(f.key)));
                args.push(expr_to_ast(f.value)?);
            }
            Expression::Call("__object_literal__".to_string(), args)
        }
    })
}

fn map_binop(op: &str) -> Result<BinaryOperator, BatchSchemaError> {
    super::arena::map_binary_op(op)
        .ok_or_else(|| BatchSchemaError::UnknownKind(format!("binop `{}`", op)))
}

fn map_unop(op: &str) -> Result<UnaryOperator, BatchSchemaError> {
    super::arena::map_unary_op(op)
        .ok_or_else(|| BatchSchemaError::UnknownKind(format!("unop `{}`", op)))
}

/// Convert a `BatchStatement` to an AST `Statement`.
pub fn stmt_to_ast(s: BatchStatement) -> Result<Statement, BatchSchemaError> {
    Ok(match s {
        BatchStatement::Call { callee, args } => {
            let converted: Result<Vec<_>, _> = args.into_iter().map(expr_to_ast).collect();
            Statement::Expression {
                expr: Expression::Call(callee, converted?),
                location: None,
            }
        }
        BatchStatement::MethodCall {
            receiver,
            method,
            args,
        } => {
            let converted: Result<Vec<_>, _> = args.into_iter().map(expr_to_ast).collect();
            Statement::Expression {
                expr: Expression::MethodCall {
                    object: Box::new(expr_to_ast(receiver)?),
                    method,
                    arguments: converted?,
                    location: crate::ast::SourceLocation::new(0, 0, "plugin"),
                },
                location: None,
            }
        }
        BatchStatement::Assign { target, expr } => Statement::Assignment {
            target: AssignmentTarget::Variable(target),
            value: expr_to_ast(expr)?,
            location: None,
        },
        BatchStatement::VarDecl { name, ty, expr } => Statement::VariableDecl {
            name,
            type_: resolve_type(&ty)?,
            initializer: match expr {
                Some(e) => Some(expr_to_ast(e)?),
                None => None,
            },
            location: None,
        },
        BatchStatement::If { cond, then, else_ } => {
            let then_branch: Result<Vec<_>, _> = then.into_iter().map(stmt_to_ast).collect();
            let else_branch = if let Some(es) = else_ {
                let converted: Result<Vec<_>, _> = es.into_iter().map(stmt_to_ast).collect();
                Some(converted?)
            } else {
                None
            };
            Statement::If {
                condition: expr_to_ast(cond)?,
                then_branch: then_branch?,
                else_branch,
                location: None,
            }
        }
        BatchStatement::While { cond, body } => {
            let body: Result<Vec<_>, _> = body.into_iter().map(stmt_to_ast).collect();
            Statement::While {
                condition: expr_to_ast(cond)?,
                body: body?,
                location: None,
            }
        }
        BatchStatement::For {
            iter_var,
            iterable,
            body,
        } => {
            let body: Result<Vec<_>, _> = body.into_iter().map(stmt_to_ast).collect();
            Statement::Iterate {
                iterator: iter_var,
                collection: expr_to_ast(iterable)?,
                body: body?,
                location: None,
            }
        }
        BatchStatement::Return { expr } => Statement::Return {
            value: match expr {
                Some(e) => Some(expr_to_ast(e)?),
                None => None,
            },
            location: None,
        },
        BatchStatement::Block { stmts } => {
            // A bare block wraps into the same If(true, [...], None) shape used
            // by `_stmt_block` — that keeps `flatten_block` (bridges.rs) able to
            // extract nested blocks uniformly.
            let stmts: Result<Vec<_>, _> = stmts.into_iter().map(stmt_to_ast).collect();
            Statement::If {
                condition: Expression::Literal(Value::Boolean(true)),
                then_branch: stmts?,
                else_branch: None,
                location: None,
            }
        }
    })
}

/// Convert a `BatchFunction` to an AST `Function`. `flags` bits are the same
/// as `_emit_function`: bit 0 = exported (Public), higher bits ignored here
/// (the caller applies bit 1 = BFS root at its own layer).
///
/// Only accepts the inline-body path (`body: Some(_)`, `body_handle: None`).
/// For the `body_handle` path introduced by Amendment 12 (§3.14 `batch.func2`),
/// callers must resolve the handle via arena.take_stmt() and use
/// `function_to_ast_with_body`.
pub fn function_to_ast(f: BatchFunction, exported: bool) -> Result<Function, BatchSchemaError> {
    let inline = match (f.body, f.body_handle) {
        (Some(stmts), None) => stmts,
        (None, Some(h)) => {
            return Err(BatchSchemaError::UnresolvedBodyHandle {
                handle: h,
                reason: "function_to_ast does not resolve body_handle; caller must \
                         resolve via arena.take_stmt() and use function_to_ast_with_body"
                    .to_string(),
            });
        }
        (Some(_), Some(_)) => {
            return Err(BatchSchemaError::Json {
                message: "function has both `body` and `body_handle`; exactly one required"
                    .to_string(),
                byte_offset: None,
            });
        }
        (None, None) => {
            return Err(BatchSchemaError::Json {
                message: "function has neither `body` nor `body_handle`; exactly one required"
                    .to_string(),
                byte_offset: None,
            });
        }
    };
    let mut resolved = Vec::with_capacity(inline.len());
    for s in inline {
        resolved.push(stmt_to_ast(s)?);
    }
    function_to_ast_with_body(f.name, f.params, f.return_type, resolved, exported)
}

/// Convert a `BatchFunction`'s header plus an already-resolved AST body into
/// a `Function`. Sibling to `class_to_ast` which takes `method_bodies`; this
/// separation lets the bridge consume `_emit_stmt_from_source` handles
/// (Amendment 12 §3.14 `batch.func2` `from_source_handle_or_0` path) without
/// this pure conversion layer having to know about arenas.
pub fn function_to_ast_with_body(
    name: String,
    params_in: Vec<BatchParam>,
    return_type_in: String,
    body: Vec<Statement>,
    exported: bool,
) -> Result<Function, BatchSchemaError> {
    let mut params = Vec::with_capacity(params_in.len());
    for p in params_in {
        params.push(Parameter::new(p.name, resolve_type(&p.ty)?));
    }
    let return_type = resolve_type(&return_type_in)?;
    let mut func = Function::new(name, params, return_type, body, None);
    if exported {
        func.visibility = Visibility::Public;
    }
    Ok(func)
}

/// Convert a `ClassSpec` plus already-resolved method bodies into an AST `Class`.
///
/// `method_bodies` is passed in aligned with `spec.methods` (same length,
/// same order). Each entry is the fully-resolved `Vec<Statement>` for that
/// method's body — the caller has already handled the inline-vs-`body_handle`
/// choice. This separation is what lets the bridge consume `_emit_stmt_from_source`
/// handles from the arena without this pure conversion layer having to know
/// about arenas.
pub fn class_to_ast(
    spec: ClassSpec,
    method_bodies: Vec<Vec<Statement>>,
    exported: bool,
) -> Result<Class, BatchSchemaError> {
    if spec.methods.len() != method_bodies.len() {
        return Err(BatchSchemaError::UnresolvedBodyHandle {
            handle: 0,
            reason: "internal: method_bodies length mismatch".to_string(),
        });
    }
    let mut class = Class::new(spec.name, None);
    class.base_class = spec.parent;
    class.capabilities = spec.cans;

    for field in spec.fields {
        let ty = resolve_type(&field.ty)?;
        let mut f = Field::new(field.name, ty);
        // Honor optional `visibility: "public"` on the spec. Everything else
        // (absent, `"private"`, unknown) keeps the private default set by
        // Field::new. See BatchField.visibility doc for the motivating case.
        if let Some(v) = field.visibility.as_deref() {
            if v.eq_ignore_ascii_case("public") {
                f.visibility = Visibility::Public;
            }
        }
        class.fields.push(f);
    }

    for (m, body) in spec.methods.into_iter().zip(method_bodies) {
        let mut params = Vec::with_capacity(m.params.len());
        for p in m.params {
            params.push(Parameter::new(p.name, resolve_type(&p.ty)?));
        }
        let return_type = resolve_type(&m.return_type)?;
        let mut func = Function::new(m.name, params, return_type, body, None);
        if exported {
            func.visibility = Visibility::Public;
        }
        class.methods.push(func);
    }

    Ok(class)
}

// ─────────────────────────────────────────────────────────────────────────────
// Capability schema (§3.18 / Amendment 13)
// ─────────────────────────────────────────────────────────────────────────────

/// A top-level `can Name:` capability declaration emitted by the
/// `_emit_capability` bridge. v1 of the bridge is contract-only — the JSON
/// spec has no `body` field. Future amendments may add default-body support
/// mirroring the `can` grammar (`foundation/spec/grammar.ebnf` §6.4a), which
/// already allows both required signatures and defaulted methods.
#[derive(Debug, Deserialize)]
pub struct CapabilitySpec {
    pub name: String,
    #[serde(default)]
    pub methods: Vec<CapabilityMethodSpec>,
}

/// One method entry in a `CapabilitySpec`. Mirrors the class-method spec
/// (`BatchMethod`) but has no `body` / `body_handle` — capability methods in
/// v1 are contract signatures only.
#[derive(Debug, Deserialize)]
pub struct CapabilityMethodSpec {
    pub name: String,
    #[serde(default)]
    pub params: Vec<BatchParam>,
    pub return_type: String,
}

/// Parse a JSON capability spec (per typed-emission.md §3.18).
/// Errors are lifted into `BatchSchemaError::Json` with the underlying
/// serde message + byte offset so `emit_plugin013` can produce the same
/// diagnostic shape as class/function specs.
pub fn parse_capability_spec(json: &str) -> Result<CapabilitySpec, BatchSchemaError> {
    serde_json::from_str(json).map_err(|e| BatchSchemaError::Json {
        message: e.to_string(),
        byte_offset: Some(e.column().saturating_sub(1)),
    })
}

/// Convert a parsed `CapabilitySpec` to an `ast::Capability` node. Validates
/// that no method name is duplicated within the capability — the compiler's
/// resolver would flag a duplicate later, but catching it here produces a
/// better plugin-attributed diagnostic (PLUGIN013).
pub fn capability_to_ast(spec: CapabilitySpec) -> Result<crate::ast::Capability, BatchSchemaError> {
    let mut cap = crate::ast::Capability::new(spec.name, None);
    let mut seen: std::collections::HashSet<String> =
        std::collections::HashSet::with_capacity(spec.methods.len());
    for m in spec.methods {
        if !seen.insert(m.name.clone()) {
            return Err(BatchSchemaError::Json {
                message: format!("capability method `{}` appears more than once", m.name),
                byte_offset: None,
            });
        }
        let return_type = resolve_type(&m.return_type)?;
        let mut params = Vec::with_capacity(m.params.len());
        for p in m.params {
            params.push(Parameter::new(p.name, resolve_type(&p.ty)?));
        }
        cap.methods.push(crate::ast::CapabilityMethod {
            name: m.name,
            parameters: params,
            return_type,
            default_body: None,
            location: None,
        });
    }
    Ok(cap)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (pure — no wasm required)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_batch() {
        let json = r#"{
            "functions": [{
                "name": "f",
                "params": [],
                "return_type": "integer",
                "body": [{"kind":"return","expr":{"kind":"int_lit","value":42}}]
            }]
        }"#;
        let spec = parse_batch_spec(json).unwrap();
        assert_eq!(spec.functions.len(), 1);
        assert_eq!(spec.functions[0].name, "f");
    }

    #[test]
    fn parse_control_flow_batch() {
        let json = r#"{
            "functions": [{
                "name": "g",
                "params": [{"name":"n","type":"integer"}],
                "return_type": "integer",
                "body": [
                    {"kind":"if",
                     "cond":{"kind":"bin_op","op":">",
                             "lhs":{"kind":"ident","name":"n"},
                             "rhs":{"kind":"int_lit","value":0}},
                     "then":[{"kind":"return","expr":{"kind":"ident","name":"n"}}],
                     "else":[{"kind":"return","expr":{"kind":"int_lit","value":0}}]
                    }
                ]
            }]
        }"#;
        let spec = parse_batch_spec(json).unwrap();
        let f = function_to_ast(spec.functions.into_iter().next().unwrap(), true).unwrap();
        assert_eq!(f.name, "g");
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.body.len(), 1);
    }

    #[test]
    fn parse_unknown_kind_lifts_to_unknownkind() {
        let json = r#"{"functions":[{"name":"x","params":[],"return_type":"void",
                       "body":[{"kind":"invalid"}]}]}"#;
        let err = parse_batch_spec(json).unwrap_err();
        match err {
            BatchSchemaError::UnknownKind(k) => assert_eq!(k, "invalid"),
            other => panic!("expected UnknownKind, got {:?}", other),
        }
    }

    #[test]
    fn resolve_array_type() {
        let t = resolve_type("Array<integer>").unwrap();
        matches!(t, Type::List(_, _));
    }

    #[test]
    fn resolve_class_ref() {
        let t = resolve_type("User").unwrap();
        assert!(matches!(t, Type::Object(ref n) if n == "User"));
    }

    #[test]
    fn resolve_bogus_type_errors() {
        assert!(matches!(
            resolve_type("bogus_lowercase"),
            Err(BatchSchemaError::UnresolvableType(_))
        ));
    }

    #[test]
    fn parse_class_with_body_handle() {
        let json = r#"{
            "name": "User",
            "fields": [{"name":"id","type":"integer"}],
            "methods": [{
                "name": "findById",
                "params": [{"name":"id","type":"integer"}],
                "return_type": "User",
                "body_handle": 7
            }]
        }"#;
        let spec = parse_class_spec(json).unwrap();
        assert_eq!(spec.name, "User");
        assert_eq!(spec.methods.len(), 1);
        assert_eq!(spec.methods[0].body_handle, Some(7));
        assert!(spec.methods[0].body.is_none());
    }

    #[test]
    fn parse_class_with_inline_body() {
        let json = r#"{
            "name": "User",
            "fields": [],
            "methods": [{
                "name": "hello",
                "params": [],
                "return_type": "void",
                "body": [{"kind":"return"}]
            }]
        }"#;
        let spec = parse_class_spec(json).unwrap();
        assert_eq!(spec.methods[0].body.as_ref().unwrap().len(), 1);
        assert!(spec.methods[0].body_handle.is_none());
    }

    // ─────────────────────────────────────────────────────────────────────
    // Amendment 10 §3.17 — `_emit_class_methods_from_spec`
    // ─────────────────────────────────────────────────────────────────────

    fn getter_template_json() -> &'static str {
        r#"{
            "name": "User",
            "table_name": "users",
            "fields": [
                {"name":"id","type":"integer"},
                {"name":"email","type":"string"}
            ],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"get{field_name.capitalize}",
                    "params":[],
                    "return_type":"{field_type}",
                    "body":[{"kind":"return","expr":{"kind":"field",
                        "receiver":{"kind":"ident","name":"this"},
                        "name":"{field_name}"}}],
                    "iterate_over":"fields"
                }}
            ]
        }"#
    }

    fn expand_entries(spec: &mut ClassSpec) -> Result<(), String> {
        let entries = spec.method_entries.take().unwrap_or_default();
        let mut out = Vec::new();
        for e in entries {
            match e {
                BatchMethodEntry::Inline(m) => out.push(m),
                BatchMethodEntry::FromSpec { template } => {
                    let mut ms = expand_from_spec(
                        &template,
                        &spec.name,
                        spec.table_name.as_deref(),
                        &spec.fields,
                    )?;
                    out.append(&mut ms);
                }
            }
        }
        spec.methods = out;
        Ok(())
    }

    #[test]
    fn class_methods_from_spec_iterate_none_emits_one_method() {
        let json = r#"{
            "name": "U",
            "fields": [{"name":"a","type":"integer"}],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"noop",
                    "params":[],
                    "return_type":"void",
                    "body":[{"kind":"return"}],
                    "iterate_over":"none"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        expand_entries(&mut spec).unwrap();
        assert_eq!(spec.methods.len(), 1);
        assert_eq!(spec.methods[0].name, "noop");
    }

    #[test]
    fn class_methods_from_spec_iterate_fields_emits_n_methods() {
        let mut spec = parse_class_spec_with_entries(getter_template_json()).unwrap();
        expand_entries(&mut spec).unwrap();
        assert_eq!(spec.methods.len(), 2);
    }

    #[test]
    fn class_methods_from_spec_iterate_fields_empty_emits_zero_methods() {
        let json = r#"{
            "name": "U",
            "fields": [],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"get{field_name.capitalize}",
                    "params":[],
                    "return_type":"{field_type}",
                    "body":[{"kind":"return"}],
                    "iterate_over":"fields"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        expand_entries(&mut spec).unwrap();
        assert_eq!(spec.methods.len(), 0);
    }

    #[test]
    fn class_methods_from_spec_substitutes_field_name_in_method_name() {
        let mut spec = parse_class_spec_with_entries(getter_template_json()).unwrap();
        expand_entries(&mut spec).unwrap();
        let names: Vec<_> = spec.methods.iter().map(|m| m.name.clone()).collect();
        assert_eq!(names, vec!["getId", "getEmail"]);
    }

    #[test]
    fn class_methods_from_spec_substitutes_field_type_in_return_type() {
        let mut spec = parse_class_spec_with_entries(getter_template_json()).unwrap();
        expand_entries(&mut spec).unwrap();
        assert_eq!(spec.methods[0].return_type, "integer");
        assert_eq!(spec.methods[1].return_type, "string");
    }

    #[test]
    fn class_methods_from_spec_substitutes_model_name_in_body() {
        let json = r#"{
            "name": "Widget",
            "fields": [],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"describe",
                    "params":[],
                    "return_type":"string",
                    "body":[{"kind":"return",
                        "expr":{"kind":"string_lit","value":"model={model_name}"}}],
                    "iterate_over":"none"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        expand_entries(&mut spec).unwrap();
        let body = spec.methods[0].body.as_ref().unwrap();
        let ret = &body[0];
        match ret {
            BatchStatement::Return {
                expr: Some(BatchExpr::StringLit { value }),
            } => {
                assert_eq!(value, "model=Widget");
            }
            _ => panic!("expected return with string_lit"),
        }
    }

    #[test]
    fn class_methods_from_spec_substitutes_table_name_in_string_literal() {
        let json = r#"{
            "name": "User",
            "table_name": "users_v2",
            "fields": [],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"table",
                    "params":[],
                    "return_type":"string",
                    "body":[{"kind":"return",
                        "expr":{"kind":"string_lit","value":"SELECT * FROM {table_name}"}}],
                    "iterate_over":"none"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        expand_entries(&mut spec).unwrap();
        let body = spec.methods[0].body.as_ref().unwrap();
        match &body[0] {
            BatchStatement::Return {
                expr: Some(BatchExpr::StringLit { value }),
            } => {
                assert_eq!(value, "SELECT * FROM users_v2");
            }
            _ => panic!("expected return with substituted string_lit"),
        }
    }

    #[test]
    fn class_methods_from_spec_missing_table_name_returns_error() {
        let json = r#"{
            "name": "User",
            "fields": [],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"t",
                    "params":[],
                    "return_type":"string",
                    "body":[{"kind":"return",
                        "expr":{"kind":"string_lit","value":"{table_name}"}}],
                    "iterate_over":"none"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        let err = expand_entries(&mut spec).unwrap_err();
        assert!(err.contains("table_name"), "got: {}", err);
    }

    #[test]
    fn class_methods_from_spec_unknown_placeholder_returns_error() {
        let json = r#"{
            "name": "U",
            "fields": [{"name":"a","type":"integer"}],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"m",
                    "params":[],
                    "return_type":"void",
                    "body":[{"kind":"call","callee":"log",
                        "args":[{"kind":"string_lit","value":"{arbitrary}"}]}],
                    "iterate_over":"fields"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        let err = expand_entries(&mut spec).unwrap_err();
        assert!(err.contains("unknown placeholder"), "got: {}", err);
    }

    #[test]
    fn class_methods_from_spec_invalid_identifier_after_substitution_returns_error() {
        // name_template that yields an invalid identifier (starts with a digit).
        // We use a literal invalid name — no substitution needed to demonstrate.
        let json = r#"{
            "name": "U",
            "fields": [],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"1bad",
                    "params":[],
                    "return_type":"void",
                    "body":[{"kind":"return"}],
                    "iterate_over":"none"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        let err = expand_entries(&mut spec).unwrap_err();
        assert!(err.contains("not a valid Clean identifier"), "got: {}", err);
    }

    #[test]
    fn class_methods_from_spec_filter_by_type_matches_only_matching_fields() {
        let json = r#"{
            "name": "U",
            "fields": [
                {"name":"id","type":"integer"},
                {"name":"email","type":"string"},
                {"name":"score","type":"integer"}
            ],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"get{field_name.capitalize}",
                    "params":[],
                    "return_type":"integer",
                    "body":[{"kind":"return","expr":{"kind":"int_lit","value":0}}],
                    "iterate_over":"fields",
                    "filter":{"type":"integer"}
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        expand_entries(&mut spec).unwrap();
        let names: Vec<_> = spec.methods.iter().map(|m| m.name.clone()).collect();
        assert_eq!(names, vec!["getId", "getScore"]);
    }

    #[test]
    fn class_methods_from_spec_backcompat_inline_without_kind_field() {
        // Amendment 3 shape (no `kind`) must still parse as Inline.
        let json = r#"{
            "name": "U",
            "fields": [],
            "methods": [
                {"name":"hello","params":[],"return_type":"void",
                 "body":[{"kind":"return"}]}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        expand_entries(&mut spec).unwrap();
        assert_eq!(spec.methods.len(), 1);
        assert_eq!(spec.methods[0].name, "hello");
    }

    #[test]
    fn class_methods_from_spec_composes_with_inline_methods() {
        let json = r#"{
            "name": "U",
            "fields": [{"name":"id","type":"integer"}],
            "methods": [
                {"kind":"inline",
                 "name":"first",
                 "params":[],
                 "return_type":"void",
                 "body":[{"kind":"return"}]},
                {"kind":"from_spec","template":{
                    "name_template":"get{field_name.capitalize}",
                    "params":[],
                    "return_type":"{field_type}",
                    "body":[{"kind":"return"}],
                    "iterate_over":"fields"
                }},
                {"kind":"inline",
                 "name":"last",
                 "params":[],
                 "return_type":"void",
                 "body":[{"kind":"return"}]}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        expand_entries(&mut spec).unwrap();
        let names: Vec<_> = spec.methods.iter().map(|m| m.name.clone()).collect();
        assert_eq!(names, vec!["first", "getId", "last"]);
    }

    #[test]
    fn class_methods_from_spec_iterate_none_rejects_field_placeholder() {
        let json = r#"{
            "name": "U",
            "fields": [{"name":"x","type":"integer"}],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"get{field_name}",
                    "params":[],
                    "return_type":"void",
                    "body":[{"kind":"return"}],
                    "iterate_over":"none"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        let err = expand_entries(&mut spec).unwrap_err();
        assert!(
            err.contains("field_name") && err.contains("iterate_over=none"),
            "got: {}",
            err
        );
    }

    #[test]
    fn class_methods_from_spec_substitutes_model_name_lowercase() {
        let json = r#"{
            "name": "Widget",
            "fields": [],
            "methods": [
                {"kind":"from_spec","template":{
                    "name_template":"table_of",
                    "params":[],
                    "return_type":"string",
                    "body":[{"kind":"return",
                        "expr":{"kind":"string_lit","value":"{model_name.lowercase}s"}}],
                    "iterate_over":"none"
                }}
            ]
        }"#;
        let mut spec = parse_class_spec_with_entries(json).unwrap();
        expand_entries(&mut spec).unwrap();
        match &spec.methods[0].body.as_ref().unwrap()[0] {
            BatchStatement::Return {
                expr: Some(BatchExpr::StringLit { value }),
            } => {
                assert_eq!(value, "widgets");
            }
            _ => panic!(),
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Amendment 13 §3.18 — `_emit_capability`
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_capability_spec_minimal() {
        let json = r#"{
            "name": "Persist",
            "methods": [
                {"name": "save",   "params": [], "return_type": "boolean"},
                {"name": "delete", "params": [], "return_type": "boolean"}
            ]
        }"#;
        let spec = parse_capability_spec(json).unwrap();
        assert_eq!(spec.name, "Persist");
        assert_eq!(spec.methods.len(), 2);
        assert_eq!(spec.methods[0].name, "save");
        assert_eq!(spec.methods[1].name, "delete");
    }

    #[test]
    fn capability_to_ast_resolves_types() {
        let spec = parse_capability_spec(
            r#"{
                "name": "Renderable",
                "methods": [
                    {"name":"draw","params":[{"name":"n","type":"integer"}],"return_type":"void"}
                ]
            }"#,
        )
        .unwrap();
        let cap = capability_to_ast(spec).unwrap();
        assert_eq!(cap.name, "Renderable");
        assert_eq!(cap.methods.len(), 1);
        assert_eq!(cap.methods[0].name, "draw");
        assert!(matches!(cap.methods[0].return_type, Type::Void));
        assert_eq!(cap.methods[0].parameters.len(), 1);
        assert_eq!(cap.methods[0].parameters[0].name, "n");
        assert!(matches!(cap.methods[0].parameters[0].type_, Type::Integer));
        // v1 is contract-only.
        assert!(cap.methods[0].default_body.is_none());
    }

    #[test]
    fn capability_duplicate_method_rejected() {
        let spec = parse_capability_spec(
            r#"{
                "name": "Persist",
                "methods": [
                    {"name":"save","params":[],"return_type":"boolean"},
                    {"name":"save","params":[],"return_type":"boolean"}
                ]
            }"#,
        )
        .unwrap();
        let err = capability_to_ast(spec).unwrap_err();
        match err {
            BatchSchemaError::Json { message, .. } => {
                assert!(message.contains("save"), "message: {}", message);
                assert!(message.contains("more than once"), "message: {}", message);
            }
            other => panic!("expected Json error, got {:?}", other),
        }
    }

    #[test]
    fn capability_malformed_json_lifts_to_json_error() {
        let err = parse_capability_spec("not json").unwrap_err();
        assert!(matches!(err, BatchSchemaError::Json { .. }));
    }

    #[test]
    fn capability_unresolvable_return_type() {
        let spec = parse_capability_spec(
            r#"{"name":"X","methods":[{"name":"f","params":[],"return_type":"bogus_lowercase"}]}"#,
        )
        .unwrap();
        let err = capability_to_ast(spec).unwrap_err();
        assert!(matches!(err, BatchSchemaError::UnresolvableType(_)));
    }

    // ── BatchField.visibility (prompt dea4378416b8) ────────────────────────────

    #[test]
    fn class_field_visibility_defaults_to_private_when_absent() {
        // Absent visibility on the field spec must not change behavior — the
        // Field::new default is Private per the 2026-06-25 spec flip.
        let spec = parse_class_spec(
            r#"{"name":"User","fields":[{"name":"id","type":"integer"}],"methods":[]}"#,
        )
        .unwrap();
        let class = class_to_ast(spec, Vec::new(), false).unwrap();
        assert_eq!(class.fields.len(), 1);
        assert!(matches!(class.fields[0].visibility, Visibility::Private));
    }

    #[test]
    fn class_field_visibility_public_when_spec_says_public() {
        let spec = parse_class_spec(
            r#"{"name":"Page","fields":[{"name":"slug","type":"string","visibility":"public"}],"methods":[]}"#,
        ).unwrap();
        let class = class_to_ast(spec, Vec::new(), false).unwrap();
        assert!(matches!(class.fields[0].visibility, Visibility::Public));
    }

    #[test]
    fn class_field_visibility_unknown_value_keeps_default_private() {
        // Guard against typo drift — anything other than "public" (case-
        // insensitive) stays private rather than silently upgrading.
        let spec = parse_class_spec(
            r#"{"name":"X","fields":[{"name":"f","type":"integer","visibility":"pub"}],"methods":[]}"#,
        ).unwrap();
        let class = class_to_ast(spec, Vec::new(), false).unwrap();
        assert!(matches!(class.fields[0].visibility, Visibility::Private));
    }

    // Prompt 17d864a6 — batch.methodCall bridges. Regression test that both
    // expression-form and statement-form MethodCall variants round-trip
    // through the JSON schema and lower to Expression::MethodCall (not
    // Expression::Call, which is a bare function call and doesn't type-check
    // for receiver.method(args) shapes).
    #[test]
    fn expr_method_call_lowers_to_expression_method_call() {
        let e = BatchExpr::MethodCall {
            receiver: Box::new(BatchExpr::Ident {
                name: "s".to_string(),
            }),
            method: "replace".to_string(),
            args: vec![
                BatchExpr::StringLit {
                    value: "old".to_string(),
                },
                BatchExpr::StringLit {
                    value: "new".to_string(),
                },
            ],
        };
        let ast = expr_to_ast(e).unwrap();
        match ast {
            Expression::MethodCall {
                object,
                method,
                arguments,
                ..
            } => {
                assert!(matches!(*object, Expression::Variable(ref n) if n == "s"));
                assert_eq!(method, "replace");
                assert_eq!(arguments.len(), 2);
            }
            other => panic!("expected MethodCall, got {:?}", other),
        }
    }

    #[test]
    fn stmt_method_call_lowers_to_statement_expression_wrapping_method_call() {
        let s = BatchStatement::MethodCall {
            receiver: BatchExpr::Ident {
                name: "xs".to_string(),
            },
            method: "push".to_string(),
            args: vec![BatchExpr::IntLit { value: 42 }],
        };
        let ast = stmt_to_ast(s).unwrap();
        match ast {
            Statement::Expression {
                expr:
                    Expression::MethodCall {
                        object,
                        method,
                        arguments,
                        ..
                    },
                ..
            } => {
                assert!(matches!(*object, Expression::Variable(ref n) if n == "xs"));
                assert_eq!(method, "push");
                assert_eq!(arguments.len(), 1);
            }
            other => panic!("expected Statement::Expression(MethodCall), got {:?}", other),
        }
    }

    #[test]
    fn expr_method_call_parses_from_json_via_snake_case() {
        // Serde is configured with rename_all = "snake_case" on BatchExpr;
        // the plugin sends {"kind":"method_call", "receiver": ..., "method": ..., "args": [...]}
        // through the JSON path. Verify the JSON round-trip.
        let json = r#"{
            "functions": [{
                "name": "wrap",
                "params": [],
                "return_type": "void",
                "body": [{
                    "kind": "method_call",
                    "receiver": {"kind": "ident", "name": "xs"},
                    "method": "push",
                    "args": [{"kind": "int_lit", "value": 1}]
                }]
            }]
        }"#;
        let spec = parse_batch_spec(json).unwrap();
        let f = &spec.functions[0];
        let body = f.body.as_ref().expect("body was provided inline");
        assert_eq!(body.len(), 1);
        match &body[0] {
            BatchStatement::MethodCall {
                receiver,
                method,
                args,
            } => {
                assert!(matches!(receiver, BatchExpr::Ident { name } if name == "xs"));
                assert_eq!(method, "push");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected MethodCall, got {:?}", other),
        }
    }

    #[test]
    fn class_field_visibility_mixed_fields_get_independent_visibility() {
        let spec = parse_class_spec(
            r#"{"name":"Page","fields":[
                {"name":"id","type":"integer"},
                {"name":"slug","type":"string","visibility":"public"},
                {"name":"internal_flag","type":"boolean","visibility":"private"}
            ],"methods":[]}"#,
        )
        .unwrap();
        let class = class_to_ast(spec, Vec::new(), false).unwrap();
        assert_eq!(class.fields.len(), 3);
        assert!(matches!(class.fields[0].visibility, Visibility::Private));
        assert!(matches!(class.fields[1].visibility, Visibility::Public));
        assert!(matches!(class.fields[2].visibility, Visibility::Private));
    }
}
