//! Stub host implementation of the Delivery-2 JSON bridges
//! (`_json_encode_v2` / `_json_encode_pretty_v2` / `_json_decode_v2` /
//! `_json_get`) for standalone integration tests.
//!
//! These stubs implement the [BOXED_ANY_ABI] boxed-Any layout in a
//! wasmtime `Linker` so tests that compile Clean programs with
//! `--enable-json-bridge` can be exercised end-to-end without needing a
//! full clean-server or clean-framework host. The implementation is
//! deliberately minimal — enough to cover the JSON coverage suite in
//! `tests/cln/stdlib/json/` and the assertions in
//! `tests/test_json_bridge_runtime.rs`.
//!
//! Layout reference (foundation/spec/platform/BOXED_ANY_ABI.md):
//!   * 12-byte boxed-Any block: [tag@0:i32][value1@4:i32][value2@8:i32]
//!   * Tag 0 (Null), 1 (Integer, i64 split), 2 (Boolean), 3 (Number, f64
//!     split), 4 (String — LP ptr at offset 4), 5 (Array — JSON-tree ptr),
//!     6 (Object — JSON-tree ptr).
//!   * JSON-tree Array: [count:i32][boxed_ptr:i32]…  (stride 4)
//!   * JSON-tree Object: [count:i32](key_lp_ptr:i32, val_boxed_ptr:i32)…
//!
//! The stubs use the compiled WASM module's exported `malloc` for
//! allocation so the memory layout stays coherent with the module's own
//! allocations. They rely on serde_json for the RFC 8259 grammar.
//!
//! [BOXED_ANY_ABI]: ../../../foundation/spec/platform/BOXED_ANY_ABI.md

#![allow(dead_code)] // test-only helpers; not all tests exercise every path

use serde_json::Value;
use wasmtime::{Caller, Extern, Linker, Val};

// ---------------------------------------------------------------------------
// Memory helpers
// ---------------------------------------------------------------------------

fn memory<T>(caller: &mut Caller<'_, T>) -> Option<wasmtime::Memory> {
    match caller.get_export("memory") {
        Some(Extern::Memory(mem)) => Some(mem),
        _ => None,
    }
}

fn malloc<T>(caller: &mut Caller<'_, T>, size: i32) -> Result<i32, String> {
    let malloc_fn = caller
        .get_export("malloc")
        .and_then(|e| match e {
            Extern::Func(f) => Some(f),
            _ => None,
        })
        .ok_or_else(|| "no malloc export".to_string())?;
    let typed = malloc_fn
        .typed::<i32, i32>(&caller)
        .map_err(|e| format!("malloc type mismatch: {e}"))?;
    typed
        .call(&mut *caller, size)
        .map_err(|e| format!("malloc call failed: {e}"))
}

fn read_i32<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<i32, String> {
    let mem = memory(caller).ok_or_else(|| "no memory".to_string())?;
    let data = mem.data(&caller);
    let p = ptr as usize;
    if p + 4 > data.len() {
        return Err(format!("read_i32: out of bounds at {p}"));
    }
    let bytes = [data[p], data[p + 1], data[p + 2], data[p + 3]];
    Ok(i32::from_le_bytes(bytes))
}

fn read_f64<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<f64, String> {
    let mem = memory(caller).ok_or_else(|| "no memory".to_string())?;
    let data = mem.data(&caller);
    let p = ptr as usize;
    if p + 8 > data.len() {
        return Err(format!("read_f64: out of bounds at {p}"));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[p..p + 8]);
    Ok(f64::from_le_bytes(bytes))
}

fn read_i64<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<i64, String> {
    let mem = memory(caller).ok_or_else(|| "no memory".to_string())?;
    let data = mem.data(&caller);
    let p = ptr as usize;
    if p + 8 > data.len() {
        return Err(format!("read_i64: out of bounds at {p}"));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[p..p + 8]);
    Ok(i64::from_le_bytes(bytes))
}

