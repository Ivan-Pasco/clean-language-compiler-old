//! Native WASM string-builder helpers
//!
//! Implements an append-only growable buffer used by the HIR-level
//! accumulator-loop rewrite (see `src/hir/hir_builder.rs`, function
//! `rewrite_string_accumulator_loops`).
//!
//! ## Why this exists
//!
//! The compiler ships a bump allocator (`__malloc`) that cannot reclaim
//! individual blocks. The canonical SSR pattern
//!
//! ```text
//! string acc = ""
//! while i < N
//!     acc = acc + render(i)
//! ```
//!
//! is therefore O(N²) in heap consumption: every iteration allocates a
//! brand-new `acc` (left + right concatenated), and the OLD acc is stranded
//! forever in the heap. Tracked in CMP-SSR-MALLOC-OOM-PAGE-RENDER.
//!
//! Mirroring Go's `strings.Builder`, the compiler instead detects the
//! accumulator pattern at HIR time and lowers it to a doubling-capacity
//! buffer. The geometric series of stranded older buffers is O(N) total —
//! same trick `Vec<T>::push` uses in Rust.
//!
//! ## Layout
//!
//! A builder is a heap region with this header:
//!
//! ```text
//! offset 0..3   capacity : i32     // bytes allocated for content
//! offset 4..7   length   : i32     // bytes currently in use
//! offset 8..    bytes              // the accumulated content
//! ```
//!
//! `finalize` returns `builder_ptr + 4`. From the returned pointer's
//! perspective, byte 0 is the length and byte 4 is the first content byte
//! — exactly the layout every other Clean string uses (see
//! `STRING_LENGTH_OFFSET`, `STRING_DATA_OFFSET`). The 4 bytes preceding the
//! returned pointer (the now-stale capacity field) become garbage; no
//! Clean Language consumer ever reads bytes before a string's length
//! field.
//!
//! ## Reallocation contract
//!
//! `append` returns the (possibly-relocated) builder pointer because a
//! growth event allocates a fresh region via `__malloc` and memcpys
//! existing content into it. The rewrite assigns the result back to the
//! builder local:
//!
//! ```text
//! __sb_0 = __string_builder_new()
//! while cond
//!     __sb_0 = __string_builder_append(__sb_0, <expr>)
//! acc = __string_builder_finalize(__sb_0)
//! ```
//!
//! ## See also
//!
//! `src/hir/hir_builder.rs` (module doc) — pipeline map for the five
//! string-accumulator rewrite passes that emit calls into this module.
//! `__string_builder_reclaim` and `__heap_ptr_snapshot` stay registered
//! as harmless stubs after the 0.30.375 reclaim rollback; see the module
//! doc for the failure mode that removed the emission.

use wasm_encoder::{BlockType, Instruction, MemArg};

use super::{HEAP_PTR_GLOBAL, STRING_DATA_OFFSET, STRING_LENGTH_OFFSET};

/// Builder header layout offsets (relative to the builder pointer).
const BUILDER_CAPACITY_OFFSET: u64 = 0;
const BUILDER_LENGTH_OFFSET: u64 = 4;
const BUILDER_HEADER_SIZE: i32 = 8;
const INITIAL_CAPACITY: i32 = 16;

/// Generate instructions for `__string_builder_new`.
///
/// Parameters: none.
/// Returns: i32 — pointer to a freshly allocated builder with `capacity =
/// INITIAL_CAPACITY` and `length = 0`. Returns 0 if the underlying
/// `__malloc` fails (host capped memory growth).
///
/// Locals:
///   - local 0: builder_ptr
pub fn gen_string_builder_new(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // builder_ptr = malloc(HEADER_SIZE + INITIAL_CAPACITY)
        Instruction::I32Const(BUILDER_HEADER_SIZE + INITIAL_CAPACITY),
        Instruction::Call(malloc_func),
        Instruction::LocalTee(0),
        // If allocation failed, return 0 (already on stack via LocalTee).
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        // Write capacity = INITIAL_CAPACITY at builder_ptr + 0
        Instruction::LocalGet(0),
        Instruction::I32Const(INITIAL_CAPACITY),
        Instruction::I32Store(MemArg {
            offset: BUILDER_CAPACITY_OFFSET,
            align: 2,
            memory_index: 0,
        }),
        // Write length = 0 at builder_ptr + 4
        Instruction::LocalGet(0),
        Instruction::I32Const(0),
        Instruction::I32Store(MemArg {
            offset: BUILDER_LENGTH_OFFSET,
            align: 2,
            memory_index: 0,
        }),
        // Return builder_ptr
        Instruction::LocalGet(0),
    ]
}

