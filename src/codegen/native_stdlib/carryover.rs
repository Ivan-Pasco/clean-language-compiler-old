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
//! Two independent pools, each lazily `__malloc`-allocated on first
//! write:
//!
//! ```text
//!     CARRYOVER_A_BASE  ──►  [ ................ POOL_SIZE ................ ]
//!     CARRYOVER_B_BASE  ──►  [ ................ POOL_SIZE ................ ]
//! ```
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

use wasm_encoder::{BlockType, Instruction, MemArg};

use super::{ALIGNMENT, CARRYOVER_A_BASE_GLOBAL, CARRYOVER_B_BASE_GLOBAL, STRING_LENGTH_OFFSET};

/// Size of each carryover pool in bytes.
///
/// Sized to hold a single string up to ~32 KB. Larger writes spill to
/// `__malloc` (one leak per oversize iter, correctness preserved).
pub const CARRYOVER_POOL_SIZE: i32 = 32_768;

/// Generate instructions for `__carryover_copy`.
///
/// Parameters:
///   - local 0: src_str_ptr (i32) — pointer to a Clean string
///     `[len:u32 LE][utf8 data:len bytes]`
///   - local 1: slot (i32) — 0 or 1
///
/// Returns: i32 — pointer to a Clean string in the selected pool base
/// (or on the main heap on spill). Same layout as the source.
///
/// Behavior:
///   1. Read `len = i32.load(src_str_ptr + STRING_LENGTH_OFFSET)`.
///   2. `total = align_up_8(4 + len)`.
///   3. Select `pool_base = (slot == 0) ? CARRYOVER_A_BASE : CARRYOVER_B_BASE`.
///   4. If `pool_base == 0`: lazily `__malloc(CARRYOVER_POOL_SIZE)`,
///      store into the slot's global. If malloc fails, fall back to
///      per-call `__malloc(total)`.
///   5. If `total > CARRYOVER_POOL_SIZE`: fall back to per-call
///      `__malloc(total)`.
///   6. `memcpy(pool_base, src_str_ptr, 4 + len)`.
///   7. Return `pool_base`.
///
/// Notes:
///   * No bump pointer — the pool is single-tenanted per slot.
///   * The 8-byte alignment on `total` is defensive: subsequent
///     reads through the pool base still land on well-aligned
///     addresses. String reads only need 4-byte alignment for the
///     length prefix, but rounding up costs nothing and future-proofs
///     the pool against extensions.
///
/// Locals (see registration in `codegen_registration.rs`):
///   - 0: src_str_ptr (param)
///   - 1: slot (param)
///   - 2: len (str length in bytes)
///   - 3: total (4 + len, aligned up)
///   - 4: pool_base (chosen slot's global value)
///   - 5: dst_ptr (return value; either pool_base or a fresh __malloc)
///   - 6: copy_i (memcpy loop counter)
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
    ins.push(Instruction::LocalSet(2));

    // total = align_up_8(4 + len) = ((len + 4) + 7) & ~7
    ins.push(Instruction::LocalGet(2));
    ins.push(Instruction::I32Const(4 + (ALIGNMENT as i32 - 1)));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::I32Const(!(ALIGNMENT - 1) as i32));
    ins.push(Instruction::I32And);
    ins.push(Instruction::LocalSet(3));

    // pool_base = (slot == 0) ? CARRYOVER_A_BASE : CARRYOVER_B_BASE
    ins.push(Instruction::LocalGet(1));
    ins.push(Instruction::I32Eqz);
    ins.push(Instruction::If(BlockType::Result(
        wasm_encoder::ValType::I32,
    )));
    ins.push(Instruction::GlobalGet(CARRYOVER_A_BASE_GLOBAL));
    ins.push(Instruction::Else);
    ins.push(Instruction::GlobalGet(CARRYOVER_B_BASE_GLOBAL));
    ins.push(Instruction::End);
    ins.push(Instruction::LocalSet(4));

    // If pool_base == 0, lazily allocate the pool.
    ins.push(Instruction::LocalGet(4));
    ins.push(Instruction::I32Eqz);
    ins.push(Instruction::If(BlockType::Empty));
    // pool_base = __malloc(CARRYOVER_POOL_SIZE)
    ins.push(Instruction::I32Const(CARRYOVER_POOL_SIZE));
    ins.push(Instruction::Call(malloc_func));
    ins.push(Instruction::LocalSet(4));
    // If lazy malloc failed (returned 0), fall through — the spill path
    // below detects pool_base == 0 by testing (total > POOL_SIZE) OR
    // (pool_base == 0). Since we compare pool_base again, a failed lazy
    // init still routes to per-call malloc for correctness.
    //
    // On success, store the pool base into the correct global for reuse.
    ins.push(Instruction::LocalGet(4));
    ins.push(Instruction::I32Eqz);
    ins.push(Instruction::I32Eqz); // negate: skip the store when pool_base is still 0
    ins.push(Instruction::If(BlockType::Empty));
    ins.push(Instruction::LocalGet(1));
    ins.push(Instruction::I32Eqz);
    ins.push(Instruction::If(BlockType::Empty));
    ins.push(Instruction::LocalGet(4));
    ins.push(Instruction::GlobalSet(CARRYOVER_A_BASE_GLOBAL));
    ins.push(Instruction::Else);
    ins.push(Instruction::LocalGet(4));
    ins.push(Instruction::GlobalSet(CARRYOVER_B_BASE_GLOBAL));
    ins.push(Instruction::End);
    ins.push(Instruction::End);
    ins.push(Instruction::End);

    // Spill decision: dst_ptr = (pool_base == 0 || total > POOL_SIZE)
    //                          ? __malloc(total)
    //                          : pool_base
    ins.push(Instruction::LocalGet(4));
    ins.push(Instruction::I32Eqz);
    ins.push(Instruction::LocalGet(3));
    ins.push(Instruction::I32Const(CARRYOVER_POOL_SIZE));
    ins.push(Instruction::I32GtU);
    ins.push(Instruction::I32Or);
    ins.push(Instruction::If(BlockType::Result(
        wasm_encoder::ValType::I32,
    )));
    // Spill: allocate a fresh main-heap block. Leaks one string per
    // oversize iter, but preserves correctness.
    ins.push(Instruction::LocalGet(3));
    ins.push(Instruction::Call(malloc_func));
    ins.push(Instruction::Else);
    ins.push(Instruction::LocalGet(4));
    ins.push(Instruction::End);
    ins.push(Instruction::LocalSet(5));

    // memcpy loop: for (i = 0; i < 4 + len; i += 4) dst[i..i+4] = src[i..i+4]
    // Simpler: byte loop for correctness on non-multiple-of-4 lengths.
    //
    // Byte loop:
    //     copy_i = 0
    //     while copy_i < (4 + len):
    //         dst[copy_i] = src[copy_i]
    //         copy_i += 1
    ins.push(Instruction::I32Const(0));
    ins.push(Instruction::LocalSet(6));
    ins.push(Instruction::Block(BlockType::Empty));
    ins.push(Instruction::Loop(BlockType::Empty));
    // if copy_i >= (4 + len) break
    ins.push(Instruction::LocalGet(6));
    ins.push(Instruction::LocalGet(2));
    ins.push(Instruction::I32Const(4));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::I32GeU);
    ins.push(Instruction::BrIf(1));
    // dst[copy_i] = src[copy_i]
    ins.push(Instruction::LocalGet(5));
    ins.push(Instruction::LocalGet(6));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::LocalGet(0));
    ins.push(Instruction::LocalGet(6));
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
    // copy_i += 1
    ins.push(Instruction::LocalGet(6));
    ins.push(Instruction::I32Const(1));
    ins.push(Instruction::I32Add);
    ins.push(Instruction::LocalSet(6));
    ins.push(Instruction::Br(0));
    ins.push(Instruction::End);
    ins.push(Instruction::End);

    // Return dst_ptr.
    ins.push(Instruction::LocalGet(5));

    ins
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_carryover_copy_touches_both_pool_globals() {
        let ins = gen_carryover_copy(0);
        let touches_a = ins
            .iter()
            .any(|i| matches!(i, Instruction::GlobalGet(g) if *g == CARRYOVER_A_BASE_GLOBAL));
        let touches_b = ins
            .iter()
            .any(|i| matches!(i, Instruction::GlobalGet(g) if *g == CARRYOVER_B_BASE_GLOBAL));
        assert!(touches_a, "must read CARRYOVER_A_BASE for slot 0");
        assert!(touches_b, "must read CARRYOVER_B_BASE for slot 1");
    }

    #[test]
    fn test_carryover_copy_lazily_calls_malloc() {
        let ins = gen_carryover_copy(0);
        // At least one Call(0) — either the lazy pool alloc or the spill fallback.
        let call_count = ins
            .iter()
            .filter(|i| matches!(i, Instruction::Call(f) if *f == 0))
            .count();
        assert!(
            call_count >= 2,
            "expected both lazy-pool-alloc AND spill-fallback to call __malloc; got {call_count}"
        );
    }

    #[test]
    fn test_carryover_copy_writes_pool_bases_on_lazy_init() {
        let ins = gen_carryover_copy(0);
        let sets_a = ins
            .iter()
            .any(|i| matches!(i, Instruction::GlobalSet(g) if *g == CARRYOVER_A_BASE_GLOBAL));
        let sets_b = ins
            .iter()
            .any(|i| matches!(i, Instruction::GlobalSet(g) if *g == CARRYOVER_B_BASE_GLOBAL));
        assert!(
            sets_a && sets_b,
            "lazy init must persist the fresh pool base into the correct global (both slots)"
        );
    }
}
