//! Native WASM transient arena
//!
//! A second bump region used by the HIR-level accumulator-loop rewrite to
//! hold per-iteration intermediates (typically conditional helpers like
//! `headHtml = "<h2>" + head + "</h2>"`) that would otherwise strand on the
//! main heap and drive CMP-SSR-MALLOC-OOM-PAGE-RENDER.
//!
//! ## Why a second arena
//!
//! The main bump heap (`HEAP_PTR_GLOBAL`, see `memory.rs::gen_malloc`) is
//! sorted by allocation time, not by lifetime. The previous attempt at
//! per-iter reclamation (`__string_builder_reclaim`, rolled back in
//! `CMP-SSR-RECLAIM-FREES-LIVE-POINTER`) tried to "punch a hole" mid-region
//! by resetting `HEAP_PTR` to `max(init_mark, builder_end)`. That corrupts
//! any live pointer that happens to sit above the new mark — exactly what
//! happened with `head = json.get(...)` reassignments in the canonical
//! /tutorials shape.
//!
//! The right pattern (Cyclone nested regions; bump-scope `scoped()`;
//! frame-allocator practice) is:
//!
//!   * outer region — anything that may live across iteration boundaries
//!   * inner region — strictly per-iteration transients
//!
//! Resetting the inner region cannot dangle outer pointers because they
//! were never in that region.
//!
//! ## Layout
//!
//! The arena lives at a fixed-size pool that is `__malloc`-allocated
//! lazily on first `__transient_scope_enter`. The pool's base address is
//! stored in `TRANSIENT_BASE_GLOBAL` (0 = uninitialized). The current
//! bump pointer within the pool is stored in `TRANSIENT_PTR_GLOBAL`.
//!
//!   TRANSIENT_BASE  ──►  [ ... transient bytes ... ]   ◄── TRANSIENT_PTR
//!                                                          (bumps upward)
//!                        [               POOL_SIZE              ]
//!
//! When a request asks for more than the pool can fit, we fall back to
//! `__malloc` on the main heap. That fallback path is rare in practice
//! (the helper shape is bounded by a handful of small strings per iter),
//! but it guarantees correctness: an allocation never fails because the
//! transient arena is the wrong region — it just leaks for that iter.
//!
//! ## Scoping
//!
//! `__transient_scope_enter()` returns the current `TRANSIENT_PTR`.
//! `__transient_scope_exit(mark)` writes that value back. Nested enters
//! compose because each one saves and restores its own mark.
//!
//! ## Pool size
//!
//! `TRANSIENT_POOL_SIZE` is 64 KB. Each iteration of a typical SSR loop
//! allocates a few hundred bytes of helper strings; 64 KB covers ~100
//! iterations of "comfortable" headroom before any single iteration
//! spills to the main heap. Spill is correct (the helpers still get
//! freed at end-of-iter via the bump pattern of the pool), it just
//! means the leak from that iter survives on the main heap until the
//! next host-driven scope_pop.
//!
//! ## See also
//!
//! `src/hir/hir_builder.rs` (module doc) — pipeline map for the five
//! string-accumulator rewrite passes and the reclaim rollback that
//! motivated this arena.

use wasm_encoder::{BlockType, Instruction};

use super::{ALIGNMENT, TRANSIENT_BASE_GLOBAL, TRANSIENT_PTR_GLOBAL};

/// Size of the lazily-allocated transient pool, in bytes.
///
/// Sized to cover the per-iter helper alloc footprint of the canonical
/// SSR shape (~500 bytes) for ~100 iterations before any single iter
/// spills to the main heap. Spill is correctness-safe; the pool is
/// purely an optimization to keep transient bytes out of the main
/// bump region.
pub const TRANSIENT_POOL_SIZE: i32 = 65_536;

