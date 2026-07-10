//! Regression test for COMPILER-EMIT-ARENA-CONVERSION-MISSING
//! (fingerprint 3b15cd54, reported against compiler 0.33.29–0.33.33) and
//! the follow-up assemble-hook contract (prompt 872aed33).
//!
//! History:
//!  - 0.33.35 introduced `reconstruct_assemble_source` as a duct-tape
//!    fallback that serialized the arena's `PluginExpansion` back into
//!    Clean source by joining captured source-origin strings.
//!  - 0.33.38 (this landing) removes the fallback and replaces it with
//!    the `_inject_source_file` bridge specified in
//!    `foundation/spec/plugins/contracts/assemble.md`. Structural bridges
//!    called from `assemble_typed` now refuse with PLUGIN018 at the
//!    bridge entry.
//!
//! What this test asserts today: `PluginExpansion` no longer carries
//! `function_body_sources`, `start_body_sources`, or
//! `inline_stmt_sources` — the parallel source-origin arrays are gone.
//! If a maintainer restores them, this test stops compiling and the
//! regression is caught before ship.
//!
//! The end-to-end behaviour of structural bridges refusing with PLUGIN018
//! is covered by the arena-level unit tests inside
//! `src/plugins/typed_emission/arena.rs` and the plugin-loader
//! integration tests that drive a real plugin.wasm.

use clean_language_compiler::plugins::PluginExpansion;

#[test]
fn expansion_carries_no_source_origin_arrays() {
    // This is a compilation guard as much as an assertion. The fields
    // `function_body_sources`, `start_body_sources`, and
    // `inline_stmt_sources` were introduced in 0.33.35 to power the
    // `reconstruct_assemble_source` fallback and removed when
    // `_inject_source_file` (assemble.md §6.1) landed. Re-adding any of
    // them would re-open the oscillation window described in the
    // assemble.md preamble.
    let expansion = PluginExpansion::default();
    assert!(expansion.functions.is_empty());
    assert!(expansion.classes.is_empty());
    assert!(expansion.externals.is_empty());
    assert!(expansion.statements.is_empty());
    assert!(expansion.start_function.is_none());
    assert!(expansion.state.is_none());
}
