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

use wasm_encoder::{BlockType, Instruction, MemArg};

use super::{STRING_DATA_OFFSET, STRING_LENGTH_OFFSET};

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

/// Generate instructions for `__string_builder_finalize_to_main`.
///
/// Parameters:
///   - local 0: builder_ptr (i32) — pointer to a builder region that may
///     live in the transient arena (or any region that will be reclaimed
///     before the returned string is next consumed).
///
/// Returns: i32 — a pointer to a length-prefixed Clean string whose bytes
/// live on the MAIN heap. Callers can hold the returned pointer past a
/// `__transient_scope_exit` or any main-heap reclaim of the source
/// builder's region.
///
/// This is the "escape hatch" variant of `__string_builder_finalize`.
/// The plain finalize does `return builder_ptr + 4` — the returned string
/// aliases into the builder's own bytes, so it dies whenever the
/// builder's region is reclaimed. When the builder lives in the transient
/// arena (see `native_stdlib::transient_arena`), the caller is about to
/// call `__transient_scope_exit`, which would invalidate the alias.
///
/// This variant instead:
///   1. Reads the builder's length (at offset 4).
///   2. `__malloc`s exactly `4 + length` bytes on the main heap.
///   3. Writes the length prefix to the new main-heap region.
///   4. memcpys `length` bytes from `builder_ptr + 8` (past the header) to
///      `new_ptr + 4` (past the length prefix).
///   5. Returns the new main-heap pointer.
///
/// This is what closes the SSR-LOOP-CLASS-METHOD-HTMLBLOCK-TRAP family
/// (dashboard fingerprints `5f77eb36` and `4c06a901`): the singleshot
/// accumulator rewrite (see `hir/hir_builder.rs::rewrite_string_
/// accumulator_singleshot`) can now safely route its builder through the
/// transient arena, dropping the ~500-byte-per-call main-heap footprint
/// of stranded growth stages down to just `4 + final_length` for the
/// survivor string.
///
/// Locals:
///   - local 0: builder_ptr (parameter)
///   - local 1: length (loaded from builder_ptr + 4)
///   - local 2: new_ptr (return value, from __malloc)
///   - local 3: i (byte-copy loop counter)
pub fn gen_string_builder_finalize_to_main(malloc_func: u32) -> Vec<Instruction<'static>> {
    vec![
        // Null-builder guard: if builder_ptr is 0 (upstream OOM), return 0.
        // The plain finalize returns 0+4=4, which is a bogus pointer inside
        // the reserved header area — this variant refuses to construct a
        // garbage main-heap string from a null builder.
        Instruction::LocalGet(0),
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        // length = load(builder_ptr + BUILDER_LENGTH_OFFSET) -> local 1
        Instruction::LocalGet(0),
        Instruction::I32Load(MemArg {
            offset: BUILDER_LENGTH_OFFSET,
            align: 2,
            memory_index: 0,
        }),
        Instruction::LocalSet(1),
        // new_ptr = __malloc(4 + length) -> local 2
        Instruction::LocalGet(1),
        Instruction::I32Const(4),
        Instruction::I32Add,
        Instruction::Call(malloc_func),
        Instruction::LocalTee(2),
        // If __malloc failed, propagate 0.
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
        Instruction::Return,
        Instruction::End,
        // Write length prefix at new_ptr + 0.
        Instruction::LocalGet(2),
        Instruction::LocalGet(1),
        Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }),
        // Copy loop: for i in 0..length: new_ptr[4+i] = builder_ptr[8+i]
        Instruction::I32Const(0),
        Instruction::LocalSet(3),
        Instruction::Block(BlockType::Empty),
        Instruction::Loop(BlockType::Empty),
        Instruction::LocalGet(3),
        Instruction::LocalGet(1),
        Instruction::I32GeU,
        Instruction::BrIf(1),
        // Destination address: new_ptr + 4 + i
        Instruction::LocalGet(2),
        Instruction::I32Const(4),
        Instruction::I32Add,
        Instruction::LocalGet(3),
        Instruction::I32Add,
        // Source byte: load8u from builder_ptr + BUILDER_HEADER_SIZE + i
        Instruction::LocalGet(0),
        Instruction::I32Const(BUILDER_HEADER_SIZE),
        Instruction::I32Add,
        Instruction::LocalGet(3),
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
        Instruction::LocalGet(3),
        Instruction::I32Const(1),
        Instruction::I32Add,
        Instruction::LocalSet(3),
        Instruction::Br(0),
        Instruction::End,
        Instruction::End,
        // Return new main-heap pointer.
        Instruction::LocalGet(2),
    ]
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
