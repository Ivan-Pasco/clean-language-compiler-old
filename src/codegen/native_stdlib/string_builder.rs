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
        // If allocation failed, return 0 (already on stack via LocalTee).
        Instruction::I32Eqz,
        Instruction::If(BlockType::Empty),
        Instruction::I32Const(0),
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
}