/// Generate instructions for `__transient_scope_enter`.
///
/// Parameters: none
/// Returns: i32 — the current `TRANSIENT_PTR` value (the save mark)
///
/// On first call (when `TRANSIENT_BASE == 0`), lazily initializes the
/// pool via `__malloc(TRANSIENT_POOL_SIZE)`. If `__malloc` fails (e.g.
/// host-capped memory growth), `TRANSIENT_BASE` stays 0 and the
/// returned mark is also 0 — `__transient_alloc` will then fall back to
/// `__malloc` for every allocation, which is the safe degradation path.
///
/// Locals:
///   - local 0: pool_ptr (used during lazy init only)
pub fn gen_transient_scope_enter(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // If TRANSIENT_BASE is 0, lazily allocate the pool.
        Instruction::GlobalGet(TRANSIENT_BASE_GLOBAL),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        // pool_ptr = __malloc(TRANSIENT_POOL_SIZE)
        Instruction::I32Const(TRANSIENT_POOL_SIZE),
        Instruction::Call(malloc_func),
        Instruction::LocalTee(0),
        // If malloc failed (returned 0), leave TRANSIENT_BASE = 0 and
        // TRANSIENT_PTR = 0. The save-mark we return will be 0, and
        // every subsequent `__transient_alloc` will spill to the main
        // heap. Correctness is preserved.
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        // TRANSIENT_BASE = pool_ptr
        Instruction::LocalGet(0),
        Instruction::GlobalSet(TRANSIENT_BASE_GLOBAL),
        // TRANSIENT_PTR = pool_ptr
        Instruction::LocalGet(0),
        Instruction::GlobalSet(TRANSIENT_PTR_GLOBAL),
        Instruction::End,
        // Return TRANSIENT_PTR (the current save-mark).
        Instruction::GlobalGet(TRANSIENT_PTR_GLOBAL),
    ]
}

/// Generate instructions for `__transient_scope_exit`.
///
/// Parameters:
///   - local 0: mark (i32) — value previously returned by
///     `__transient_scope_enter`.
///
/// Returns: void
///
/// Resets `TRANSIENT_PTR` to the supplied mark. All transient
/// allocations made after the matching `enter` become reclaimable.
///
/// Defensive guard: if `mark` is 0, the matching `enter` failed to
/// lazily allocate (or was never called), so the pool was never
/// initialized for this scope. Silently no-op — overwriting
/// `TRANSIENT_PTR` with 0 would corrupt the pool for any outer scope
/// that did successfully initialize.
pub fn gen_transient_scope_exit() -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::Return,
        Instruction::End,
        Instruction::LocalGet(0),
        Instruction::GlobalSet(TRANSIENT_PTR_GLOBAL),
    ]
}

