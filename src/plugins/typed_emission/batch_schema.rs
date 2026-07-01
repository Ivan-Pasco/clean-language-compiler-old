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
    pub body: Vec<BatchStatement>,
}

#[derive(Debug, Deserialize)]
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
}

#[derive(Debug, Deserialize)]
pub struct BatchField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
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

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BatchStatement {
    Call {
        callee: String,
        #[serde(default)]
        args: Vec<BatchExpr>,
    },
    Assign {
        target: String,
        expr: BatchExpr,
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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
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
            let mut c = 1usize;
            let mut byte_within = 0usize;
            for lc in source[off..].chars() {
                if c == col {
                    return Some(off + byte_within);
                }
                if lc == '\n' {
                    break;
                }
                byte_within += lc.len_utf8();
                c += 1;
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
        _ => {
            // Array<T> → List(resolve(T))
            if let Some(rest) = name.strip_prefix("Array<") {
                if let Some(inner) = rest.strip_suffix('>') {
                    let elem = resolve_type(inner)?;
                    return Ok(Type::List(
                        Box::new(elem),
                        crate::ast::ListBehavior::Default,
                    ));
                }
                return Err(BatchSchemaError::UnresolvableType(name.to_string()));
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
        BatchStatement::Assign { target, expr } => Statement::Assignment {
            target: AssignmentTarget::Variable(target),
            value: expr_to_ast(expr)?,
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
pub fn function_to_ast(f: BatchFunction, exported: bool) -> Result<Function, BatchSchemaError> {
    let mut params = Vec::with_capacity(f.params.len());
    for p in f.params {
        params.push(Parameter::new(p.name, resolve_type(&p.ty)?));
    }
    let return_type = resolve_type(&f.return_type)?;
    let body: Result<Vec<_>, _> = f.body.into_iter().map(stmt_to_ast).collect();
    let mut func = Function::new(f.name, params, return_type, body?, None);
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

    for field in spec.fields {
        let ty = resolve_type(&field.ty)?;
        class.fields.push(Field::new(field.name, ty));
    }

    for (m, body) in spec.methods.into_iter().zip(method_bodies.into_iter()) {
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
}
