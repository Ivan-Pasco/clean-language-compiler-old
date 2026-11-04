//! Host implementation of the Layer 2 JSON bridges for the wasmtime hosts
//! that live inside the compiler crate: the plugin sandbox
//! (`plugins/wasm_adapter.rs`) and the standalone `wasmtime_runner`.
//!
//! The bridges and their contracts are defined in
//! `foundation/spec/stdlib-reference.md` §8 and
//! `foundation/spec/platform/runtime-abi/v1.toml`; the memory layout of the
//! values they read and write is `foundation/spec/platform/BOXED_ANY_ABI.md`:
//!
//! | Bridge                   | WASM signature   | Behaviour |
//! |--------------------------|------------------|-----------|
//! | `_json_encode_v2`        | `(any) -> lp`    | compact RFC 8259 text, insertion order |
//! | `_json_encode_pretty_v2` | `(any) -> lp`    | same, 2-space indentation |
//! | `_json_decode_v2`        | `(ptr, len) -> any` | boxed-Any tree, or 0 on a parse failure |
//! | `_json_get`              | `(any, lp) -> any` | value at a dot-separated path, null-tag on a miss |
//!
//! Encoding an invalid boxed-Any traps (D5) instead of returning `"null"`.

use serde_json::Value;
use wasmtime::{Caller, Linker, Memory};

/// Allocates `size` bytes in the calling module's linear memory and returns
/// the address. Each host passes its own allocator: the runner calls the
/// module's `malloc` export, the plugin sandbox its shared bump heap.
pub type JsonAlloc<T> = fn(&mut Caller<'_, T>, i32) -> Result<i32, String>;

/// Integers with a magnitude up to 2^53 are boxed as `integer` (tag 1);
/// anything larger is boxed as `number` (tag 3) — BOXED_ANY_ABI §4 / D1.
const JS_SAFE_INTEGER: i64 = 1 << 53;

const TAG_NULL: i32 = 0;
const TAG_INTEGER: i32 = 1;
const TAG_BOOLEAN: i32 = 2;
const TAG_NUMBER: i32 = 3;
const TAG_STRING: i32 = 4;
const TAG_ARRAY: i32 = 5;
const TAG_OBJECT: i32 = 6;

/// Register `_json_encode_v2`, `_json_encode_pretty_v2`, `_json_decode_v2`
/// and `_json_get` on `linker`, allocating results with `alloc`.
pub fn register_json_bridges<T: 'static>(
    linker: &mut Linker<T>,
    alloc: JsonAlloc<T>,
) -> wasmtime::Result<()> {
    linker.func_wrap(
        "env",
        "_json_encode_v2",
        move |mut caller: Caller<'_, T>, boxed: i32| -> wasmtime::Result<i32> {
            let value = read_boxed_any(&mut caller, boxed)
                .map_err(|e| wasmtime::format_err!("_json_encode_v2: {e}"))?;
            let text = serde_json::to_string(&value)?;
            alloc_lp_string(&mut caller, alloc, &text)
                .map_err(|e| wasmtime::format_err!("_json_encode_v2: {e}"))
        },
    )?;
    linker.func_wrap(
        "env",
        "_json_encode_pretty_v2",
        move |mut caller: Caller<'_, T>, boxed: i32| -> wasmtime::Result<i32> {
            let value = read_boxed_any(&mut caller, boxed)
                .map_err(|e| wasmtime::format_err!("_json_encode_pretty_v2: {e}"))?;
            let text = serde_json::to_string_pretty(&value)?;
            alloc_lp_string(&mut caller, alloc, &text)
                .map_err(|e| wasmtime::format_err!("_json_encode_pretty_v2: {e}"))
        },
    )?;
    linker.func_wrap(
        "env",
        "_json_decode_v2",
        move |mut caller: Caller<'_, T>, ptr: i32, len: i32| -> wasmtime::Result<i32> {
            let Ok(text) = read_utf8(&mut caller, ptr, len) else {
                return Ok(0);
            };
            // A parse failure is the 0 sentinel (BOXED_ANY_ABI §6); the
            // compiler turns it into a raise or a null.
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                return Ok(0);
            };
            write_boxed_any(&mut caller, alloc, &value)
                .map_err(|e| wasmtime::format_err!("_json_decode_v2: {e}"))
        },
    )?;
    linker.func_wrap(
        "env",
        "_json_get",
        move |mut caller: Caller<'_, T>, source: i32, path: i32| -> wasmtime::Result<i32> {
            let path = read_lp_string(&mut caller, path)
                .map_err(|e| wasmtime::format_err!("_json_get: path: {e}"))?;
            let source = json_get_source(&mut caller, source)
                .map_err(|e| wasmtime::format_err!("_json_get: {e}"))?;
            let leaf = source.as_ref().and_then(|v| json_get_path(v, &path));
            write_boxed_any(&mut caller, alloc, leaf.unwrap_or(&Value::Null))
                .map_err(|e| wasmtime::format_err!("_json_get: {e}"))
        },
    )?;
    Ok(())
}

