//! Import and function registration for the `CodeGenerator`.
//! This module contains all `register_*` functions that set up WASM imports.

use super::native_stdlib;
use crate::error::CompilerError;
use crate::types::WasmType;
use tracing::debug;

impl super::CodeGenerator {
    /// Register file operation functions using WASM instructions from FileClass
    /// Only registers specification-compliant functions: file.read, file.write, file.append, file.exists, file.delete
    /// NOTE: File imports are now registered in register_import_functions_only() which is called first
    pub(crate) fn register_file_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::file_class::FileClass;

        // File imports are already registered by register_import_functions_only()
        // Just register the wrapper functions (file.read, file.write, etc.)
        let file_class = FileClass::new();
        file_class.register_functions(self)?;

        Ok(())
    }

    /// Register validator operation functions (validator.create, validator.ok, validator.isOk, etc.)
    pub fn register_validator_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::memory::MemoryManager;
        use crate::stdlib::validator::ValidatorManager;
        use std::cell::RefCell;
        use std::rc::Rc;

        // Create a MemoryManager and ValidatorManager instance.
        // Resolve mem_alloc index dynamically so the validator is resilient to
        // import ordering changes (e.g. when input_* imports are gated out).
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let mem_alloc_idx = self
            .get_function_index("mem_alloc")
            .unwrap_or(crate::stdlib::validator::DEFAULT_MEM_ALLOC_CALL_INDEX);
        let validator_manager =
            ValidatorManager::new_with_mem_alloc_idx(memory_manager, mem_alloc_idx);
        validator_manager.register_functions(self)?;

        Ok(())
    }

    /// Register JSON operations for parsing and stringifying JSON
    /// BOOK: json-module - pure WASM JSON implementation
    pub fn register_json_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::JsonClass;

        tracing::debug!(
            function_count = self.function_count,
            "Registering JSON operations"
        );

        let json_class = JsonClass::new();
        json_class.register_functions(self)?;

        tracing::debug!(
            function_count = self.function_count,
            "JSON operations registered successfully"
        );

        Ok(())
    }

    /// Register native memory allocation operations for standalone WASM execution
    /// These functions run entirely in WASM without host imports
    pub fn register_memory_operations(&mut self) -> Result<(), CompilerError> {
        // NATIVE: malloc - bump allocator for memory allocation
        // Uses global 0 (HEAP_PTR_GLOBAL) as heap pointer
        // Parameters: size (i32)
        // Returns: pointer (i32) to allocated memory
        let malloc_instructions = native_stdlib::memory::gen_malloc();
        let malloc_idx = self.register_function(
            "__malloc",
            &[WasmType::I32],
            Some(WasmType::I32),
            &malloc_instructions,
        )?;
        // Create alias for internal use
        self.add_function_alias("malloc", malloc_idx);

        // NATIVE: free - bump-allocator no-op companion to malloc.
        // The export must exist so host bridges that guard reclamation with
        // `if (state.exports.free)` (e.g. cln-node-server memory-runtime.js)
        // proceed. Per-block reclaim is impossible in a bump allocator;
        // per-request reclaim is provided by `scope_push` / `scope_pop`.
        // Fixes COMPILER-NO-FREE-EXPORT-LEAKS-WASM-MEMORY.
        let free_instructions = native_stdlib::memory::gen_free();
        let free_idx = self.register_function(
            "__free",
            &[WasmType::I32],
            None, // void return
            &free_instructions,
        )?;
        self.add_function_alias("free", free_idx);

        // NATIVE: scope_push - returns current __heap_ptr as a save-point.
        // Hosts call this at the start of a request, then call scope_pop with
        // the returned value at the end to reclaim every allocation made in
        // between. O(1) region-based reclaim.
        let scope_push_instructions = native_stdlib::memory::gen_scope_push();
        let scope_push_idx = self.register_function(
            "__scope_push",
            &[],
            Some(WasmType::I32),
            &scope_push_instructions,
        )?;
        self.add_function_alias("scope_push", scope_push_idx);

        // NATIVE: scope_pop - resets __heap_ptr to the supplied save-point.
        // Pairs with scope_push. Caller must not retain references into the
        // reclaimed region.
        let scope_pop_instructions = native_stdlib::memory::gen_scope_pop();
        let scope_pop_idx = self.register_function(
            "__scope_pop",
            &[WasmType::I32],
            None, // void return
            &scope_pop_instructions,
        )?;
        self.add_function_alias("scope_pop", scope_pop_idx);

        // NATIVE: memcpy - byte-by-byte memory copy
        // Parameters: dest (i32), src (i32), len (i32)
        // Returns: void
        let memcpy_instructions = native_stdlib::memory::gen_memcpy();
        self.register_function(
            "__memcpy",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            None, // void return
            &memcpy_instructions,
        )?;
        // Create alias for internal use
        if let Some(memcpy_idx) = self.get_function_index("__memcpy") {
            self.add_function_alias("memcpy", memcpy_idx);
        }

        // NATIVE: string_concat - concatenates two strings using malloc
        // Parameters: str1_ptr (i32), str2_ptr (i32)
        // Returns: pointer (i32) to new concatenated string
        let concat_instructions = native_stdlib::string_ops::gen_concat(malloc_idx);
        self.register_function(
            "__string_concat",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &concat_instructions,
        )?;
        // NOTE: Do NOT alias "string.concat" here - that would overwrite the host import
        // registered separately. The host import is preferred because it
        // handles memory allocation more reliably in the runtime environment.

        // NATIVE: string_builder_new/_append/_finalize — doubling-capacity
        // growable buffer used by the HIR-level accumulator-loop rewrite
        // that resolves CMP-SSR-MALLOC-OOM-PAGE-RENDER. See
        // `src/codegen/native_stdlib/string_builder.rs` for layout and
        // `src/hir/hir_builder.rs::rewrite_string_accumulator_loops` for
        // the pattern that emits these calls.
        //
        // The trio is registered here (next to __string_concat) so they
        // share the same `malloc_idx` and live in the same WASM type
        // partition as every other native allocator helper.
        let sb_new_instructions = native_stdlib::string_builder::gen_string_builder_new(malloc_idx);
        let sb_new_idx = self.register_function_with_locals(
            "__string_builder_new",
            &[],
            Some(WasmType::I32),
            &[WasmType::I32], // local 0: builder_ptr
            &sb_new_instructions,
        )?;
        self.add_function_alias("string_builder_new", sb_new_idx);

        let sb_append_instructions =
            native_stdlib::string_builder::gen_string_builder_append(malloc_idx);
        let sb_append_idx = self.register_function_with_locals(
            "__string_builder_append",
            &[WasmType::I32, WasmType::I32], // builder_ptr, str_ptr
            Some(WasmType::I32),             // returns possibly-relocated builder
            &[
                WasmType::I32, // local 2: capacity
                WasmType::I32, // local 3: length
                WasmType::I32, // local 4: str_len
                WasmType::I32, // local 5: needed
                WasmType::I32, // local 6: new_capacity
                WasmType::I32, // local 7: new_builder_ptr
                WasmType::I32, // local 8: i
            ],
            &sb_append_instructions,
        )?;
        self.add_function_alias("string_builder_append", sb_append_idx);

        let sb_finalize_instructions = native_stdlib::string_builder::gen_string_builder_finalize();
        let sb_finalize_idx = self.register_function(
            "__string_builder_finalize",
            &[WasmType::I32], // builder_ptr
            Some(WasmType::I32),
            &sb_finalize_instructions,
        )?;
        self.add_function_alias("string_builder_finalize", sb_finalize_idx);

        // NATIVE: string_builder_reclaim — selective per-iter heap reclaim
        // for accumulator-rewritten loops. Resets HEAP_PTR to
        // `max(init_mark, builder_ptr + 8 + capacity)`, freeing every
        // transient allocation made above the builder's tail this iter.
        // Closes the per-iter conditional-helper leak path that left
        // CMP-SSR-MALLOC-OOM-PAGE-RENDER reproducible after the 0.30.368
        // chained-RHS matcher.
        let sb_reclaim_instructions = native_stdlib::string_builder::gen_string_builder_reclaim();
        let sb_reclaim_idx = self.register_function_with_locals(
            "__string_builder_reclaim",
            &[WasmType::I32, WasmType::I32], // builder_ptr, init_mark
            None,                            // void
            &[WasmType::I32],                // local 2: builder_end
            &sb_reclaim_instructions,
        )?;
        self.add_function_alias("string_builder_reclaim", sb_reclaim_idx);

        // NATIVE: heap_ptr_snapshot — capture the current HEAP_PTR value.
        // Paired with `string_builder_reclaim` at the bottom of each
        // accumulator-rewritten loop iteration. Distinct from
        // `__scope_push` so the rewrite cannot be confused with a
        // host-side reclaim frame.
        let snapshot_instructions = native_stdlib::string_builder::gen_heap_ptr_snapshot();
        let snapshot_idx = self.register_function(
            "__heap_ptr_snapshot",
            &[],
            Some(WasmType::I32),
            &snapshot_instructions,
        )?;
        self.add_function_alias("heap_ptr_snapshot", snapshot_idx);

        // NATIVE: transient arena (transient_scope_enter / _exit /
        // transient_alloc) — second bump region for per-iteration
        // intermediates inside accumulator-rewritten loops. Replaces the
        // rolled-back `string_builder_reclaim` mechanism with a region
        // discipline that cannot dangle live pointers: outer-scope
        // values stay on the main heap, body-local intermediates flow
        // through the transient pool, and the per-iter scope_exit
        // resets only the transient pool. See
        // `native_stdlib/transient_arena.rs` for the architectural
        // motivation (Cyclone-style nested regions) and
        // CMP-SSR-RECLAIM-FREES-LIVE-POINTER for the failure mode this
        // pattern is designed to prevent. Registered here so they share
        // `malloc_idx` with the rest of the native allocator family.
        // The HIR rewriter starts emitting calls to these helpers in a
        // follow-up commit; this commit only adds infrastructure.
        let transient_enter_instructions =
            native_stdlib::transient_arena::gen_transient_scope_enter(malloc_idx);
        let transient_enter_idx = self.register_function_with_locals(
            "__transient_scope_enter",
            &[],
            Some(WasmType::I32),
            &[WasmType::I32], // local 0: pool_ptr (used during lazy init)
            &transient_enter_instructions,
        )?;
        self.add_function_alias("transient_scope_enter", transient_enter_idx);

        let transient_exit_instructions =
            native_stdlib::transient_arena::gen_transient_scope_exit();
        let transient_exit_idx = self.register_function(
            "__transient_scope_exit",
            &[WasmType::I32], // mark
            None,             // void
            &transient_exit_instructions,
        )?;
        self.add_function_alias("transient_scope_exit", transient_exit_idx);

        let transient_alloc_instructions =
            native_stdlib::transient_arena::gen_transient_alloc(malloc_idx);
        let transient_alloc_idx = self.register_function_with_locals(
            "__transient_alloc",
            &[WasmType::I32], // size
            Some(WasmType::I32),
            &[
                WasmType::I32, // local 1: base
                WasmType::I32, // local 2: ptr (saved TRANSIENT_PTR)
                WasmType::I32, // local 3: aligned_size
                WasmType::I32, // local 4: new_ptr
            ],
            &transient_alloc_instructions,
        )?;
        self.add_function_alias("transient_alloc", transient_alloc_idx);

        // NATIVE: string_concat_transient — identical to __string_concat
        // but routes the result allocation through __transient_alloc
        // instead of __malloc. Used by the MIR builder when a string
        // concat's result is consumed inside an accumulator-rewritten
        // loop body (the transient scope established by Step 2). The
        // routing decision happens at MIR-build time based on the
        // lexical scope of the destination value — see
        // `src/mir/mir_builder/expressions.rs::is_string_concat` path.
        //
        // The body is exactly the same shape as __string_concat:
        // gen_concat is allocator-agnostic, parameterized over the
        // function index that performs the result allocation. Calling
        // it with transient_alloc_idx produces a concat that lands its
        // result in the transient pool. Caller responsibility: only
        // use this variant when the result is provably body-local
        // within a `transient_scope_enter` / `transient_scope_exit`
        // pair, otherwise the result is dangling after the scope exit.
        let concat_transient_instructions =
            native_stdlib::string_ops::gen_concat(transient_alloc_idx);
        let concat_transient_idx = self.register_function(
            "__string_concat_transient",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &concat_transient_instructions,
        )?;
        self.add_function_alias("string_concat_transient", concat_transient_idx);

        // NATIVE: string_index_of - finds substring in string
        // Parameters: str_ptr (i32), search_ptr (i32)
        // Returns: index (i32) or -1 if not found
        let string_index_of_instructions = native_stdlib::string_ops::gen_index_of();
        let string_index_of_idx = self.register_function(
            "__string_index_of",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &string_index_of_instructions,
        )?;
        self.add_function_alias("string.indexOf", string_index_of_idx);

        // NATIVE: string_index_of_from - finds substring in string starting from index
        // Parameters: str_ptr (i32), search_ptr (i32), start_index (i32)
        // Returns: index (i32) or -1 if not found
        let string_index_of_from_instructions = native_stdlib::string_ops::gen_index_of_from();
        let string_index_of_from_idx = self.register_function_with_locals(
            "__string_index_of_from",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // search_len
                WasmType::I32, // i
                WasmType::I32, // j
                WasmType::I32, // match
            ],
            &string_index_of_from_instructions,
        )?;
        self.add_function_alias("string.indexOfFrom", string_index_of_from_idx);

        // NATIVE: string_contains - checks if string contains substring
        // Parameters: str_ptr (i32), search_ptr (i32)
        // Returns: boolean (i32)
        let string_contains_instructions =
            native_stdlib::string_ops::gen_contains(string_index_of_idx);
        let string_contains_idx = self.register_function(
            "__string_contains",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &string_contains_instructions,
        )?;
        self.add_function_alias("string.contains", string_contains_idx);

        // NATIVE: string_last_index_of - finds last occurrence of substring in string
        // Parameters: str_ptr (i32), search_ptr (i32)
        // Returns: index (i32) or -1 if not found
        let string_last_index_of_instructions = native_stdlib::string_ops::gen_last_index_of();
        let string_last_index_of_idx = self.register_function_with_locals(
            "__string_last_index_of",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // 6 extra locals
            &string_last_index_of_instructions,
        )?;
        self.add_function_alias("string.lastIndexOf", string_last_index_of_idx);

        // NATIVE: string_last_index_of_from - finds last occurrence of substring starting from index
        // Parameters: str_ptr (i32), search_ptr (i32), start_index (i32)
        // Returns: index (i32) or -1 if not found
        let string_last_index_of_from_instructions =
            native_stdlib::string_ops::gen_last_index_of_from();
        let string_last_index_of_from_idx = self.register_function_with_locals(
            "__string_last_index_of_from",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // search_len
                WasmType::I32, // i
                WasmType::I32, // j
                WasmType::I32, // match
                WasmType::I32, // result
            ],
            &string_last_index_of_from_instructions,
        )?;
        self.add_function_alias("string.lastIndexOfFrom", string_last_index_of_from_idx);

        // NATIVE: string_starts_with - checks if string starts with prefix
        // Parameters: str_ptr (i32), prefix_ptr (i32)
        // Returns: boolean (i32)
        let string_starts_with_instructions = native_stdlib::string_ops::gen_starts_with();
        let string_starts_with_idx = self.register_function_with_locals(
            "__string_starts_with",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32], // 3 extra locals
            &string_starts_with_instructions,
        )?;
        self.add_function_alias("string.startsWith", string_starts_with_idx);

        // NATIVE: string_ends_with - checks if string ends with suffix
        // Parameters: str_ptr (i32), suffix_ptr (i32)
        // Returns: boolean (i32)
        let string_ends_with_instructions = native_stdlib::string_ops::gen_ends_with();
        let string_ends_with_idx = self.register_function_with_locals(
            "__string_ends_with",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // 4 extra locals
            &string_ends_with_instructions,
        )?;
        self.add_function_alias("string.endsWith", string_ends_with_idx);

        // NATIVE: string_substring - extracts a substring from a string
        // Parameters: str_ptr (i32), start (i32), end (i32)
        // Returns: pointer (i32) to new string
        // Locals: str_len, new_len, new_ptr, i — needed because gen_substring
        // now clamps signed indices per spec (stdlib-reference.md line 165);
        // reading `str_len` requires a scratch local. Was 3 locals pre-clamp.
        let string_substring_instructions = native_stdlib::string_ops::gen_substring(malloc_idx);
        let string_substring_idx = self.register_function_with_locals(
            "__string_substring",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            &string_substring_instructions,
        )?;
        self.add_function_alias("string.substring", string_substring_idx);

        // NATIVE: string_from_char_code - converts integer code point to single-char string
        // Parameters: code_point (i32)
        // Returns: pointer (i32) to new 1-char string
        let from_char_code_instructions = native_stdlib::string_ops::gen_from_char_code(malloc_idx);
        let from_char_code_idx = self.register_function_with_locals(
            "__string_from_char_code",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local 1: new_ptr
            &from_char_code_instructions,
        )?;
        self.add_function_alias("string.fromCharCode", from_char_code_idx);

        // NATIVE: int_to_string - converts integer to string using malloc
        // Parameters: value (i32)
        // Returns: pointer (i32) to new string
        let int_to_string_instructions =
            native_stdlib::type_conversions::gen_int_to_string(malloc_idx);
        self.register_function(
            "__int_to_string",
            &[WasmType::I32],
            Some(WasmType::I32),
            &int_to_string_instructions,
        )?;
        // Create aliases - int_to_string is the internal name used throughout the pipeline
        if let Some(its_idx) = self.get_function_index("__int_to_string") {
            self.add_function_alias("int_to_string", its_idx);
            self.add_function_alias("integer.toString", its_idx);
        }

        // NATIVE: int64_to_string - converts 64-bit integer to string using malloc
        // Parameter local 0 is i64; the function mirrors __int_to_string but with i64
        // arithmetic so values outside the i32 range round-trip correctly. Locals 1, 3,
        // 5, 6, 7 are i32 (flag, counts, pointers); locals 2 and 4 are i64 (abs_value, temp).
        let int64_to_string_instructions =
            native_stdlib::type_conversions::gen_int64_to_string(malloc_idx);
        self.register_function_with_locals(
            "__int64_to_string",
            &[WasmType::I64],
            Some(WasmType::I32),
            &[
                WasmType::I32, // local 1: is_negative
                WasmType::I64, // local 2: abs_value
                WasmType::I32, // local 3: digit_count
                WasmType::I64, // local 4: temp
                WasmType::I32, // local 5: new_ptr
                WasmType::I32, // local 6: write_pos
                WasmType::I32, // local 7: digit
            ],
            &int64_to_string_instructions,
        )?;
        if let Some(idx) = self.get_function_index("__int64_to_string") {
            self.add_function_alias("int64_to_string", idx);
        }

        // NATIVE: bool_to_string - returns pointer to pre-allocated "true" or "false" string
        // Parameters: bool_value (i32, 0 or non-zero)
        // Returns: pointer (i32) to "true" or "false" string
        let true_ptr = self.add_string_to_pool("true");
        let false_ptr = self.add_string_to_pool("false");
        let bool_to_string_instructions =
            native_stdlib::type_conversions::gen_bool_to_string(true_ptr, false_ptr);
        self.register_function(
            "__bool_to_string",
            &[WasmType::I32],
            Some(WasmType::I32),
            &bool_to_string_instructions,
        )?;
        // Create aliases - bool_to_string is the internal name used throughout the pipeline
        if let Some(bts_idx) = self.get_function_index("__bool_to_string") {
            self.add_function_alias("bool_to_string", bts_idx);
            self.add_function_alias("boolean.toString", bts_idx);
        }

        // NATIVE: string_to_int - parses decimal string to integer
        // Parameters: str_ptr (i32)
        // Returns: integer (i32)
        let string_to_int_instructions = native_stdlib::type_conversions::gen_string_to_int();
        self.register_function(
            "__string_to_int",
            &[WasmType::I32],
            Some(WasmType::I32),
            &string_to_int_instructions,
        )?;
        // Create aliases - string_to_int is the internal name used throughout the pipeline
        if let Some(sti_idx) = self.get_function_index("__string_to_int") {
            self.add_function_alias("string_to_int", sti_idx);
            self.add_function_alias("string.toInteger", sti_idx);
        }

        // Route string.toNumber to the host-imported string_to_float parser.
        // Without this alias, string.toNumber resolves to the generic value-decoding
        // body from method_style.rs which reads the length prefix as an integer and
        // converts THAT to f64 — so "3.14".toNumber() returns 4.0. Aliasing here mirrors
        // the string.toInteger → __string_to_int wiring above.
        if let Some(stf_idx) = self.get_function_index("string_to_float") {
            self.add_function_alias("string.toNumber", stf_idx);
        }

        // NATIVE: list_get_i32 - get element from i32 list
        // Parameters: list_ptr (i32), index (i32)
        // Returns: element (i32)
        let list_get_i32_instructions = native_stdlib::list_ops::gen_get_i32();
        self.register_function(
            "__list_get_i32",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_get_i32_instructions,
        )?;
        if let Some(lg_idx) = self.get_function_index("__list_get_i32") {
            self.add_function_alias("list.get", lg_idx);
        }

        // NATIVE: list_set_i32 - set element in i32 list
        // Parameters: list_ptr (i32), index (i32), value (i32)
        // Returns: list_ptr (i32) for chaining
        let list_set_i32_instructions = native_stdlib::list_ops::gen_set_i32();
        self.register_function(
            "__list_set_i32",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_set_i32_instructions,
        )?;
        if let Some(ls_idx) = self.get_function_index("__list_set_i32") {
            self.add_function_alias("list.set", ls_idx);
        }

        // NATIVE: list_pop_i32 - remove and return last element
        // Parameters: list_ptr (i32)
        // Returns: element (i32)
        let list_pop_i32_instructions = native_stdlib::list_ops::gen_pop_i32();
        self.register_function(
            "__list_pop_i32",
            &[WasmType::I32],
            Some(WasmType::I32),
            &list_pop_i32_instructions,
        )?;
        if let Some(lp_idx) = self.get_function_index("__list_pop_i32") {
            self.add_function_alias("list.pop", lp_idx);
        }

        // NATIVE: list_index_of_i32 - find index of element
        // Parameters: list_ptr (i32), value (i32)
        // Returns: index (i32) or -1 if not found
        let list_index_of_i32_instructions = native_stdlib::list_ops::gen_index_of_i32();
        let list_index_of_idx = self.register_function(
            "__list_index_of_i32",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_index_of_i32_instructions,
        )?;
        self.add_function_alias("list.indexOf", list_index_of_idx);

        // NATIVE: list_contains_i32 - check if list contains element
        // Parameters: list_ptr (i32), value (i32)
        // Returns: boolean (i32)
        let list_contains_i32_instructions =
            native_stdlib::list_ops::gen_contains_i32(list_index_of_idx);
        self.register_function(
            "__list_contains_i32",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_contains_i32_instructions,
        )?;
        if let Some(lc_idx) = self.get_function_index("__list_contains_i32") {
            self.add_function_alias("list.contains", lc_idx);
        }

        // NATIVE: list_push_i32 - add element to end of list (IN-PLACE MUTATION)
        // This mutates the list in place - no reallocation, no new pointer.
        // Safe because empty lists are pre-allocated with capacity 8.
        // Parameters: list_ptr (i32), value (i32)
        // Returns: list_ptr (i32) - same pointer, mutated in place
        let list_push_i32_instructions = native_stdlib::list_ops::gen_push_i32_inplace();
        self.register_function(
            "__list_push_i32",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &list_push_i32_instructions,
        )?;
        if let Some(lpu_idx) = self.get_function_index("__list_push_i32") {
            self.add_function_alias("list.push", lpu_idx);
            self.add_function_alias("array_push", lpu_idx);
        }

        Ok(())
    }

    /// Register list operation functions using WASM instructions from ListManager
    pub(crate) fn register_list_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::list_ops::ListManager;
        use crate::stdlib::memory::MemoryManager;
        use std::cell::RefCell;
        use std::rc::Rc;

        // Create a MemoryManager and ListManager instance
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(16))));
        let list_manager = ListManager::new(memory_manager);
        list_manager.register_functions(self)?;

        Ok(())
    }

    /// Register HTTP operation functions using HttpClass
    pub(crate) fn register_http_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::http_class::HttpClass;

        // Create an HttpClass instance and register its functions
        let http_class = HttpClass::new();
        http_class.register_functions(self)?;

        Ok(())
    }

    /// Register math operation functions using MathClass
    pub(crate) fn register_math_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::math_class::MathClass;
        use crate::types::WasmType;

        // FIRST: Register math imports that MathClass will alias
        // These must be registered before MathClass tries to create aliases

        // Core trig functions
        self.register_import_function(
            "env",
            "math_pow",
            &[WasmType::F64, WasmType::F64],
            Some(WasmType::F64),
        )?;
        self.register_import_function("env", "math_sin", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_cos", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_tan", &[WasmType::F64], Some(WasmType::F64))?;

        // Inverse trig functions
        self.register_import_function("env", "math_asin", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_acos", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_atan", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function(
            "env",
            "math_atan2",
            &[WasmType::F64, WasmType::F64],
            Some(WasmType::F64),
        )?;

        // Hyperbolic functions
        self.register_import_function("env", "math_sinh", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_cosh", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_tanh", &[WasmType::F64], Some(WasmType::F64))?;

        // Logarithmic and exponential functions
        self.register_import_function("env", "math_ln", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_log10", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_log2", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_exp", &[WasmType::F64], Some(WasmType::F64))?;
        self.register_import_function("env", "math_exp2", &[WasmType::F64], Some(WasmType::F64))?;

        // Random number generation — returns f64 in [0.0, 1.0)
        self.register_import_function("env", "math_random", &[], Some(WasmType::F64))?;

        // THEN: Create MathClass instance which creates aliases and native functions
        let math_class = MathClass::new();
        math_class.register_functions(self)?;

        Ok(())
    }

    /// Register string.split as an import
    /// This must be called BEFORE any stdlib functions are registered
    /// because WASM requires all imports to come before internal functions
    pub fn register_string_split_import(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        // Register string.split as an import that routes to the runtime function
        // This takes (string_ptr, delimiter_ptr) and returns a list pointer
        self.register_import_function(
            "env",
            "string.split",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;

        Ok(())
    }

    /// Register string trim functions as native WASM implementations
    /// These are pure WASM functions that trim whitespace from strings without host imports
    pub fn register_string_trim_imports(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        // Get malloc function index - required for allocating new trimmed strings
        let malloc_idx = self
            .get_function_index("__malloc")
            .or_else(|| self.get_function_index("malloc"))
            .ok_or_else(|| {
                CompilerError::codegen_error(
                    "malloc not registered before trim functions",
                    None,
                    None,
                )
            })?;

        // NATIVE: string_trim - trims whitespace from both ends
        // Parameters: str_ptr (i32)
        // Returns: pointer (i32) to new trimmed string
        let trim_instructions = native_stdlib::string_ops::gen_trim(malloc_idx);
        let trim_idx = self.register_function_with_locals(
            "__string_trim",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // start_idx
                WasmType::I32, // end_idx
                WasmType::I32, // new_len
                WasmType::I32, // new_ptr
                WasmType::I32, // i
                WasmType::I32, // temp byte
            ],
            &trim_instructions,
        )?;

        // NATIVE: string_trim_start - trims whitespace from start
        // Parameters: str_ptr (i32)
        // Returns: pointer (i32) to new trimmed string
        let trim_start_instructions = native_stdlib::string_ops::gen_trim_start(malloc_idx);
        let trim_start_idx = self.register_function_with_locals(
            "__string_trim_start",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // start_idx
                WasmType::I32, // new_len
                WasmType::I32, // new_ptr
                WasmType::I32, // i
                WasmType::I32, // temp byte
            ],
            &trim_start_instructions,
        )?;

        // NATIVE: string_trim_end - trims whitespace from end
        // Parameters: str_ptr (i32)
        // Returns: pointer (i32) to new trimmed string
        let trim_end_instructions = native_stdlib::string_ops::gen_trim_end(malloc_idx);
        let trim_end_idx = self.register_function_with_locals(
            "__string_trim_end",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[
                WasmType::I32, // str_len
                WasmType::I32, // end_idx
                WasmType::I32, // new_ptr
                WasmType::I32, // i
                WasmType::I32, // temp byte
            ],
            &trim_end_instructions,
        )?;

        // Add canonical dot-notation aliases
        self.add_function_alias("string.trim", trim_idx);
        self.add_function_alias("string.trimStart", trim_start_idx);
        self.add_function_alias("string.trimEnd", trim_end_idx);

        // NATIVE: string_to_upper - converts string to uppercase (ASCII)
        let to_upper_instructions = native_stdlib::string_ops::gen_to_upper(malloc_idx);
        let to_upper_idx = self.register_function_with_locals(
            "__string_to_upper",
            &[WasmType::I32],    // str_ptr
            Some(WasmType::I32), // returns new_ptr
            &[
                WasmType::I32, // str_len
                WasmType::I32, // new_ptr
                WasmType::I32, // i (loop counter)
                WasmType::I32, // ch
            ],
            &to_upper_instructions,
        )?;
        self.add_function_alias("string.toUpperCase", to_upper_idx);

        // NATIVE: string_to_lower - converts string to lowercase (ASCII)
        let to_lower_instructions = native_stdlib::string_ops::gen_to_lower(malloc_idx);
        let to_lower_idx = self.register_function_with_locals(
            "__string_to_lower",
            &[WasmType::I32],    // str_ptr
            Some(WasmType::I32), // returns new_ptr
            &[
                WasmType::I32, // str_len
                WasmType::I32, // new_ptr
                WasmType::I32, // i (loop counter)
                WasmType::I32, // ch
            ],
            &to_lower_instructions,
        )?;
        self.add_function_alias("string.toLowerCase", to_lower_idx);

        Ok(())
    }

    /// Register string_compare function import
    /// This is used for string equality comparisons (==, !=)
    pub fn register_string_compare_import(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        // Register string_compare as an import that compares two strings
        // Returns 1 if equal, 0 if not equal
        let idx = self.register_import_function(
            "env",
            "string_compare",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;

        // Add canonical alias — skip when the import was tree-shaken (u32::MAX sentinel)
        // to avoid inserting a bogus index that would generate an invalid Call instruction.
        if idx != u32::MAX {
            self.add_function_alias("string.compare", idx);
        }

        Ok(())
    }

    /// Register string_replace function import
    /// This is used for the string.replace() method
    pub fn register_string_replace_import(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        // Register string_replace as an import that replaces all occurrences of a substring
        // Parameters: (string_ptr: i32, search_ptr: i32, replace_ptr: i32) -> i32
        let idx = self.register_import_function(
            "env",
            "string_replace",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;

        // Add canonical aliases — skip when the import was tree-shaken (u32::MAX sentinel)
        // to avoid inserting a bogus index that would generate an invalid Call instruction.
        if idx != u32::MAX {
            self.add_function_alias("string.replace", idx);
            // replaceAll uses the same host function (replaces all occurrences)
            self.add_function_alias("string.replaceAll", idx);
        }

        Ok(())
    }

    /// Register string_repeat as a host import.
    /// Signature: (str_ptr: i32, str_len: i32, count: i32) -> i32  (result ptr)
    /// Register the raw `string_repeat` host import (imports phase only).
    /// Call `register_string_repeat_wrapper()` AFTER all imports.
    pub fn register_string_repeat_import(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        self.register_import_function(
            "env",
            "string_repeat",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        Ok(())
    }

    /// Create the `string.repeat(str_ptr, count)` WASM wrapper that reads the
    /// length from memory and calls the raw `string_repeat(ptr, len, count)` host.
    /// Must be called AFTER all imports.
    pub fn register_string_repeat_wrapper(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        use wasm_encoder::Instruction;

        let raw_idx = match self.function_map.get("string_repeat").copied() {
            Some(idx) => idx,
            None => return Ok(()), // import not registered
        };

        let wrapper_instructions = vec![
            Instruction::LocalGet(0), // str_ptr
            Instruction::LocalGet(0), // str_ptr again — to load len
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // len = i32.load(str_ptr)
            Instruction::LocalGet(1), // count
            Instruction::Call(raw_idx),
        ];
        let wrapper_idx = self.register_function(
            "__string_repeat_wrapper",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &wrapper_instructions,
        )?;
        self.add_function_alias("string.repeat", wrapper_idx);
        Ok(())
    }

    /// Register the raw `string_matches` host import (imports phase only).
    /// Signature: (str_ptr: i32, str_len: i32, pattern_id: i32) -> i32
    /// pattern_id is resolved at compile time: email=0 url=1 uuid=2 phone=3 date=4 integer=5 number=6 alphanumeric=7
    /// Call `register_string_matches_wrapper()` AFTER all imports.
    pub fn register_string_matches_import(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        self.register_import_function(
            "env",
            "string_matches",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;
        Ok(())
    }

    /// Create the `string.matches(str_ptr, pattern_id)` WASM wrapper.
    /// Reads str_len from memory at str_ptr[0] and calls the 3-param host import.
    /// Must be called AFTER all imports.
    pub fn register_string_matches_wrapper(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        use wasm_encoder::Instruction;

        let raw_idx = match self.function_map.get("string_matches").copied() {
            Some(idx) => idx,
            None => return Ok(()),
        };

        let wrapper_instructions = vec![
            Instruction::LocalGet(0), // str_ptr
            Instruction::LocalGet(0), // str_ptr for len load
            Instruction::I32Load(wasm_encoder::MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // str_len from memory
            Instruction::LocalGet(1), // pattern_id (compile-time integer)
            Instruction::Call(raw_idx),
        ];
        let wrapper_idx = self.register_function(
            "__string_matches_wrapper",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &wrapper_instructions,
        )?;
        self.add_function_alias("string.matches", wrapper_idx);
        Ok(())
    }

    /// Register raw imports for the endpoint-test bridge functions (imports phase only).
    /// `_test_http_request`:    10 x i32 (5 str ptr+len pairs) -> i32 handle
    /// `_test_response_status`: (handle: i32) -> i32
    /// `_test_response_body`:   (handle: i32) -> i32 (string structure ptr)
    /// Call `register_test_bridge_wrappers()` AFTER all imports.
    pub fn register_test_bridge_imports(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        self.register_import_function(
            "env",
            "_test_http_request",
            &[
                WasmType::I32,
                WasmType::I32, // method ptr+len
                WasmType::I32,
                WasmType::I32, // path ptr+len
                WasmType::I32,
                WasmType::I32, // body ptr+len
                WasmType::I32,
                WasmType::I32, // header-key ptr+len
                WasmType::I32,
                WasmType::I32, // header-value ptr+len
            ],
            Some(WasmType::I32),
        )?;
        self.register_import_function(
            "env",
            "_test_response_status",
            &[WasmType::I32],
            Some(WasmType::I32),
        )?;
        self.register_import_function(
            "env",
            "_test_response_body",
            &[WasmType::I32],
            Some(WasmType::I32),
        )?;
        Ok(())
    }

    /// Create the `_test_http_request_clean` WASM wrapper.
    /// Takes 5 string structure pointers (each pointing to [len:4][data]), reads their
    /// lengths from memory, and calls the `_test_http_request` host import with 10 i32 params.
    /// Must be called AFTER all imports.
    pub fn register_test_bridge_wrappers(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        use wasm_encoder::Instruction;

        let raw_idx = match self.function_map.get("_test_http_request").copied() {
            Some(idx) => idx,
            None => return Ok(()),
        };

        // Wrapper: (method_ptr, path_ptr, body_ptr, hk_ptr, hv_ptr) -> handle
        // For each string ptr N: push ptr+4 (content), push mem[ptr] (len)
        let expand = |local: u32| {
            vec![
                Instruction::LocalGet(local),
                Instruction::I32Const(4),
                Instruction::I32Add, // content_ptr = ptr + 4
                Instruction::LocalGet(local),
                Instruction::I32Load(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // len = mem[ptr]
            ]
        };

        let mut instructions: Vec<Instruction> = Vec::new();
        for local in 0u32..5 {
            instructions.extend(expand(local));
        }
        instructions.push(Instruction::Call(raw_idx));

        let wrapper_idx = self.register_function(
            "_test_http_request_clean",
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ],
            Some(WasmType::I32),
            &instructions,
        )?;
        self.add_function_alias("_test_http_request_clean", wrapper_idx);
        Ok(())
    }

    /// Register string class operation functions using StringClass
    pub(crate) fn register_string_class_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::string_class::StringClass;

        // NOTE: string.split import is registered separately via register_string_split_import()
        // NOTE: string trim imports are registered separately via register_string_trim_imports()
        // NOTE: string replace import is registered separately via register_string_replace_import()
        // These are called earlier in the initialization sequence to ensure correct WASM indexing
        // Do NOT register them here to avoid duplicate registration issues

        // Create a StringClass instance and register its functions
        debug!("Creating StringClass instance");
        let string_class = StringClass::new();
        debug!(
            function_count = self.function_count,
            "Calling string_class.register_functions()"
        );
        string_class.register_functions(self)?;
        debug!(
            function_count = self.function_count,
            "StringClass registration completed"
        );

        Ok(())
    }

    /// Register list class operation functions using ListClass
    pub(crate) fn register_list_class_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::list_class::ListClass;

        // Create a ListClass instance with the dynamically resolved mem_alloc index
        // so list operations are resilient to import ordering changes.
        let mem_alloc_idx = self
            .get_function_index("mem_alloc")
            .unwrap_or(crate::stdlib::list_class::DEFAULT_MEM_ALLOC_CALL_INDEX_PUB);
        let list_class = ListClass::new_with_mem_alloc_idx(mem_alloc_idx);
        list_class.register_functions(self)?;

        Ok(())
    }

    /// Register method-style and list-behavior operations (isEmpty, isDefined, list.size, etc.)
    /// Register async host bridge imports: _async_fire and _async_await.
    ///
    /// Signatures (platform-architecture/HOST_BRIDGE.md):
    ///   _async_fire  (fn_name_ptr: i32, fn_name_len: i32, args_ptr: i32, args_len: i32) -> void
    ///   _async_await (fn_name_ptr: i32, fn_name_len: i32, args_ptr: i32, args_len: i32) -> i32
    pub(crate) fn register_async_operations(&mut self) -> Result<(), CompilerError> {
        use crate::types::WasmType;

        self.register_import_function(
            "env",
            "_async_fire",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            None,
        )?;

        self.register_import_function(
            "env",
            "_async_await",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
        )?;

        Ok(())
    }

    /// Register native WASM pairs/map operations.
    ///
    /// Must be called AFTER `register_memory_operations()` so that `__malloc` is already
    /// in the function table.
    pub(crate) fn register_pairs_operations(&mut self) -> Result<(), CompilerError> {
        // Resolve malloc index — required by pairs_new to allocate map storage.
        let malloc_idx = self
            .get_function_index("__malloc")
            .or_else(|| self.get_function_index("malloc"))
            .expect("malloc not registered before pairs operations");

        // __pairs_str_eq(ptr_a: i32, ptr_b: i32) -> i32
        // Returns 1 if the two Clean Language strings are byte-equal, 0 otherwise.
        // Needs 2 extra locals: len_a (i32), i (i32).
        let str_eq_instructions = native_stdlib::pairs_ops::gen_str_eq();
        let str_eq_idx = self.register_function_with_locals(
            "__pairs_str_eq",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32], // len_a, i
            &str_eq_instructions,
        )?;
        self.add_function_alias("pairs.str_eq", str_eq_idx);

        // __pairs_new(capacity: i32) -> i32
        // Allocates a new empty pairs map with the given initial capacity.
        // Uses 1 extra local: ptr (i32) to avoid stack juggling.
        let pairs_new_instructions = native_stdlib::pairs_ops::gen_pairs_new_impl(malloc_idx);
        let pairs_new_idx = self.register_function_with_locals(
            "__pairs_new",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // ptr scratch
            &pairs_new_instructions,
        )?;
        self.add_function_alias("pairs.new", pairs_new_idx);
        self.add_function_alias("pairs.allocate", pairs_new_idx);

        // __pairs_set(map_ptr: i32, key_ptr: i32, val_ptr: i32) -> void
        // Inserts or updates a key/value pair.
        // Needs 3 extra locals: count, i, entry_ptr.
        let pairs_set_instructions = native_stdlib::pairs_ops::gen_pairs_set(str_eq_idx);
        let pairs_set_idx = self.register_function_with_locals(
            "__pairs_set",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            None,                                           // void return
            &[WasmType::I32, WasmType::I32, WasmType::I32], // count, i, entry_ptr
            &pairs_set_instructions,
        )?;
        self.add_function_alias("pairs.set", pairs_set_idx);

        // __pairs_get(map_ptr: i32, key_ptr: i32) -> i32
        // Returns the value pointer for the key, or 0 if not found.
        // Needs 3 extra locals: count, i, entry_ptr.
        let pairs_get_instructions = native_stdlib::pairs_ops::gen_pairs_get(str_eq_idx);
        let pairs_get_idx = self.register_function_with_locals(
            "__pairs_get",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32], // count, i, entry_ptr
            &pairs_get_instructions,
        )?;
        self.add_function_alias("pairs.get", pairs_get_idx);

        // __pairs_has(map_ptr: i32, key_ptr: i32) -> i32
        // Returns 1 if the key exists, 0 otherwise.
        // Needs 3 extra locals: count, i, entry_ptr.
        let pairs_has_instructions = native_stdlib::pairs_ops::gen_pairs_has(str_eq_idx);
        let pairs_has_idx = self.register_function_with_locals(
            "__pairs_has",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32, WasmType::I32], // count, i, entry_ptr
            &pairs_has_instructions,
        )?;
        self.add_function_alias("pairs.has", pairs_has_idx);

        // __pairs_len(map_ptr: i32) -> i32
        // Returns the number of entries currently stored.
        let pairs_len_instructions = native_stdlib::pairs_ops::gen_pairs_len();
        let pairs_len_idx = self.register_function(
            "__pairs_len",
            &[WasmType::I32],
            Some(WasmType::I32),
            &pairs_len_instructions,
        )?;
        self.add_function_alias("pairs.len", pairs_len_idx);
        self.add_function_alias("pairs.size", pairs_len_idx);

        Ok(())
    }

    pub(crate) fn register_conditional_operations(&mut self) -> Result<(), CompilerError> {
        use crate::stdlib::memory::MemoryManager;
        use std::cell::RefCell;
        use std::rc::Rc;

        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(16))));

        use crate::stdlib::method_style::MethodStyleManager;
        let method_style_manager = MethodStyleManager::new(memory_manager.clone());
        method_style_manager.register_functions(self)?;

        use crate::stdlib::list_behavior::ListBehaviorManager;
        let list_behavior_manager = ListBehaviorManager::new(memory_manager.clone());
        list_behavior_manager.register_functions(self)?;

        Ok(())
    }
}