/// Generate instructions for `__string_builder_append`.
///
/// Parameters:
///   - local 0: builder_ptr (i32)
///   - local 1: str_ptr     (i32) — a length-prefixed Clean string
///
/// Returns: i32 — the (possibly-relocated) builder pointer. Callers MUST
/// store the return value back into the builder local. Returns 0 if a
/// growth `__malloc` fails.
///
/// Locals:
///   - local 2: capacity         (current builder capacity)
///   - local 3: length           (current builder length)
///   - local 4: str_len          (length of incoming string)
///   - local 5: needed           (length + str_len)
///   - local 6: new_capacity     (doubled capacity when growing)
///   - local 7: new_builder_ptr  (post-grow builder pointer)
///   - local 8: i                (loop counter)
///
/// Growth rule: `new_capacity = max(capacity * 2, needed)`. This is the
/// doubling that turns the accumulator pattern from O(n²) to O(n) total
/// heap consumed. The old builder region is stranded in the bump
/// allocator, but each old region is at most half the size of the new
/// one, so the geometric sum is O(n).
pub fn gen_string_builder_append(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Null-builder guard: if the caller's builder is 0 (because a prior
        // `string_builder_new` OOM'd), return 0 immediately without touching
        // memory. Without this guard, the reads at BUILDER_CAPACITY_OFFSET
        // and BUILDER_LENGTH_OFFSET would target linear-memory offset 0 —
        // the reserved header area — and produce garbage capacity/length,
        // then the copy loop below would corrupt low memory.
        Instruction::LocalGet(0),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        // Read current capacity -> local 2
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: BUILDER_CAPACITY_OFFSET,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(2),
        // Read current length -> local 3
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: BUILDER_LENGTH_OFFSET,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(3),
        // Read incoming string length -> local 4
        Instruction::LocalGet(1),
        Instruction::I32Load(MemArg {
            offset: STRING_LENGTH_OFFSET as u64,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(4),
        // Fast path: if str_len == 0 there's nothing to append, just
        // return the builder unchanged. This also avoids a degenerate
        // doubling step when appending many empty strings.
        Instruction::LocalGet(4),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(0),
        Instruction::Return,
        Instruction::End,
        // needed = length + str_len -> local 5
        Instruction::LocalGet(3),
        Instruction::LocalGet(4),
        Instruction::I32Add,
        Instruction::LocalSet(5),
        // If needed > capacity, grow.
        Instruction::LocalGet(5),
        Instruction::LocalGet(2),
        Instruction::I32GtU,
        Instruction::If(BlockType::Empty),
        // new_capacity = capacity * 2
        Instruction::LocalGet(2),
        Instruction::I32Const(1),
        Instruction::I32Shl,
        Instruction::LocalSet(6),
        // If new_capacity < needed, new_capacity = needed
        Instruction::LocalGet(6),
        Instruction::LocalGet(5),
        Instruction::I32LtU,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(5),
        Instruction::LocalSet(6),
        Instruction::End,
        // new_builder_ptr = malloc(HEADER_SIZE + new_capacity)
        Instruction::LocalGet(6),
        Instruction::I32Const(BUILDER_HEADER_SIZE),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalTee(7),
        // If allocation failed, return the OLD builder pointer unchanged.
        // Returning 0 here would null out the caller's builder local, and
        // the next append call would read capacity/length from linear memory
        // offset 0 (the reserved header area), producing corrupted writes
        // and a garbage finalize pointer — the STATE A truncation family
        // documented in CODEGEN-STRING-ACCUM-LOOP-TRUNCATION. Returning the
        // old pointer instead lets the caller keep the partial content
        // that already fit; subsequent appends will keep hitting this
        // branch and no-oping, and finalize will produce a truncated but
        // structurally valid string with the pre-OOM prefix intact.
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::LocalGet(0),
        Instruction::Return,
        Instruction::End,
        // Write new capacity at new_builder_ptr + 0
        Instruction::LocalGet(7),
        Instruction::LocalGet(6),
        Instruction::I32Store(MemArg {
            offset: BUILDER_CAPACITY_OFFSET,
            align: 2,
            memory_index: 0,
        }),
        // Write old length at new_builder_ptr + 4 (length unchanged by relocation)
        Instruction::LocalGet(7),
        Instruction::LocalGet(3),
        Instruction::I32Store(MemArg {
            offset: BUILDER_LENGTH_OFFSET,
            align: 2,
            memory_index: 0,
        }),
        // Copy old content: new_builder_ptr[8..8+length] = old_builder_ptr[8..8+length]
        // Byte-by-byte loop. i = 0 -> local 8
        Instruction::I32Const(0),
        Instruction::LocalSet(8),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= length, exit
        Instruction::LocalGet(8),
        Instruction::LocalGet(3),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // new_builder_ptr[8 + i] = old_builder_ptr[8 + i]
        Instruction::LocalGet(7),
        Instruction::I32Const(BUILDER_HEADER_SIZE),
        Instruction::I32Add,
        Instruction::LocalGet(8),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Const(BUILDER_HEADER_SIZE),
        Instruction::I32Add,
        Instruction::LocalGet(8),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(8),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(8),
        Instruction::Br(0),
        Instruction::End, // end Loop
        Instruction::End, // end Block
        // Rebind: builder_ptr = new_builder_ptr; capacity = new_capacity
        Instruction::LocalGet(7),
        Instruction::LocalSet(0),
        Instruction::LocalGet(6),
        Instruction::LocalSet(2),
        Instruction::End, // end grow-needed If
        // Append the incoming string's bytes into the (possibly-relocated)
        // builder at offset HEADER_SIZE + length.
        // i = 0
        Instruction::I32Const(0),
        Instruction::LocalSet(8),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        // if i >= str_len, exit
        Instruction::LocalGet(8),
        Instruction::LocalGet(4),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // builder_ptr[HEADER_SIZE + length + i] = str_ptr[STRING_DATA_OFFSET + i]
        Instruction::LocalGet(0),
        Instruction::I32Const(BUILDER_HEADER_SIZE),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Add,
        Instruction::LocalGet(8),
        Instruction::I32Add,
        Instruction::LocalGet(1),
        Instruction::I32Const(STRING_DATA_OFFSET as i32),
        Instruction::I32Add,
        Instruction::LocalGet(8),
        Instruction::I32Add,
        Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }),
        // i++
        Instruction::LocalGet(8),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(8),
        Instruction::Br(0),
        Instruction::End, // end Loop
        Instruction::End, // end Block
        // Write back: length = needed (in local 5)
        Instruction::LocalGet(0),
        Instruction::LocalGet(5),
        Instruction::I32Store(MemArg {
            offset: BUILDER_LENGTH_OFFSET,
            align: 2,
            memory_index: 0,
        }),
        // Return the (possibly-relocated) builder pointer.
        Instruction::LocalGet(0),
    ]
}