/// The value `_json_get` walks. A string source holds JSON text and is
/// parsed; any other value is used as it is. `None` when the source is
/// null or its text is not valid JSON — every path then misses.
fn json_get_source<T>(caller: &mut Caller<'_, T>, boxed: i32) -> Result<Option<Value>, String> {
    if boxed == 0 {
        return Ok(None);
    }
    match read_boxed_any(caller, boxed)? {
        Value::Null => Ok(None),
        Value::String(text) => Ok(serde_json::from_str(&text).ok()),
        other => Ok(Some(other)),
    }
}

/// Follow a dot-separated `path` through `value`. A segment selects an
/// object member by name, or an array element when it is a decimal index.
pub fn json_get_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(value, |current, segment| match current {
            Value::Object(map) => map.get(segment),
            Value::Array(items) => segment
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get(index)),
            _ => None,
        })
}

fn memory<T>(caller: &mut Caller<'_, T>) -> Result<Memory, String> {
    caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| "module exports no memory".to_string())
}

fn read_bytes<T>(caller: &mut Caller<'_, T>, ptr: i32, len: usize) -> Result<Vec<u8>, String> {
    let memory = memory(caller)?;
    let data = memory.data(&*caller);
    let start = usize::try_from(ptr).map_err(|_| format!("negative address {ptr}"))?;
    let end = start
        .checked_add(len)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| format!("read of {len} bytes at {start} is out of bounds"))?;
    Ok(data[start..end].to_vec())
}

fn write_bytes<T>(caller: &mut Caller<'_, T>, ptr: i32, bytes: &[u8]) -> Result<(), String> {
    let memory = memory(caller)?;
    let data = memory.data_mut(&mut *caller);
    let start = usize::try_from(ptr).map_err(|_| format!("negative address {ptr}"))?;
    let end = start
        .checked_add(bytes.len())
        .filter(|end| *end <= data.len())
        .ok_or_else(|| format!("write of {} bytes at {start} is out of bounds", bytes.len()))?;
    data[start..end].copy_from_slice(bytes);
    Ok(())
}

