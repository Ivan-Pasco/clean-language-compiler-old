//! Build script for the Clean Language compiler.
//!
//! Two responsibilities:
//!
//! 1. **Function-registry vendor**: copies
//!    `foundation/platform-architecture/function-registry.toml` into
//!    `src/plugins/function-registry.toml` when the sibling foundation
//!    workspace is present, so `include_str!` always picks up edits during
//!    local development.
//!
//! 2. **Typed-emission op-code hash**: computes the SHA-256 of
//!    `foundation/spec/plugins/contracts/typed-emission-ops.toml` and emits
//!    it as the `EMISSION_OPS_HASH` environment variable consumed by
//!    `src/plugins/plugin_abi.rs`. When the foundation copy is absent (CI
//!    without the spec workspace), the hash of the vendored fallback copy at
//!    `src/plugins/typed-emission-ops.toml` is used. If neither exists the
//!    hash is the all-zeros sentinel ("0000…0000") and the loader treats the
//!    field as absent (warn-only until --strict-emission-ops is set).
//!
//! See:
//!   - foundation/spec/plugins/contracts/typed-emission.md §3.10
//!   - foundation/spec/plugins/contracts/typed-emission-ops.toml

use std::path::PathBuf;

/// Compute the lowercase-hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    use std::num::Wrapping;

    // Pure-Rust SHA-256 so the build script has zero extra dependencies.
    // Reference: FIPS 180-4.
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [Wrapping<u32>; 8] = [
        Wrapping(0x6a09e667u32),
        Wrapping(0xbb67ae85),
        Wrapping(0x3c6ef372),
        Wrapping(0xa54ff53a),
        Wrapping(0x510e527f),
        Wrapping(0x9b05688c),
        Wrapping(0x1f83d9ab),
        Wrapping(0x5be0cd19),
    ];

    // Pre-processing: pad the message.
    let bit_len = (bytes.len() as u64) * 8;
    let mut msg = bytes.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 64-byte block.
    for block in msg.chunks(64) {
        let mut w = [Wrapping(0u32); 64];
        for (i, chunk) in block.chunks(4).enumerate().take(16) {
            w[i] = Wrapping(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        for i in 16..64 {
            let s0 =
                w[i - 15].0.rotate_right(7) ^ w[i - 15].0.rotate_right(18) ^ (w[i - 15].0 >> 3);
            let s1 = w[i - 2].0.rotate_right(17) ^ w[i - 2].0.rotate_right(19) ^ (w[i - 2].0 >> 10);
            w[i] = w[i - 16] + Wrapping(s0) + w[i - 7] + Wrapping(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.0.rotate_right(6) ^ e.0.rotate_right(11) ^ e.0.rotate_right(25);
            let ch = (e.0 & f.0) ^ (!e.0 & g.0);
            let temp1 = hh + Wrapping(s1) + Wrapping(ch) + Wrapping(K[i]) + w[i];
            let s0 = a.0.rotate_right(2) ^ a.0.rotate_right(13) ^ a.0.rotate_right(22);
            let maj = (a.0 & b.0) ^ (a.0 & c.0) ^ (b.0 & c.0);
            let temp2 = Wrapping(s0) + Wrapping(maj);

            hh = g;
            g = f;
            f = e;
            e = d + temp1;
            d = c;
            c = b;
            b = a;
            a = temp1 + temp2;
        }
        h[0] += a;
        h[1] += b;
        h[2] += c;
        h[3] += d;
        h[4] += e;
        h[5] += f;
        h[6] += g;
        h[7] += hh;
    }

    format!(
        "{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}{:08x}",
        h[0].0, h[1].0, h[2].0, h[3].0, h[4].0, h[5].0, h[6].0, h[7].0
    )
}

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // ── 1. Function-registry vendor ──────────────────────────────────────────
    let foundation_registry = manifest_dir
        .parent()
        .map(|p| p.join("foundation/platform-architecture/function-registry.toml"));
    let vendored_registry = manifest_dir.join("src/plugins/function-registry.toml");

    if let Some(src) = foundation_registry {
        if src.is_file() {
            let bytes = std::fs::read(&src)
                .unwrap_or_else(|e| panic!("read foundation registry at {}: {e}", src.display()));
            // Avoid spurious rebuilds when content is unchanged.
            let existing = std::fs::read(&vendored_registry).unwrap_or_default();
            if existing != bytes {
                std::fs::write(&vendored_registry, &bytes).unwrap_or_else(|e| {
                    panic!(
                        "write vendored registry at {}: {e}",
                        vendored_registry.display()
                    )
                });
            }
            println!("cargo:rerun-if-changed={}", src.display());
        }
    }
    println!("cargo:rerun-if-changed={}", vendored_registry.display());

    // ── 2. Typed-emission op-code hash ───────────────────────────────────────
    //
    // Primary source: foundation/spec/plugins/contracts/typed-emission-ops.toml
    // Fallback: src/plugins/typed-emission-ops.toml (vendored copy for CI)
    // Sentinel: 64 zeros when neither is present.
    let foundation_ops = manifest_dir
        .parent()
        .map(|p| p.join("foundation/spec/plugins/contracts/typed-emission-ops.toml"));
    let vendored_ops = manifest_dir.join("src/plugins/typed-emission-ops.toml");

    let ops_bytes: Option<Vec<u8>> = foundation_ops
        .as_ref()
        .and_then(|p| {
            if p.is_file() {
                std::fs::read(p).ok()
            } else {
                None
            }
        })
        .or_else(|| {
            if vendored_ops.is_file() {
                std::fs::read(&vendored_ops).ok()
            } else {
                None
            }
        });

    let ops_hash = match ops_bytes.as_ref() {
        Some(bytes) => sha256_hex(bytes),
        None => "0".repeat(64),
    };

    println!("cargo:rustc-env=EMISSION_OPS_HASH={}", ops_hash);

    // Trigger rebuild when spec TOML changes.
    if let Some(fp) = foundation_ops {
        println!("cargo:rerun-if-changed={}", fp.display());
    }
    println!("cargo:rerun-if-changed={}", vendored_ops.display());
    println!("cargo:rerun-if-changed=build.rs");
}