/// Generate instructions for `__string_builder_finalize`.
///
/// Parameters:
///   - local 0: builder_ptr (i32)
///
/// Returns: i32 — a pointer to a length-prefixed Clean string whose
/// length sits at byte 0 and whose content begins at byte 4. The aliasing
/// trick: we return `builder_ptr + 4`. From that pointer, byte 0 is the
/// builder's length field (matching `STRING_LENGTH_OFFSET = 0`) and byte
/// 4 is the first content byte (matching `STRING_DATA_OFFSET = 4`).
///
/// The four bytes preceding the returned pointer hold the now-stale
/// capacity field and are unreachable from any Clean-side consumer.
pub fn gen_string_builder_finalize() -> Vec<Instruction<'static>> {
    vec![
        Instruction::LocalGet(0),
        Instruction::I32Const(4),
        Instruction::I32Add,
    ]
}

/// Generate instructions for `__string_builder_reclaim`.
///
/// Parameters:
///   - local 0: builder_ptr (i32) — pointer to the current builder region
///   - local 1: init_mark (i32) — HEAP_PTR snapshot taken right after
///     `__string_builder_new` returned, before the loop began.
///
/// Returns: void
///
/// Resolves CMP-SSR-MALLOC-OOM-PAGE-RENDER's per-iter conditional-helper
/// leak. The accumulator-loop rewrite (in `src/hir/hir_builder.rs`) turns
/// `acc = acc + e1 + ... + eN` inside a loop into a sequence of
/// `__string_builder_append` calls. That handles the accumulator's own
/// allocations. But the surrounding loop body often contains transient
/// per-iter allocations — typically conditional helpers like
/// `headHtml = "<h2>" + head + "</h2>"` — that allocate via `string.concat`
/// and never get reclaimed. Across thousands of iterations these compound
/// until the 128 MB bump cap is reached and `malloc` returns null.
///
/// The previous `body_is_iter_scope_safe`-driven `mem_scope_push` / `pop`
/// pair is disabled for accumulator-rewritten loops because a blind pop
/// would reclaim the (possibly relocated) builder region itself. This
/// helper performs a SELECTIVE reclaim: it sets `HEAP_PTR` to
/// `max(init_mark, builder_ptr + 8 + capacity)`. That preserves
/// everything below the (possibly relocated) builder's tail — including
/// the builder itself — while reclaiming every per-iter allocation made
/// above the builder.
///
/// Geometric leak accounting: any old builder region that was orphaned
/// by an intra-iter grow event is stranded below the current `builder_ptr`,
/// so this reclaim does NOT reclaim it. That's intentional and matches
/// the doubling-growth O(n) bound the accumulator rewrite already
/// accepts.
///
/// Locals:
///   - local 0: builder_ptr (parameter)
///   - local 1: init_mark (parameter)
///   - local 2: builder_end (builder_ptr + 8 + capacity, the byte just
///     past the last reachable builder byte)
pub fn gen_string_builder_reclaim() -> Vec<Instruction<'static>> {
    vec![
        // builder_end = builder_ptr + 8 + load(builder_ptr + 0 /* capacity */)
        Instruction::LocalGet(0),
        Instruction::I32Const(8),
        Instruction::I32Add,
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: 0, // BUILDER_CAPACITY_OFFSET
            align: 2,
            memory_index: 0,
        }),
        Instruction::I32Add,
        Instruction::LocalSet(2),
        // HEAP_PTR = max(init_mark, builder_end)
        // Compute via: if builder_end > init_mark then builder_end else init_mark
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32GtU,
        Instruction::If(BlockType::Result(wasm_encoder::ValType::I32)),
        Instruction::LocalGet(2),
        Instruction::Else,
        Instruction::LocalGet(1),
        Instruction::End,
        Instruction::GlobalSet(HEAP_PTR_GLOBAL),
    ]
}

