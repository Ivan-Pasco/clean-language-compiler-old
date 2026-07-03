/// Verb-expression spec deserializer (typed-emission.md §3.17 Amendment 9).
///
/// This module deserializes the JSON spec consumed by the
/// `_emit_verb_expression` bridge. The spec describes the whole shape of an
/// ORM `_db_query(sql, params)` call: a list of `sql_parts` that are
/// assembled left-associatively into the SQL string expression, and a list
/// of `params` that becomes the runtime parameter array.
///
/// Node kinds per §3.17:
///   - `literal`     — plugin-authored string constant (allowed in sql_parts only)
///   - `ident`       — reference to a Clean variable in scope at the call site
///   - `from_source` — verbatim user-authored fragment parsed via §3.16's
///     parser with `origin_offset` translation of diagnostic spans
///
/// Only the top-level `kind = "db_query"` is currently accepted; other verb
/// shapes are reserved for future amendments.
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct VerbSpec {
    pub kind: String,
    #[serde(default)]
    pub sql_parts: Vec<SqlPart>,
    #[serde(default)]
    pub params: Vec<SqlPart>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SqlPart {
    Literal {
        value: String,
    },
    Ident {
        name: String,
    },
    FromSource {
        source: String,
        #[serde(default)]
        origin_offset: i64,
    },
}

#[derive(Debug)]
pub enum VerbSpecError {
    /// Malformed JSON, unknown node `kind` in `sql_parts` / `params`, or any
    /// other serde-detected structural error. Message is the raw serde error.
    Json(String),
    /// Top-level `kind` is not one of the accepted verb shapes.
    UnknownKind(String),
}

/// Parse and validate a verb spec JSON payload.
///
/// Enforces the top-level `kind` allow-list. Node-level structural errors
/// (unknown `kind` inside arrays, missing fields) surface as
/// `VerbSpecError::Json` from serde itself — that error message already
/// names the offending field, which is sufficient for the PLUGIN013
/// diagnostic emitted by the bridge.
pub fn parse_verb_spec(json: &str) -> Result<VerbSpec, VerbSpecError> {
    let spec: VerbSpec =
        serde_json::from_str(json).map_err(|e| VerbSpecError::Json(e.to_string()))?;
    if spec.kind != "db_query" {
        return Err(VerbSpecError::UnknownKind(spec.kind));
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_db_query() {
        let json = r#"{"kind":"db_query","sql_parts":[],"params":[]}"#;
        let s = parse_verb_spec(json).unwrap();
        assert_eq!(s.kind, "db_query");
        assert!(s.sql_parts.is_empty());
        assert!(s.params.is_empty());
    }

    #[test]
    fn parses_literal_and_ident_parts() {
        let json = r#"{
            "kind":"db_query",
            "sql_parts":[
                {"kind":"literal","value":"SELECT * FROM "},
                {"kind":"ident","name":"table"}
            ],
            "params":[]
        }"#;
        let s = parse_verb_spec(json).unwrap();
        assert_eq!(s.sql_parts.len(), 2);
        match &s.sql_parts[0] {
            SqlPart::Literal { value } => assert_eq!(value, "SELECT * FROM "),
            _ => panic!("expected literal"),
        }
        match &s.sql_parts[1] {
            SqlPart::Ident { name } => assert_eq!(name, "table"),
            _ => panic!("expected ident"),
        }
    }

    #[test]
    fn parses_from_source_with_origin_offset() {
        let json = r#"{
            "kind":"db_query",
            "sql_parts":[
                {"kind":"from_source","source":"a == b","origin_offset":123}
            ],
            "params":[]
        }"#;
        let s = parse_verb_spec(json).unwrap();
        match &s.sql_parts[0] {
            SqlPart::FromSource {
                source,
                origin_offset,
            } => {
                assert_eq!(source, "a == b");
                assert_eq!(*origin_offset, 123);
            }
            _ => panic!("expected from_source"),
        }
    }

    #[test]
    fn from_source_default_origin_offset_is_zero() {
        let json = r#"{
            "kind":"db_query",
            "sql_parts":[{"kind":"from_source","source":"x"}],
            "params":[]
        }"#;
        let s = parse_verb_spec(json).unwrap();
        match &s.sql_parts[0] {
            SqlPart::FromSource { origin_offset, .. } => assert_eq!(*origin_offset, 0),
            _ => panic!("expected from_source"),
        }
    }

    #[test]
    fn malformed_json_returns_json_error() {
        let e = parse_verb_spec("not json").unwrap_err();
        match e {
            VerbSpecError::Json(_) => {}
            other => panic!("expected Json error, got {:?}", other),
        }
    }

    #[test]
    fn unknown_top_level_kind_returns_unknown_kind() {
        let json = r#"{"kind":"other","sql_parts":[],"params":[]}"#;
        let e = parse_verb_spec(json).unwrap_err();
        match e {
            VerbSpecError::UnknownKind(k) => assert_eq!(k, "other"),
            other => panic!("expected UnknownKind, got {:?}", other),
        }
    }

    #[test]
    fn unknown_node_kind_in_sql_parts_is_json_error() {
        let json = r#"{
            "kind":"db_query",
            "sql_parts":[{"kind":"bogus","value":"x"}],
            "params":[]
        }"#;
        let e = parse_verb_spec(json).unwrap_err();
        match e {
            VerbSpecError::Json(_) => {}
            other => panic!("expected Json error, got {:?}", other),
        }
    }
}
