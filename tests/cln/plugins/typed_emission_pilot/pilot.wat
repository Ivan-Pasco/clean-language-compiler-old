;; Typed-emission pilot plugin (WAT source).
;;
;; Implements `expand_block_typed(ctx, block_name_lp, attrs_lp, body_lp) → i32`.
;; When called, emits a single function named "two" that returns integer 2.
;;
;; Bridge call sequence (handle ordering is deterministic — 1-indexed):
;;   handle 1: _type_integer(ctx)                  → int type
;;   handle 2: _expr_int_lit(ctx, lo=2, hi=0)      → integer literal 2
;;   handle 3: _stmt_return(ctx, expr=2)            → return 2 (consumes handle 2)
;;   handle 4: _stmt_block(ctx, "[3]")              → block([3]) (consumes handle 3)
;;   _emit_function(ctx, "two", "[]", ret=1, body=4, flags=0) → success (consumes 1, 4)
;;
;; LP-string layout in static data (4-byte LE length prefix + UTF-8 bytes):
;;   offset 0:  len=3, "two"      (function name)
;;   offset 16: len=2, "[]"       (empty params JSON)
;;   offset 32: len=3, "[3]"      (stmts JSON — handle 3 is return_stmt)
;;
;; The stmts JSON "[3]" relies on handle 3 being return_stmt. This is guaranteed
;; because the arena allocates handles monotonically starting from 1 and each
;; bridge call allocates exactly one handle in the order they are called.
(module
  (import "env" "_emit_function" (func $_emit_function (param i32 i32 i32 i32 i32 i32) (result i32)))
  (import "env" "_stmt_block"    (func $_stmt_block    (param i32 i32) (result i32)))
  (import "env" "_stmt_return"   (func $_stmt_return   (param i32 i32) (result i32)))
  (import "env" "_expr_int_lit"  (func $_expr_int_lit  (param i32 i32 i32) (result i32)))
  (import "env" "_type_integer"  (func $_type_integer  (param i32) (result i32)))

  (memory (export "memory") 1)

  ;; Static LP-strings packed into the data section.
  ;;
  ;; offset 0..7:   LP("two")  = [03 00 00 00] + "two"
  ;; offset 16..21: LP("[]")   = [02 00 00 00] + "[]"
  ;; offset 32..38: LP("[3]")  = [03 00 00 00] + "[3]"
  (data (i32.const 0)  "\03\00\00\00two")
  (data (i32.const 16) "\02\00\00\00[]")
  (data (i32.const 32) "\03\00\00\00[3]")

  (func (export "expand_block_typed")
        (param $ctx i32)
        (param $block_name_lp i32)
        (param $attrs_lp i32)
        (param $body_lp i32)
        (result i32)

    (local $int_type_h i32)
    (local $two_expr_h i32)
    (local $return_stmt_h i32)
    (local $body_block_h i32)
    (local $fn_result i32)

    ;; handle 1: _type_integer(ctx)
    local.get $ctx
    call $_type_integer
    local.set $int_type_h

    ;; handle 2: _expr_int_lit(ctx, lo=2, hi=0)
    local.get $ctx
    i32.const 2    ;; lo
    i32.const 0    ;; hi
    call $_expr_int_lit
    local.set $two_expr_h

    ;; handle 3: _stmt_return(ctx, expr=handle_2); consumes handle 2
    local.get $ctx
    local.get $two_expr_h
    call $_stmt_return
    local.set $return_stmt_h

    ;; handle 4: _stmt_block(ctx, "[3]"); "[3]" is at offset 32; consumes handle 3
    local.get $ctx
    i32.const 32   ;; LP-string pointer for "[3]"
    call $_stmt_block
    local.set $body_block_h

    ;; _emit_function(ctx, name_lp=0, params_lp=16, ret_type_h=1, body_h=4, flags=0)
    ;; consumes int_type_h (1) and body_block_h (4)
    local.get $ctx
    i32.const 0    ;; LP pointer to "two"
    i32.const 16   ;; LP pointer to "[]"
    local.get $int_type_h
    local.get $body_block_h
    i32.const 0    ;; flags
    call $_emit_function
    local.set $fn_result

    ;; Return 0 on success (emit_function returns 0), non-zero on error.
    local.get $fn_result
  )
)