fn read_i32<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<i32, String> {
    let bytes = read_bytes(caller, ptr, 4)?;
    Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_8_bytes<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<[u8; 8], String> {
    let bytes = read_bytes(caller, ptr, 8)?;
    let mut out = [0u8; 8];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn write_i32<T>(caller: &mut Caller<'_, T>, ptr: i32, value: i32) -> Result<(), String> {
    write_bytes(caller, ptr, &value.to_le_bytes())
}

fn read_utf8<T>(caller: &mut Caller<'_, T>, ptr: i32, len: i32) -> Result<String, String> {
    let len = usize::try_from(len).map_err(|_| format!("negative length {len}"))?;
    String::from_utf8(read_bytes(caller, ptr, len)?).map_err(|e| format!("invalid UTF-8: {e}"))
}

/// Read a length-prefixed string (`[len:i32][bytes]`).
pub fn read_lp_string<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<String, String> {
    let len = read_i32(caller, ptr)?;
    read_utf8(caller, ptr + 4, len)
}

/// Allocate and write a length-prefixed string; returns its address.
pub fn alloc_lp_string<T>(
    caller: &mut Caller<'_, T>,
    alloc: JsonAlloc<T>,
    text: &str,
) -> Result<i32, String> {
    let bytes = text.as_bytes();
    let len = i32::try_from(bytes.len()).map_err(|_| "string too long".to_string())?;
    let ptr = alloc(caller, 4 + len)?;
    write_i32(caller, ptr, len)?;
    write_bytes(caller, ptr + 4, bytes)?;
    Ok(ptr)
}

/// Read the boxed-Any tree at `ptr` into a JSON value.
pub fn read_boxed_any<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<Value, String> {
    if ptr == 0 {
        return Err("null boxed-Any pointer".to_string());
    }
    let tag = read_i32(caller, ptr)?;
    match tag {
        TAG_NULL => Ok(Value::Null),
        TAG_INTEGER => Ok(Value::from(i64::from_le_bytes(read_8_bytes(
            caller,
            ptr + 4,
        )?))),
        TAG_BOOLEAN => Ok(Value::Bool(read_i32(caller, ptr + 4)? != 0)),
        TAG_NUMBER => {
            let number = f64::from_le_bytes(read_8_bytes(caller, ptr + 4)?);
            serde_json::Number::from_f64(number)
                .map(Value::Number)
                .ok_or_else(|| format!("{number} has no JSON representation"))
        }
        TAG_STRING => {
            let text = read_i32(caller, ptr + 4)?;
            Ok(Value::String(read_lp_string(caller, text)?))
        }
        TAG_ARRAY => {
            let array = read_i32(caller, ptr + 4)?;
            let count = read_i32(caller, array)?;
            (0..count)
                .map(|i| {
                    let element = read_i32(caller, array + 4 + i * 4)?;
                    read_boxed_any(caller, element)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        TAG_OBJECT => {
            let object = read_i32(caller, ptr + 4)?;
            let count = read_i32(caller, object)?;
            let mut map = serde_json::Map::new();
            for i in 0..count {
                let key = read_i32(caller, object + 4 + i * 8)?;
                let value = read_i32(caller, object + 8 + i * 8)?;
                let key = read_lp_string(caller, key)?;
                let value = read_boxed_any(caller, value)?;
                map.insert(key, value);
            }
            Ok(Value::Object(map))
        }
        other => Err(format!("invalid boxed-Any tag {other} at {ptr}")),
    }
}

/// Write `value` as a boxed-Any tree; returns the address of its 12-byte box.
pub fn write_boxed_any<T>(
    caller: &mut Caller<'_, T>,
    alloc: JsonAlloc<T>,
    value: &Value,
) -> Result<i32, String> {
    // Children first, so the box is written once its payload is known.
    let (tag, payload): (i32, [u8; 8]) = match value {
        Value::Null => (TAG_NULL, [0; 8]),
        Value::Bool(flag) => (TAG_BOOLEAN, widen(i32::from(*flag))),
        Value::Number(number) => match number.as_i64() {
            Some(int) if int.abs() <= JS_SAFE_INTEGER => (TAG_INTEGER, int.to_le_bytes()),
            _ => {
                let float = number
                    .as_f64()
                    .ok_or_else(|| format!("{number} has no f64 representation"))?;
                (TAG_NUMBER, float.to_le_bytes())
            }
        },
        Value::String(text) => (TAG_STRING, widen(alloc_lp_string(caller, alloc, text)?)),
        Value::Array(items) => {
            let elements = items
                .iter()
                .map(|item| write_boxed_any(caller, alloc, item))
                .collect::<Result<Vec<_>, _>>()?;
            let array = alloc(caller, 4 + 4 * elements.len() as i32)?;
            write_i32(caller, array, elements.len() as i32)?;
            for (i, element) in elements.iter().enumerate() {
                write_i32(caller, array + 4 + 4 * i as i32, *element)?;
            }
            (TAG_ARRAY, widen(array))
        }
        Value::Object(map) => {
            let mut entries = Vec::with_capacity(map.len());
            for (key, member) in map {
                let key = alloc_lp_string(caller, alloc, key)?;
                let member = write_boxed_any(caller, alloc, member)?;
                entries.push((key, member));
            }
            let object = alloc(caller, 4 + 8 * entries.len() as i32)?;
            write_i32(caller, object, entries.len() as i32)?;
            for (i, (key, member)) in entries.iter().enumerate() {
                write_i32(caller, object + 4 + 8 * i as i32, *key)?;
                write_i32(caller, object + 8 + 8 * i as i32, *member)?;
            }
            (TAG_OBJECT, widen(object))
        }
    };
    let boxed = alloc(caller, 12)?;
    write_i32(caller, boxed, tag)?;
    write_bytes(caller, boxed + 4, &payload)?;
    Ok(boxed)
}

/// An i32 payload word followed by the zero word BOXED_ANY_ABI §2 requires.
fn widen(word: i32) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&word.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::json_get_path;
    use serde_json::json;

    #[test]
    fn path_walks_objects_and_array_indices() {
        let doc = json!({"data": {"rows": [{"name": "Welcome"}, {"name": "Two"}]}});
        assert_eq!(json_get_path(&doc, "data.rows.1.name"), Some(&json!("Two")));
        assert_eq!(
            json_get_path(&doc, "data.rows.0"),
            Some(&json!({"name": "Welcome"}))
        );
    }

    #[test]
    fn path_misses_return_none() {
        let doc = json!({"items": ["a", "b"], "n": 1});
        assert_eq!(json_get_path(&doc, "items.2"), None);
        assert_eq!(json_get_path(&doc, "items.x"), None);
        assert_eq!(json_get_path(&doc, "n.child"), None);
        assert_eq!(json_get_path(&doc, "missing"), None);
    }

    #[test]
    fn numeric_segment_on_an_object_is_a_member_name() {
        let doc = json!({"0": "zero"});
        assert_eq!(json_get_path(&doc, "0"), Some(&json!("zero")));
    }
}