/// Generate instructions for `__heap_ptr_snapshot`.
///
/// Parameters: none
/// Returns: i32 — current HEAP_PTR value.
///
/// Used by the accumulator-loop rewrite to capture the HEAP_PTR right
/// after `__string_builder_new` returns. The snapshot becomes the
/// `init_mark` argument to `__string_builder_reclaim` at the end of each
/// loop iteration. Identical to `__scope_push` in body, but kept as a
/// separate function so the rewrite never accidentally pairs with a
/// host-side `__scope_pop` that would reclaim the entire frame.
pub fn gen_heap_ptr_snapshot() -> Vec<Instruction<'static>> {
    vec![Instruction::GlobalGet(HEAP_PTR_GLOBAL)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_builder_new_emits_instructions() {
        let instructions = gen_string_builder_new(0);
        assert!(!instructions.is_empty());
        // Must call malloc to allocate the initial region.
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::Call(0))),
            "string_builder_new must call malloc"
        );
        // Must write the initial capacity (16) to offset 0.
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::I32Const(INITIAL_CAPACITY))),
            "string_builder_new must write initial capacity"
        );
    }

    #[test]
    fn test_string_builder_append_emits_grow_path() {
        let instructions = gen_string_builder_append(0);
        // Must contain a loop for byte copy and a malloc call for growth.
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::Loop(_))),
            "string_builder_append must contain copy loops"
        );
        assert!(
            instructions
                .iter()
                .any(|i| matches!(i, Instruction::Call(0))),
            "string_builder_append must call malloc when growing"
        );
        // Must read both BUILDER_CAPACITY_OFFSET and BUILDER_LENGTH_OFFSET.
        let reads_cap = instructions.iter().any(|i| {
            matches!(i, Instruction::I32Load(m)
                if m.offset == BUILDER_CAPACITY_OFFSET)
        });
        let reads_len = instructions.iter().any(|i| {
            matches!(i, Instruction::I32Load(m)
                if m.offset == BUILDER_LENGTH_OFFSET)
        });
        assert!(reads_cap, "must read builder capacity");
        assert!(reads_len, "must read builder length");
    }

    /// Regression guard for the doubling growth rule. If the implementation
    /// ever degrades to linear growth, this test catches it — without
    /// doubling, the SSR repro reverts to O(n²) and CMP-SSR-MALLOC-OOM-
    /// PAGE-RENDER reopens.
    #[test]
    fn test_string_builder_append_doubles_capacity() {
        let instructions = gen_string_builder_append(0);
        // Doubling is expressed as `capacity << 1` (I32Shl with const 1).
        let doubles = instructions.windows(3).any(|w| {
            matches!(w[0], Instruction::LocalGet(2))
                && matches!(w[1], Instruction::I32Const(1))
                && matches!(w[2], Instruction::I32Shl)
        });
        assert!(
            doubles,
            "string_builder_append must double capacity on grow \
             (capacity << 1) — anything else turns the SSR repro back \
             into O(n²)"
        );
    }

    /// Regression guard for the finalize aliasing trick. The returned
    /// pointer must be `builder + 4` so that length lives at returned_ptr
    /// byte 0 — matching `STRING_LENGTH_OFFSET = 0`.
    #[test]
    fn test_string_builder_finalize_aliases_at_plus_four() {
        let instructions = gen_string_builder_finalize();
        assert_eq!(instructions.len(), 3);
        assert!(matches!(instructions[0], Instruction::LocalGet(0)));
        assert!(matches!(instructions[1], Instruction::I32Const(4)));
        assert!(matches!(instructions[2], Instruction::I32Add));
        // Sanity: the constant must match STRING_DATA_OFFSET so the
        // aliasing trick stays consistent if the layout ever changes.
        assert_eq!(STRING_DATA_OFFSET, 4);
        assert_eq!(STRING_LENGTH_OFFSET, 0);
    }

    /// The append helper uses locals up to index 8 (parameters 0–1, scratch
    /// locals 2–8). `register_function`'s auto-detection of max-local-index
    /// must therefore declare 7 extra i32 locals. This guard fails loudly
    /// if a future edit raises the max-local-index without updating the
    /// registration site or vice versa.
    #[test]
    fn test_string_builder_append_max_local_index_is_8() {
        let instructions = gen_string_builder_append(0);
        let max = instructions
            .iter()
            .filter_map(|i| match i {
                Instruction::LocalGet(idx)
                | Instruction::LocalSet(idx)
                | Instruction::LocalTee(idx) => Some(*idx),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        assert_eq!(
            max, 8,
            "string_builder_append uses locals 0..=8 — if you've added \
             more locals, update the registration site to match"
        );
    }

    /// Regression guard for CMP-SSR-MALLOC-OOM-CONDITIONAL-HELPER. The reclaim
    /// helper MUST set HEAP_PTR to `max(init_mark, builder + 8 + capacity)`.
    /// Two failure modes the test catches:
    ///   1. Setting HEAP_PTR to init_mark unconditionally — would corrupt
    ///      the builder's relocated region.
    ///   2. Setting HEAP_PTR to builder + 8 + capacity unconditionally —
    ///      would leak the very first iteration's helpers (initial mark
    ///      is below the freshly built builder's tail).
    #[test]
    fn test_string_builder_reclaim_writes_max_of_mark_and_builder_end() {
        let instructions = gen_string_builder_reclaim();
        // Must read the builder capacity (offset 0 from builder_ptr).
        let reads_capacity = instructions.iter().any(|i| {
            matches!(i, Instruction::I32Load(m)
                if m.offset == 0)
        });
        assert!(
            reads_capacity,
            "reclaim must read the builder's capacity field"
        );
        // Must add 8 (header size) to builder_ptr to reach the end.
        let adds_header = instructions
            .iter()
            .any(|i| matches!(i, Instruction::I32Const(8)));
        assert!(
            adds_header,
            "reclaim must add the 8-byte builder header to the capacity to \
             compute the builder's end byte"
        );
        // Must compare via I32GtU (unsigned), select via If/Else.
        let has_gtu = instructions
            .iter()
            .any(|i| matches!(i, Instruction::I32GtU));
        assert!(
            has_gtu,
            "reclaim must compare builder_end vs init_mark using unsigned \
             greater-than (signed compare risks misordering pointers above 2 GB)"
        );
        // Must write the result to HEAP_PTR_GLOBAL.
        let writes_heap_ptr = instructions
            .iter()
            .any(|i| matches!(i, Instruction::GlobalSet(HEAP_PTR_GLOBAL)));
        assert!(
            writes_heap_ptr,
            "reclaim must write the computed HEAP_PTR back via GlobalSet"
        );
    }

    /// Regression guard for the heap_ptr_snapshot helper used to capture
    /// the `init_mark` argument to `string_builder_reclaim`. Must be a
    /// single `GlobalGet(HEAP_PTR_GLOBAL)` so it observes exactly the
    /// builder-just-built save-point with no side effects.
    #[test]
    fn test_heap_ptr_snapshot_is_a_single_global_get() {
        let instructions = gen_heap_ptr_snapshot();
        assert_eq!(
            instructions.len(),
            1,
            "heap_ptr_snapshot must be exactly one GlobalGet — any extra ops \
             would shift the save-point relative to the builder's actual end"
        );
        assert!(
            matches!(instructions[0], Instruction::GlobalGet(HEAP_PTR_GLOBAL)),
            "heap_ptr_snapshot must read HEAP_PTR_GLOBAL — reading any other \
             global would return the wrong save-point"
        );
    }

    /// Regression guard for CODEGEN-STRING-ACCUM-LOOP-TRUNCATION (#0d080661e5f2 /
    /// #8e9e5be5614c). Under memory pressure the grow-path malloc may fail;
    /// returning 0 to the caller nulls out the caller's builder local and
    /// the next append call reads capacity/length from linear memory offset 0
    /// (the reserved header area), producing corrupted writes and a garbage
    /// finalize pointer. The fix returns the OLD builder pointer (LocalGet 0)
    /// instead of 0 so the caller preserves the partial content already built.
    #[test]
    fn test_string_builder_append_preserves_old_ptr_on_grow_failure() {
        let instructions = gen_string_builder_append(0);
        // Find the eqz+if that guards the grow-path malloc result. The
        // instruction inside the if-body must be LocalGet(0) (the OLD builder
        // pointer), NOT I32Const(0).
        // Signature: LocalTee(7), I32Eqz, If(_), LocalGet(0), Return, End
        let idx = instructions
            .windows(6)
            .position(|w| {
                matches!(w[0], Instruction::LocalTee(7))
                    && matches!(w[1], Instruction::I32Eqz)
                    && matches!(w[2], Instruction::If(_))
                    && matches!(w[3], Instruction::LocalGet(0))
                    && matches!(w[4], Instruction::Return)
                    && matches!(w[5], Instruction::End)
            })
            .expect(
                "string_builder_append must return the OLD builder pointer (LocalGet 0) \
                 on grow-path malloc failure — returning 0 would corrupt caller state \
                 and re-open CODEGEN-STRING-ACCUM-LOOP-TRUNCATION",
            );
        assert!(
            idx > 0,
            "grow-failure guard must not be the first instruction"
        );
    }

    /// Regression guard: string_builder_append must short-circuit when its
    /// builder argument is 0. Without this guard, reads at
    /// BUILDER_CAPACITY_OFFSET / BUILDER_LENGTH_OFFSET would target linear
    /// memory offset 0 — the reserved header area — producing garbage
    /// capacity/length and corrupting low memory when the copy loop runs.
    #[test]
    fn test_string_builder_append_null_builder_guard() {
        let instructions = gen_string_builder_append(0);
        // The null guard must appear as the very first executable check.
        // Signature: LocalGet(0), I32Eqz, If(_), I32Const(0), Return, End
        assert!(
            instructions.len() >= 6,
            "string_builder_append body too short to contain a null-builder guard"
        );
        assert!(
            matches!(instructions[0], Instruction::LocalGet(0)),
            "null-builder guard must read the builder_ptr param first"
        );
        assert!(
            matches!(instructions[1], Instruction::I32Eqz),
            "null-builder guard must test the builder_ptr with I32Eqz"
        );
        assert!(
            matches!(instructions[2], Instruction::If(_)),
            "null-builder guard must enter an If block on eqz"
        );
        assert!(
            matches!(instructions[3], Instruction::I32Const(0)),
            "null-builder guard must push 0 as the return value"
        );
        assert!(
            matches!(instructions[4], Instruction::Return),
            "null-builder guard must return immediately"
        );
        assert!(
            matches!(instructions[5], Instruction::End),
            "null-builder guard must close its If block"
        );
    }
}
