//! Runtime ABI v1 schema — parity-verified against `wasm_adapter.rs::setup_linker`.
//!
//! This module embeds `runtime-abi-v1.toml` at build time (via `include_str!`)
//! and exposes the parsed bridge catalog as the canonical enumeration of the
//! host bridges the compiler registers for the plugin sandbox.
//!
//! The TOML file is a *copy* of
//! `foundation/platform-architecture/runtime-abi/v1.toml` bundled inside the
//! compiler crate so CI (which checks out only the compiler repo, not the
//! foundation subtree) can build. The foundation-side TOML is the authoritative
//! spec artifact; the copy in this directory must stay in sync — a mismatch is
//! caught by the parity gate below and by the manual audit protocol documented
//! in the TOML header.
//!
//! # Authority (post Phase D S5)
//!
//! `v1.toml` is authoritative. Any bridge added to `wasm_adapter.rs` MUST be
//! added to `v1.toml` first. `setup_linker` calls
//! [`verify_registrations_against_schema`] at the end of registration and
//! fails plugin adapter construction if the two disagree.
//!
//! # Non-goals
//!
//! - This module does NOT drive registration. Closures remain hand-written
//!   in `wasm_adapter.rs` to preserve exact allocation semantics (many
//!   "stub" bridges must allocate placeholder memory so plugins reading
//!   returned pointers as length-prefixed strings do not crash).
//! - This module does NOT verify signature shape; only presence of every
//!   catalogued (module, name) pair. Signature drift would fail at plugin
//!   instantiation time as "unknown import" or "signature mismatch" — that
//!   is the wasmtime linker's responsibility, not ours.

use serde::Deserialize;

/// Embedded catalog. Copy of
/// `foundation/platform-architecture/runtime-abi/v1.toml`; foundation-side
/// TOML is authoritative. Keep the two in sync when adding bridges.
const V1_TOML_SRC: &str = include_str!("runtime-abi-v1.toml");

#[derive(Debug, Deserialize)]
struct RuntimeAbiV1 {
    #[allow(dead_code)]
    version: String,
    #[allow(dead_code)]
    status: String,
    bridge: Vec<BridgeEntry>,
}

#[derive(Debug, Deserialize)]
struct BridgeEntry {
    name: String,
    module: String,
    // Remaining fields (params, returns, host_class, semantics, source, notes)
    // are documentation-only for the verification path and intentionally not
    // deserialized here. Signature enforcement is delegated to wasmtime's
    // linker at plugin instantiation time.
}

/// Parses `v1.toml` and returns the catalog of `(module, name)` pairs the
/// compiler is expected to register.
///
/// Panics if the embedded TOML fails to parse — that would indicate the
/// spec artifact drifted from schema, which is a compile-time invariant
/// this module exists to enforce.
pub(crate) fn expected_bridges() -> Vec<(String, String)> {
    let doc: RuntimeAbiV1 = toml::from_str(V1_TOML_SRC)
        .expect("foundation/platform-architecture/runtime-abi/v1.toml failed to parse");
    doc.bridge.into_iter().map(|b| (b.module, b.name)).collect()
}

/// Verify that every `(module, name)` in `v1.toml` appears in `registered`.
/// Also verifies the total count matches — extra registrations not in the
/// TOML are a spec violation just as missing ones are.
///
/// Returns `Err` with a human-readable diagnostic on any mismatch. This is
/// the parity gate enforcing the "150 bridges must equal 150" invariant
/// documented at the top of `v1.toml`.
pub(crate) fn verify_registrations_against_schema(
    registered: &[(String, String)],
) -> Result<(), String> {
    let expected = expected_bridges();

    if expected.len() != registered.len() {
        return Err(format!(
            "runtime-abi/v1.toml parity mismatch: schema catalogues {} bridges, \
             wasm_adapter.rs registered {}. Add/remove entries so the two agree.",
            expected.len(),
            registered.len()
        ));
    }

    let mut missing = Vec::new();
    for (module, name) in &expected {
        if !registered.iter().any(|(m, n)| m == module && n == name) {
            missing.push(format!("{}::{}", module, name));
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "runtime-abi/v1.toml catalogues bridges not registered in \
             wasm_adapter.rs::setup_linker: {}",
            missing.join(", ")
        ));
    }

    let mut extra = Vec::new();
    for (module, name) in registered {
        if !expected.iter().any(|(m, n)| m == module && n == name) {
            extra.push(format!("{}::{}", module, name));
        }
    }
    if !extra.is_empty() {
        return Err(format!(
            "wasm_adapter.rs::setup_linker registers bridges not catalogued in \
             runtime-abi/v1.toml: {}",
            extra.join(", ")
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_toml_parses_and_has_expected_entries() {
        let bridges = expected_bridges();
        // Bumped from 150 → 152 on 2026-07-14 for the bytes-handle bridges
        // (_req_body_bytes, _fs_write_bytes) that unblock the errors
        // dashboard's tarball-upload endpoint. See spec/type-system.md §9b
        // and spec/plugins/frame-server.ebnf (req_body_bytes, fs_write_bytes).
        // Bumped 152 → 153 for _crypto_sha256_bytes, the third bridge in the
        // opaque-handle triad (_req_body_bytes → _crypto_sha256_bytes →
        // _fs_write_bytes) needed by tarball-upload SHA-256 verification.
        // Bumped 153 → 154 for _dev_snapshot (Layer 3, server-only), the
        // dev-mode capture bridge implemented in clean-server::dev_capture and
        // consumed by framework's /_debug/capture handler + errors dashboard's
        // retest sandbox. Framework 2.9.1 ships with emit_dev_capture disabled
        // until cln recognises the import; this bump unblocks 2.9.2.
        assert_eq!(
            bridges.len(),
            154,
            "v1.toml must catalogue exactly 154 bridges (matches \
             `grep -c 'func_wrap(' src/plugins/wasm_adapter.rs` at authoring \
             time). Update this assertion AND wasm_adapter.rs together when \
             adding entries."
        );
    }

    #[test]
    fn verify_reports_missing() {
        // Simulate a registration set missing one entry: must fail.
        let mut registered = expected_bridges();
        registered.pop();
        let err = verify_registrations_against_schema(&registered)
            .expect_err("expected parity failure when a bridge is missing");
        assert!(err.contains("parity mismatch") || err.contains("not registered"));
    }

    #[test]
    fn verify_reports_extra() {
        let mut registered = expected_bridges();
        registered.push(("env".to_string(), "__does_not_exist".to_string()));
        let err = verify_registrations_against_schema(&registered)
            .expect_err("expected parity failure when an unlisted bridge is registered");
        assert!(err.contains("parity mismatch") || err.contains("not catalogued"));
    }

    #[test]
    fn verify_accepts_exact_match() {
        let registered = expected_bridges();
        verify_registrations_against_schema(&registered).expect("exact match must verify clean");
    }
}
