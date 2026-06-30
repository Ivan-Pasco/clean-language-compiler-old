/// JSON helpers for decoding LP-string payloads in the typed-emission bridges.
///
/// All incoming JSON uses `serde_json` — already a direct Cargo dependency.
/// These helpers decode the structured payloads that the plugin passes for
/// multi-value slots (parameter lists, statement lists, argument lists, etc.).
use anyhow::{anyhow, Result};
use serde::Deserialize;

/// Deserialise a JSON array of `u32` (handle values) from a UTF-8 slice.
///
/// Used by `_stmt_block`, `_expr_array_lit`, and any other bridge that
/// accepts `stmts_json_lp` / `elems_json_lp` = `[handle, ...]`.
pub fn decode_handle_array(json: &str) -> Result<Vec<i32>> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(Vec::new());
    }
    let raw: Vec<serde_json::Value> = serde_json::from_str(trimmed).map_err(|e| {
        anyhow!(
            "typed-emission: invalid handle array JSON `{}`: {}",
            trimmed,
            e
        )
    })?;
    raw.iter()
        .map(|v| {
            v.as_i64()
                .ok_or_else(|| anyhow!("typed-emission: handle must be integer, got {:?}", v))
                .map(|n| n as i32)
        })
        .collect()
}

/// One parameter descriptor as passed in `params_json_lp`.
///
/// ```json
/// [{"name":"x","type_handle":3},{"name":"y","type_handle":4,"default_expr_handle":5}]
/// ```
#[derive(Deserialize, Debug)]
pub struct ParamDesc {
    pub name: String,
    pub type_handle: i32,
    #[serde(default)]
    pub default_expr_handle: i32,
}

/// Deserialise `params_json_lp` into a `Vec<ParamDesc>`.
pub fn decode_param_array(json: &str) -> Result<Vec<ParamDesc>> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(Vec::new());
    }
    serde_json::from_str(trimmed)
        .map_err(|e| anyhow!("typed-emission: invalid params JSON `{}`: {}", trimmed, e))
}

/// One field descriptor as passed inside `class_json_lp`.
#[derive(Deserialize, Debug)]
pub struct FieldDesc {
    pub name: String,
    pub type_handle: i32,
    #[serde(default)]
    pub default_expr_handle: i32,
}

/// One method descriptor inside `class_json_lp`.
#[derive(Deserialize, Debug)]
pub struct MethodDesc {
    pub name: String,
    pub function_handle: i32,
}

/// Full class descriptor passed to `_emit_class`.
///
/// ```json
/// {"parent":"Base","fields":[...],"methods":[...]}
/// ```
#[derive(Deserialize, Debug)]
pub struct ClassDesc {
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub fields: Vec<FieldDesc>,
    #[serde(default)]
    pub methods: Vec<MethodDesc>,
}

/// Deserialise `class_json_lp` into a `ClassDesc`.
pub fn decode_class_desc(json: &str) -> Result<ClassDesc> {
    serde_json::from_str(json.trim())
        .map_err(|e| anyhow!("typed-emission: invalid class JSON `{}`: {}", json, e))
}

/// One key-value pair in an object literal, passed in `_expr_object_lit`.
#[derive(Deserialize, Debug)]
pub struct ObjectField {
    pub key: String,
    pub value_handle: i32,
}

/// Deserialise `fields_json_lp` for `_expr_object_lit`.
pub fn decode_object_fields(json: &str) -> Result<Vec<ObjectField>> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(Vec::new());
    }
    serde_json::from_str(trimmed).map_err(|e| {
        anyhow!(
            "typed-emission: invalid object fields JSON `{}`: {}",
            trimmed,
            e
        )
    })
}

/// State fields descriptor passed to `_emit_state_block`.
/// Each entry is `{"name":"x","type_handle":N,"default_expr_handle":M?}`.
pub use ParamDesc as StateFieldDesc;

pub fn decode_state_fields(json: &str) -> Result<Vec<StateFieldDesc>> {
    decode_param_array(json)
}
