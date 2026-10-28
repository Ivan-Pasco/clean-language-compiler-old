//! Native WASM carryover-slot arena
//!
//! Fixed-position, two-slot storage for an outer-scope string variable that
//! is reassigned once per iteration inside an accumulator-rewritten loop.
//!
//! ## Why a third arena
//!
//! The main bump heap holds unbounded live data. The transient pool
//! (`transient_arena.rs`) holds strictly per-iteration intermediates that
//! are freed at every `__transient_scope_exit`. Neither fits the shape
//! of an outer-scope reassign target like
//!
//! ```text
//!     string item = json.get(arr, "0")
//!     while item != ""
//!         acc = acc + item
//!         i = i + 1
//!         item = json.get(arr, i.toString())    // outer-scope reassign
//! ```
//!
//! * Main heap: every iter leaks the previous `item`'s bytes because
//!   nothing frees them. Over N iters this trips the 32 MB linear-memory
//!   cap (`CODEGEN-LOOP-OUTER-STRING-REASSIGN-LEAK`, fingerprint
//!   `88dc6aeb0f8e`).
//!
//! * Transient pool: freed at end of every iter. The value written at
//!   iter N tail is dead before iter N+1's condition check reads it —
//!   the loop appears to terminate after one iter
//!   (`CODEGEN-STRING-MOVE-AND-RECLAIM-CORRUPTS-STRING`, fingerprint
//!   `7289dcf25032`).
//!
//! The carryover arena is a **two-slot ping-pong**: iter N writes into
//! slot `(N mod 2)`, iter N+1 reads that slot and writes into the other
//! slot (which by then holds the now-dead iter N-1 value). Each slot is
//! a fixed-size pool; a write simply copies the string into the pool
//! base — no bump pointer, because the pool holds exactly one value at
//! a time.
//!
//! ## Safety invariant
//!
//! Slot `s`'s content is valid from the moment it is written until the
//! NEXT write to slot `s`. Between those writes, iter N+1's body reads
//! from slot `s`; iter N+1's tail writes slot `1 - s`; iter N+2's body
//! reads slot `1 - s`; iter N+2's tail writes slot `s`, overwriting the
//! now-dead iter N value. Every read observes a live value.
//!
//! For this to be sound, the HIR rewriter must verify:
//!   1. The target variable is declared OUTSIDE the loop, so its
//!      lifetime is bounded by the enclosing scope (not the iteration).
//!   2. It is reassigned exactly once per iteration path via a call
//!      that returns a Clean string.
//!   3. It is NOT captured into an outer-scope collection
//!      (`list.push`, `pairs.set`, etc.) — such a capture would hold a
//!      dangling pointer after the next carryover overwrite.
//!   4. It is NOT returned from the enclosing function while still
//!      referring to the ping-ponged slot.
//!
//! Condition 3 is the same escape check
//! (`collect_escaping_body_local_names`) already used by the transient
//! arena rewrite.
//!
//! ## Layout
//!
//! Each rewritten variable gets its own pool pair from
//! `__carryover_pool_new`, allocated just before its loop and passed to
//! every `__carryover_copy` call for that variable:
//!
//! ```text
//!     pool        ──►  [ slot 0 ............ POOL_SIZE ................ ]
//!     pool + SIZE ──►  [ slot 1 ............ POOL_SIZE ................ ]
//! ```
//!
//! Pools used to be two module-wide globals (`CARRYOVER_A/B_BASE`), which
//! let a loop inside another loop's iteration overwrite the outer loop's
//! variable. Those globals are now unused but stay reserved so global
//! numbering does not shift.
//!
//! Each write memcpy's `4 + string_length` bytes from `src_str_ptr` to
//! the pool base. No allocation bookkeeping — the pool is
//! single-tenanted.
//!
//! ## Overflow (spill) semantics
//!
//! If the incoming string is larger than `POOL_SIZE`, the helper falls
//! back to `__malloc(4 + len)` and copies there. The returned pointer
//! is on the main heap; that iter's copy leaks (one main-heap block per
//! oversize write). Correctness is preserved — the string is valid for
//! reads — only the space-bound optimization is lost. `POOL_SIZE` is
//! chosen large enough (32 KB) to cover realistic per-item JSON payload
//! sizes without spill.
//!
//! ## See also
//!
//! `src/hir/hir_builder.rs` (module doc) — pipeline map. The carryover
//! arena is consumed by pass 5 (`apply_outer_string_carryover_rewrite`),
//! which pass 2 (`rewrite_string_accumulator_loops`) invokes on match.

use wasm_encoder::{BlockType, Instruction, MemArg};

use super::{ALIGNMENT, STRING_LENGTH_OFFSET};

/// Size of each carryover pool in bytes.
///
/// Sized to hold a single string up to ~32 KB. Larger writes spill to
/// `__malloc` (one leak per oversize iter, correctness preserved).
pub const CARRYOVER_POOL_SIZE: i32 = 32_768;