fn write_i32<T>(caller: &mut Caller<'_, T>, ptr: i32, v: i32) -> Result<(), String> {
    let mem = memory(caller).ok_or_else(|| "no memory".to_string())?;
    let data = mem.data_mut(&mut *caller);
    let p = ptr as usize;
    if p + 4 > data.len() {
        return Err(format!("write_i32: out of bounds at {p}"));
    }
    data[p..p + 4].copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn write_f64<T>(caller: &mut Caller<'_, T>, ptr: i32, v: f64) -> Result<(), String> {
    let mem = memory(caller).ok_or_else(|| "no memory".to_string())?;
    let data = mem.data_mut(&mut *caller);
    let p = ptr as usize;
    if p + 8 > data.len() {
        return Err(format!("write_f64: out of bounds at {p}"));
    }
    data[p..p + 8].copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn read_lp_string<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<String, String> {
    let len = read_i32(caller, ptr)?;
    if len < 0 {
        return Err(format!("negative LP string length {len}"));
    }
    let mem = memory(caller).ok_or_else(|| "no memory".to_string())?;
    let data = mem.data(&caller);
    let start = (ptr as usize) + 4;
    let end = start + len as usize;
    if end > data.len() {
        return Err(format!("read_lp_string: OOB at {start}..{end}"));
    }
    String::from_utf8(data[start..end].to_vec())
        .map_err(|e| format!("LP string not UTF-8: {e}"))
}

fn alloc_lp_string<T>(caller: &mut Caller<'_, T>, s: &str) -> Result<i32, String> {
    let bytes = s.as_bytes();
    let size = 4 + bytes.len() as i32;
    let ptr = malloc(caller, size)?;
    write_i32(caller, ptr, bytes.len() as i32)?;
    // Write bytes
    let mem = memory(caller).ok_or_else(|| "no memory".to_string())?;
    let data = mem.data_mut(&mut *caller);
    let start = (ptr as usize) + 4;
    let end = start + bytes.len();
    if end > data.len() {
        return Err(format!("alloc_lp_string: OOB at {start}..{end}"));
    }
    data[start..end].copy_from_slice(bytes);
    Ok(ptr)
}

// ---------------------------------------------------------------------------
// Boxed-Any read (WASM memory → serde_json::Value)
// ---------------------------------------------------------------------------

fn read_boxed_any<T>(caller: &mut Caller<'_, T>, ptr: i32) -> Result<Value, String> {
    if ptr == 0 {
        return Err("null boxed-Any pointer".to_string());
    }
    let tag = read_i32(caller, ptr)?;
    match tag {
        0 => Ok(Value::Null),
        1 => {
            let n = read_i64(caller, ptr + 4)?;
            Ok(Value::from(n))
        }
        2 => {
            let b = read_i32(caller, ptr + 4)?;
            Ok(Value::Bool(b != 0))
        }
        3 => {
            let f = read_f64(caller, ptr + 4)?;
            serde_json::Number::from_f64(f)
                .map(Value::Number)
                .ok_or_else(|| format!("non-finite number {f}"))
        }
        4 => {
            let sp = read_i32(caller, ptr + 4)?;
            let s = read_lp_string(caller, sp)?;
            Ok(Value::String(s))
        }
        // (Rust-borrow: keep the two reads on separate lines so caller isn't
        // borrowed twice within the same expression.)
        5 => {
            let arr_ptr = read_i32(caller, ptr + 4)?;
            let count = read_i32(caller, arr_ptr)?;
            let mut out = Vec::with_capacity(count as usize);
            for i in 0..count {
                let child_ptr = read_i32(caller, arr_ptr + 4 + i * 4)?;
                out.push(read_boxed_any(caller, child_ptr)?);
            }
            Ok(Value::Array(out))
        }
        6 => {
            let obj_ptr = read_i32(caller, ptr + 4)?;
            let count = read_i32(caller, obj_ptr)?;
            let mut out = serde_json::Map::with_capacity(count as usize);
            for i in 0..count {
                let key_ptr = read_i32(caller, obj_ptr + 4 + i * 8)?;
                let val_ptr = read_i32(caller, obj_ptr + 4 + i * 8 + 4)?;
                let key = read_lp_string(caller, key_ptr)?;
                let val = read_boxed_any(caller, val_ptr)?;
                out.insert(key, val);
            }
            Ok(Value::Object(out))
        }
        _ => Err(format!("invalid boxed-Any tag {tag} at ptr {ptr}")),
    }
}

// ---------------------------------------------------------------------------
// Boxed-Any write (serde_json::Value → WASM memory)
// ---------------------------------------------------------------------------

fn write_boxed_any<T>(caller: &mut Caller<'_, T>, value: &Value) -> Result<i32, String> {
    let ptr = malloc(caller, 12)?;
    // Zero the value slots by default.
    write_i32(caller, ptr + 4, 0)?;
    write_i32(caller, ptr + 8, 0)?;
    match value {
        Value::Null => {
            write_i32(caller, ptr, 0)?;
        }
        Value::Bool(b) => {
            write_i32(caller, ptr, 2)?;
            write_i32(caller, ptr + 4, if *b { 1 } else { 0 })?;
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                write_i32(caller, ptr, 1)?;
                // Split i64 across offsets 4 and 8 (little-endian).
                let lo = (i & 0xFFFF_FFFF) as i32;
                let hi = ((i >> 32) & 0xFFFF_FFFF) as i32;
                write_i32(caller, ptr + 4, lo)?;
                write_i32(caller, ptr + 8, hi)?;
            } else if let Some(f) = n.as_f64() {
                write_i32(caller, ptr, 3)?;
                write_f64(caller, ptr + 4, f)?;
            } else {
                return Err(format!("unrepresentable number {n}"));
            }
        }
        Value::String(s) => {
            write_i32(caller, ptr, 4)?;
            let sp = alloc_lp_string(caller, s)?;
            write_i32(caller, ptr + 4, sp)?;
        }
        Value::Array(items) => {
            // JSON-tree Array: [count][boxed_ptr]…
            let count = items.len() as i32;
            let arr_ptr = malloc(caller, 4 + count * 4)?;
            write_i32(caller, arr_ptr, count)?;
            for (i, item) in items.iter().enumerate() {
                let child = write_boxed_any(caller, item)?;
                write_i32(caller, arr_ptr + 4 + (i as i32) * 4, child)?;
            }
            write_i32(caller, ptr, 5)?;
            write_i32(caller, ptr + 4, arr_ptr)?;
        }
        Value::Object(entries) => {
            // JSON-tree Object: [count](key_lp, val_boxed)…
            let count = entries.len() as i32;
            let obj_ptr = malloc(caller, 4 + count * 8)?;
            write_i32(caller, obj_ptr, count)?;
            for (i, (k, v)) in entries.iter().enumerate() {
                let key_ptr = alloc_lp_string(caller, k)?;
                let val_ptr = write_boxed_any(caller, v)?;
                write_i32(caller, obj_ptr + 4 + (i as i32) * 8, key_ptr)?;
                write_i32(caller, obj_ptr + 4 + (i as i32) * 8 + 4, val_ptr)?;
            }
            write_i32(caller, ptr, 6)?;
            write_i32(caller, ptr + 4, obj_ptr)?;
        }
    }
    Ok(ptr)
}

// ---------------------------------------------------------------------------
// Linker registration
// ---------------------------------------------------------------------------

/// Register the four Delivery-2 JSON bridges on the given linker.
///
/// The store type parameter `T` is left generic so tests can bring their own
/// state type. All four bridges are pure host-side pass-throughs that use
/// serde_json for the RFC 8259 grammar and the module's own `malloc` export
/// for memory allocation.
///
/// # Panics
///
/// Registration failures panic (they represent test-setup misuse, not
/// runtime errors).
pub fn register_json_v2_bridges<T: 'static>(linker: &mut Linker<T>) {
    // _json_encode_v2(boxed_any_ptr: i32) -> i32 (LP string ptr)
    linker
        .func_wrap(
            "env",
            "_json_encode_v2",
            |mut caller: Caller<'_, T>, ptr: i32| -> i32 {
                let value = match read_boxed_any(&mut caller, ptr) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let s = value.to_string();
                alloc_lp_string(&mut caller, &s).unwrap_or(0)
            },
        )
        .expect("register _json_encode_v2");

    // _json_encode_pretty_v2(boxed_any_ptr: i32) -> i32 (LP string ptr)
    linker
        .func_wrap(
            "env",
            "_json_encode_pretty_v2",
            |mut caller: Caller<'_, T>, ptr: i32| -> i32 {
                let value = match read_boxed_any(&mut caller, ptr) {
                    Ok(v) => v,
                    Err(_) => return 0,
                };
                let s = serde_json::to_string_pretty(&value).unwrap_or_default();
                alloc_lp_string(&mut caller, &s).unwrap_or(0)
            },
        )
        .expect("register _json_encode_pretty_v2");

    // _json_decode_v2(text_lp_ptr: i32) -> i32 (boxed-Any ptr, 0 on parse fail)
    linker
        .func_wrap(
            "env",
            "_json_decode_v2",
            |mut caller: Caller<'_, T>, text_ptr: i32| -> i32 {
                let text = match read_lp_string(&mut caller, text_ptr) {
                    Ok(t) => t,
                    Err(_) => return 0,
                };
                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => return 0, // sentinel per D5
                };
                write_boxed_any(&mut caller, &value).unwrap_or(0)
            },
        )
        .expect("register _json_decode_v2");

    // _json_get(any: i32, path_lp_ptr: i32) -> i32 (boxed-Any ptr or null-tag)
    // Minimal path traversal: dot-separated keys and integer indices. Returns
    // a fresh null-tag boxed-Any on miss (matches the compiler-stdlib
    // semantics used by the pure-WASM json.get).
    linker
        .func_wrap(
            "env",
            "_json_get",
            |mut caller: Caller<'_, T>, any_ptr: i32, path_ptr: i32| -> i32 {
                let path = match read_lp_string(&mut caller, path_ptr) {
                    Ok(p) => p,
                    Err(_) => return alloc_null_boxed(&mut caller),
                };
                let mut value = match read_boxed_any(&mut caller, any_ptr) {
                    Ok(v) => v,
                    Err(_) => return alloc_null_boxed(&mut caller),
                };
                // If the root any is actually a JSON string, auto-parse it (RUNTIME002).
                if let Value::String(s) = &value {
                    if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                        value = parsed;
                    }
                }
                for seg in path.split('.') {
                    match &value {
                        Value::Object(map) => {
                            value = map.get(seg).cloned().unwrap_or(Value::Null);
                        }
                        Value::Array(arr) => match seg.parse::<usize>() {
                            Ok(i) if i < arr.len() => value = arr[i].clone(),
                            _ => value = Value::Null,
                        },
                        _ => value = Value::Null,
                    }
                    if matches!(value, Value::Null) {
                        break;
                    }
                }
                write_boxed_any(&mut caller, &value).unwrap_or_else(|_| alloc_null_boxed(&mut caller))
            },
        )
        .expect("register _json_get");
}

fn alloc_null_boxed<T>(caller: &mut Caller<'_, T>) -> i32 {
    let ptr = match malloc(caller, 12) {
        Ok(p) => p,
        Err(_) => return 0,
    };
    let _ = write_i32(caller, ptr, 0);
    let _ = write_i32(caller, ptr + 4, 0);
    let _ = write_i32(caller, ptr + 8, 0);
    ptr
}

// Silence unused-import warning when a specific test only consumes a subset.
#[allow(unused_imports)]
use Val as _WasmVal;