/// Generate instructions for `__transient_alloc`.
///
/// Parameters:
///   - local 0: size (i32) — requested allocation size in bytes
///
/// Returns: i32 — pointer to size bytes of memory. Allocation site:
///   * the transient pool, if `TRANSIENT_BASE != 0` and the request
///     fits within the remaining pool bytes.
///   * the main bump heap, otherwise (fallback via `__malloc`).
///
/// Locals:
///   - local 0: size (parameter)
///   - local 1: base (TRANSIENT_BASE snapshot)
///   - local 2: ptr  (TRANSIENT_PTR snapshot — the candidate return)
///   - local 3: aligned_size
///   - local 4: new_ptr
pub fn gen_transient_alloc(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // base = TRANSIENT_BASE
        Instruction::GlobalGet(TRANSIENT_BASE_GLOBAL),
        Instruction::LocalSet(1),
        // If base == 0, fall back to __malloc(size).
        Instruction::LocalGet(1),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(0),
        Instruction::Call(malloc_func),
        Instruction::Return,
        Instruction::End,
        // ptr = TRANSIENT_PTR
        Instruction::GlobalGet(TRANSIENT_PTR_GLOBAL),
        Instruction::LocalSet(2),
        // aligned_size = (size + 7) & ~7
        Instruction::LocalGet(0),
        Instruction::I32Const((ALIGNMENT - 1) as i32),
        Instruction::I32Add,
        Instruction::I32Const(!(ALIGNMENT - 1) as i32),
        Instruction::I32And,
        Instruction::LocalSet(3),
        // new_ptr = ptr + aligned_size
        Instruction::LocalGet(2),
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::LocalSet(4),
        // Pool overflow check: if new_ptr > base + POOL_SIZE, spill to __malloc.
        Instruction::LocalGet(4),
        Instruction::LocalGet(1),
        Instruction::I32Const(TRANSIENT_POOL_SIZE),
        Instruction::I32Add,
        Instruction::I32GtU,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(0),
        Instruction::Call(malloc_func),
        Instruction::Return,
        Instruction::End,
        // Fits in pool: commit new_ptr and return the old ptr.
        Instruction::LocalGet(4),
        Instruction::GlobalSet(TRANSIENT_PTR_GLOBAL),
        Instruction::LocalGet(2),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_enter_reads_then_returns_transient_ptr() {
        let instructions = gen_transient_scope_enter(0);
        assert!(!instructions.is_empty());
        // Must touch TRANSIENT_BASE_GLOBAL for the lazy-init guard.
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::GlobalGet(g) if *g == TRANSIENT_BASE_GLOBAL)),
            "scope_enter must check TRANSIENT_BASE_GLOBAL for lazy init"
        );
        // Must call __malloc inside the lazy-init branch.
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::Call(f) if *f == 0)),
            "scope_enter must call __malloc when the pool is uninitialized"
        );
        // Must end by reading TRANSIENT_PTR_GLOBAL (the returned mark).
        assert!(
            matches!(
                instructions.last(),
                Some(Instruction::GlobalGet(g)) if *g == TRANSIENT_PTR_GLOBAL
            ),
            "scope_enter must return TRANSIENT_PTR_GLOBAL as its mark — \
             any other return value breaks the matching scope_exit"
        );
    }

    #[test]
    fn test_scope_exit_writes_mark_to_transient_ptr() {
        let instructions = gen_transient_scope_exit();
        // Must write the input local to TRANSIENT_PTR_GLOBAL.
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::GlobalSet(g) if *g == TRANSIENT_PTR_GLOBAL)),
            "scope_exit must write TRANSIENT_PTR_GLOBAL — without this the \
             pool never gets reset and the entire purpose of the arena is lost"
        );
        // Must guard against a 0 mark (uninitialized scope) to avoid
        // clobbering an outer scope's TRANSIENT_PTR.
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::I32Eqz)),
            "scope_exit must guard mark==0 — silently writing 0 to \
             TRANSIENT_PTR would corrupt a successfully-initialized \
             outer scope"
        );
    }

    #[test]
    fn test_transient_alloc_falls_back_when_base_is_zero() {
        let instructions = gen_transient_alloc(0);
        // The first thing after reading TRANSIENT_BASE is the
        // base==0 → malloc fallback branch.
        let mut saw_base_read = false;
        let mut saw_call_in_fallback = false;
        for i in &instructions {
            if matches!(i, Instruction::GlobalGet(g) if *g == TRANSIENT_BASE_GLOBAL) {
                saw_base_read = true;
            }
            if saw_base_read && matches!(i, Instruction::Call(f) if *f == 0) {
                saw_call_in_fallback = true;
                break;
            }
        }
        assert!(
            saw_base_read,
            "transient_alloc must read TRANSIENT_BASE_GLOBAL to detect \
             the uninitialized-pool case"
        );
        assert!(
            saw_call_in_fallback,
            "transient_alloc must fall back to __malloc when the pool \
             was never initialized (TRANSIENT_BASE == 0) — without the \
             fallback the function would return a stale TRANSIENT_PTR \
             that points to a non-existent pool"
        );
    }

    #[test]
    fn test_transient_alloc_writes_back_transient_ptr_on_hit() {
        let instructions = gen_transient_alloc(0);
        // The success path commits a new TRANSIENT_PTR via GlobalSet.
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::GlobalSet(g) if *g == TRANSIENT_PTR_GLOBAL)),
            "transient_alloc must bump TRANSIENT_PTR_GLOBAL on the pool-hit \
             path — otherwise the next alloc would return the same pointer"
        );
    }

    #[test]
    fn test_transient_alloc_aligns_size_to_8_bytes() {
        let instructions = gen_transient_alloc(0);
        // The alignment dance is (size + 7) & ~7 — that's an I32Const(7),
        // an I32Add, an I32Const(0xFFFFFFF8 == -8 as i32), and an I32And.
        let mut saw_7 = false;
        let mut saw_neg8 = false;
        for i in &instructions {
            if matches!(i, Instruction::I32Const(7)) {
                saw_7 = true;
            }
            if matches!(i, Instruction::I32Const(-8)) {
                saw_neg8 = true;
            }
        }
        assert!(
            saw_7 && saw_neg8,
            "transient_alloc must align allocations to {} bytes — without \
             alignment, subsequent allocations may load/store across \
             unaligned boundaries (slow on most hosts, trap on some)",
            ALIGNMENT
        );
    }
}