/// Generate instructions for `__carryover_pool_new`.
///
/// Allocates one pool pair for a single rewritten loop variable:
/// `2 * CARRYOVER_POOL_SIZE` bytes, slot 0 then slot 1. Returns the base
/// pointer, or 0 when memory cannot grow (copies then spill to the main
/// heap).
///
/// Each rewritten variable gets its own pair, allocated just before its
/// loop. A loop running inside another loop's iteration (for example in a
/// method called from the outer loop body) therefore never overwrites the
/// outer loop's slots; the inner pair is reclaimed with the outer
/// iteration's scope.
pub fn gen_carryover_pool_new(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        Instruction::I32Const(2 * CARRYOVER_POOL_SIZE),
        Instruction::Call(malloc_func),
    ]
}

/// Generate instructions for `__carryover_copy`.
///
/// Parameters:
///   - local 0: src_str_ptr (i32) — pointer to a Clean string
///     `[len:u32 LE][utf8 data:len bytes]`
///   - local 1: slot (i32) — 0 or 1
///   - local 2: pool (i32) — pool pair from `__carryover_pool_new`
///
/// Returns: i32 — pointer to a Clean string in the selected slot (or on
/// the main heap on spill). Same layout as the source.
///
/// Behavior:
///   1. `len = i32.load(src_str_ptr + STRING_LENGTH_OFFSET)`.
///   2. `total = align_up_8(4 + len)`.
///   3. `slot_base = pool + slot * CARRYOVER_POOL_SIZE` (0 if `pool == 0`).
///   4. If `slot_base == 0` or `total > CARRYOVER_POOL_SIZE`: copy into a
///      fresh `__malloc(total)` instead.
///   5. Clear any json.get cache entry keyed by the destination.
///   6. `memcpy(dst, src_str_ptr, 4 + len)`; return `dst`.
///
/// Locals:
///   - 3: len
///   - 4: total (4 + len, aligned)
///   - 5: slot_base
///   - 6: dst_ptr
///   - 7: copy_i (memcpy loop counter)
#[allow(clippy::vec_init_then_push)]
pub fn gen_carryover_copy(malloc_func: u32) -> Vec<Instruction<'static>> {
    let mut ins: Vec<Instruction<'static>> = Vec::new();

    // len = i32.load(src_str_ptr + STRING_LENGTH_OFFSET)
    ins.push(Instruction::LocalGet(0));
    ins.push(Instruction::I32Load(MemArg {
        offset: STRING_LENGTH_OFFSET as u64,
        align: 2,
        memory_index: 0,
    }));
    ins.push(Instruction::LocalSet(3));

    // total = align_up_8(4 + len)
    ins.push(Instruction::LocalGet(3));
    ins.push(Instruction::I32Const(4 + (ALIGNMENT as i32 - 1)));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::I32Const(!(ALIGNMENT - 1) as i32));
    ins.push(Instruction::I32And);
    ins.push(Instruction::LocalSet(4));

    // slot_base = pool == 0 ? 0 : pool + slot * CARRYOVER_POOL_SIZE
    ins.push(Instruction::LocalGet(2));
    ins.push(Instruction::I32Eqz);
    ins.push(Instruction::If(BlockType::Result(
        wasm_encoder::ValType::I32,
    )));
    ins.push(Instruction::I32Const(0));
    ins.push(Instruction::Else);
    ins.push(Instruction::LocalGet(2));
    ins.push(Instruction::LocalGet(1));
    ins.push(Instruction::I32Const(CARRYOVER_POOL_SIZE));
    ins.push(Instruction::I32Mul);
    ins.push(Instruction::I32Add);
    ins.push(Instruction::End);
    ins.push(Instruction::LocalSet(5));

    // dst_ptr = (slot_base == 0 || total > POOL_SIZE) ? __malloc(total) : slot_base
    ins.push(Instruction::LocalGet(5));
    ins.push(Instruction::I32Eqz);
    ins.push(Instruction::LocalGet(4));
    ins.push(Instruction::I32Const(CARRYOVER_POOL_SIZE));
    ins.push(Instruction::I32GtU);
    ins.push(Instruction::I32Or);
    ins.push(Instruction::If(BlockType::Result(
        wasm_encoder::ValType::I32,
    )));
    // Spill: allocate a fresh main-heap block. Leaks one string per
    // oversize iteration, but preserves correctness.
    ins.push(Instruction::LocalGet(4));
    ins.push(Instruction::Call(malloc_func));
    ins.push(Instruction::Else);
    ins.push(Instruction::LocalGet(5));
    ins.push(Instruction::End);
    ins.push(Instruction::LocalSet(6));

    // The slot is about to be overwritten in place: drop any json.get cache
    // entry keyed by its address, or the next json.get on the new contents
    // would return the previous contents' parse tree.
    ins.extend(super::memory::invalidate_json_cache_at(6));

    // Byte copy: dst[i] = src[i] for i in 0..(4 + len)
    ins.push(Instruction::I32Const(0));
    ins.push(Instruction::LocalSet(7));
    ins.push(Instruction::Block(BlockType::Empty));
    ins.push(Instruction::Loop(BlockType::Empty));
    ins.push(Instruction::LocalGet(7));
    ins.push(Instruction::LocalGet(3));
    ins.push(Instruction::I32Const(4));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::I32GeU);
    ins.push(Instruction::BrIf(1));
    ins.push(Instruction::LocalGet(6));
    ins.push(Instruction::LocalGet(7));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::LocalGet(0));
    ins.push(Instruction::LocalGet(7));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::I32Load8U(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    ins.push(Instruction::I32Store8(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));
    ins.push(Instruction::LocalGet(7));
    ins.push(Instruction::I32Const(1));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::LocalSet(7));
    ins.push(Instruction::Br(0));
    ins.push(Instruction::End);
    ins.push(Instruction::End);

    ins.push(Instruction::LocalGet(6));
    ins
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::native_stdlib::{CARRYOVER_A_BASE_GLOBAL, CARRYOVER_B_BASE_GLOBAL};

    #[test]
    fn test_carryover_copy_reads_string_length() {
        let ins = gen_carryover_copy(0);
        // Must load i32 at STRING_LENGTH_OFFSET of the source pointer.
        assert!(
            ins.iter().any(|i| matches!(i, Instruction::I32Load(MemArg {
                offset,
                ..
            }) if *offset == STRING_LENGTH_OFFSET as u64)),
            "carryover_copy must read the length prefix at offset 0 of the source string"
        );
    }

    /// Overwriting a slot in place must drop any json.get cache entry keyed
    /// by the slot's address (the slots alternate across loop iterations).
    #[test]
    fn test_carryover_copy_invalidates_json_cache() {
        use crate::codegen::native_stdlib::{JSON_CACHE_BASE_GLOBAL, JSON_CACHE_ENTRIES};
        let ins = gen_carryover_copy(0);
        for entry in 0..JSON_CACHE_ENTRIES {
            let src = JSON_CACHE_BASE_GLOBAL + 3 * entry;
            assert!(
                ins.iter()
                    .any(|i| matches!(i, Instruction::GlobalSet(g) if *g == src)),
                "carryover_copy must clear json cache entry {entry} keyed by the slot"
            );
        }
    }

    // The former tests here asserted that every copy went through the
    // global CARRYOVER_A/B pool slots and lazily initialised them. Those
    // globals were shared by every loop in the program, so a loop running
    // inside another loop's iteration overwrote the outer loop's variable
    // (website /syntax rendered one card per category). Pools are now
    // per-variable; these tests pin that contract instead.

    /// Copies must not touch the shared carryover globals any more.
    #[test]
    fn test_carryover_copy_uses_no_shared_pool_globals() {
        let ins = gen_carryover_copy(0);
        let touches_globals = ins.iter().any(|i| {
            matches!(i, Instruction::GlobalGet(g) | Instruction::GlobalSet(g)
                if *g == CARRYOVER_A_BASE_GLOBAL || *g == CARRYOVER_B_BASE_GLOBAL)
        });
        assert!(
            !touches_globals,
            "carryover_copy must use its pool argument, not the shared CARRYOVER_A/B globals"
        );
    }

    /// The slot address is derived from the pool argument (local 2).
    #[test]
    fn test_carryover_copy_reads_pool_argument() {
        let ins = gen_carryover_copy(0);
        assert!(
            ins.iter().any(|i| matches!(i, Instruction::LocalGet(2))),
            "carryover_copy must read its pool argument (local 2)"
        );
        assert!(
            ins.iter()
                .any(|i| matches!(i, Instruction::I32Const(c) if *c == CARRYOVER_POOL_SIZE)),
            "slot 1 must start CARRYOVER_POOL_SIZE bytes after slot 0"
        );
    }

    /// Oversize strings and a failed pool allocation spill to __malloc.
    #[test]
    fn test_carryover_copy_spills_to_malloc() {
        let ins = gen_carryover_copy(7);
        assert!(
            ins.iter().any(|i| matches!(i, Instruction::Call(7))),
            "carryover_copy must fall back to __malloc for oversize strings"
        );
    }

    /// Each pool pair holds two slots.
    #[test]
    fn test_carryover_pool_new_allocates_two_slots() {
        let ins = gen_carryover_pool_new(9);
        assert!(ins
            .iter()
            .any(|i| matches!(i, Instruction::I32Const(c) if *c == 2 * CARRYOVER_POOL_SIZE)));
        assert!(ins.iter().any(|i| matches!(i, Instruction::Call(9))));
    }
}
