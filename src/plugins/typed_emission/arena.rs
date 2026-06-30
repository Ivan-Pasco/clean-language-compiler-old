use super::error::{EmitDiagnostic, EmitError};
/// Single-call arena for typed AST emission.
///
/// Created at the start of every `expand_block_typed` invocation, dropped on
/// return. NEVER shared across calls — see typed-emission.md §4.
///
/// Handles are 1-indexed. Index 0 is the sentinel ("null") handle, valid for
/// slots documented as nullable (e.g. the `else_block_handle` in `_stmt_if`,
/// the `return_type_handle` when `void` is implied). The `take_*` methods
/// accept 0 as nullable and return `Ok(None)` for those variants.
use crate::ast::{Expression, Function, Statement, Type};
use crate::plugins::PluginExpansion;

/// All node kinds that can be allocated in the arena.
pub(super) enum EmitNode {
    /// A fully-built statement.
    Stmt(Statement),
    /// A fully-built expression.
    Expr(Expression),
    /// A fully-built type.
    Type(Type),
    /// A pending function that may be emitted at file scope (`_emit_function`)
    /// or consumed into a class as a method (`_emit_class`). The distinction
    /// is deferred until the declaration emitter runs.
    PendingFunction(Function),
}

/// Public re-export of operator mapping utilities used by bridges.
pub use mapping::{map_binary_op, map_unary_op};

mod mapping {
    use crate::ast::{BinaryOperator, UnaryOperator};

    /// Map an operator string to a `BinaryOperator` variant.
    /// Returns `None` for unrecognised strings; the bridge emits PLUGIN007
    /// (BridgeHostClassMismatchInEmission is the closest, but we use a
    /// general unknown-op diagnostic) and returns handle 0.
    pub fn map_binary_op(op: &str) -> Option<BinaryOperator> {
        Some(match op {
            "+" => BinaryOperator::Add,
            "-" => BinaryOperator::Subtract,
            "*" => BinaryOperator::Multiply,
            "/" => BinaryOperator::Divide,
            "%" => BinaryOperator::Modulo,
            "**" => BinaryOperator::Power,
            "==" => BinaryOperator::Equal,
            "!=" => BinaryOperator::NotEqual,
            "<" => BinaryOperator::Less,
            ">" => BinaryOperator::Greater,
            "<=" => BinaryOperator::LessEqual,
            ">=" => BinaryOperator::GreaterEqual,
            "is" => BinaryOperator::Is,
            "not" => BinaryOperator::Not,
            "and" => BinaryOperator::And,
            "or" => BinaryOperator::Or,
            "default" => BinaryOperator::Default,
            _ => return None,
        })
    }

    /// Map an operator string to a `UnaryOperator` variant.
    pub fn map_unary_op(op: &str) -> Option<UnaryOperator> {
        Some(match op {
            "-" => UnaryOperator::Negate,
            "not" | "!" => UnaryOperator::Not,
            "!postfix" => UnaryOperator::RequiredAssert,
            _ => return None,
        })
    }
}

/// Single-call arena.
pub struct EmitArena {
    /// Monotonic ctx_handle assigned to this arena instance. Every bridge
    /// call validates that the caller's ctx matches this value before
    /// proceeding.
    ctx_handle: i32,

    /// 1-indexed node storage. `nodes[0]` is unused (reserved as sentinel).
    nodes: Vec<Option<EmitNode>>,

    /// Consumption tracking. `consumed[i]` is `true` when handle `i` has been
    /// consumed by a parent constructor. `consumed[0]` is always `false`
    /// (the sentinel is never consumed). Uses `Vec<bool>` — bitvec is not a
    /// current Cargo dependency per the design note.
    consumed: Vec<bool>,

    /// Diagnostics accumulated via `_emit_error`.
    diagnostics: Vec<EmitDiagnostic>,

    /// Accumulating expansion populated by declaration emitters.
    pub expansion: PluginExpansion,

    /// Set when any `_emit_error` with severity == 2 is called. The
    /// expansion is treated as failed even if the WASM function returns 0.
    pub saw_error_severity: bool,
}

impl EmitArena {
    /// Construct a fresh arena for a single expand-block call.
    pub fn new(ctx_handle: i32) -> Self {
        // Slot 0 is the sentinel; pre-populate so 1-indexed handles align.
        Self {
            ctx_handle,
            nodes: vec![None],     // index 0 = sentinel
            consumed: vec![false], // index 0 = sentinel (never consumed)
            diagnostics: Vec::new(),
            expansion: PluginExpansion::default(),
            saw_error_severity: false,
        }
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Private allocation
    // ──────────────────────────────────────────────────────────────────────────

    /// Allocate a node and return its 1-indexed handle.
    fn insert(&mut self, node: EmitNode) -> i32 {
        let idx = self.nodes.len() as i32;
        self.nodes.push(Some(node));
        self.consumed.push(false);
        idx
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Public ctx validation
    // ──────────────────────────────────────────────────────────────────────────

    /// Return the ctx_handle for diagnostic messages.
    pub fn ctx_handle(&self) -> i32 {
        self.ctx_handle
    }

    /// Validate that `ctx` matches this arena's ctx_handle.
    pub fn check_ctx(&self, ctx: i32) -> Result<(), EmitError> {
        if ctx == self.ctx_handle {
            Ok(())
        } else {
            Err(EmitError::WrongCtx)
        }
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Internal take helpers — validate, optionally consume, return the node
    // ──────────────────────────────────────────────────────────────────────────

    /// Consume (or peek at) a node by handle, validating ctx and kind.
    ///
    /// `consume = true` marks the handle as consumed; subsequent calls with
    /// the same handle return `Err(HandleConsumed)` — PLUGIN008.
    fn take_node(&mut self, ctx: i32, handle: i32, consume: bool) -> Result<&EmitNode, EmitError> {
        self.check_ctx(ctx)?;

        if handle <= 0 {
            // 0 is the sentinel; negative is always invalid.
            return Err(EmitError::NullHandle);
        }

        let idx = handle as usize;

        if idx >= self.nodes.len() {
            return Err(EmitError::OutOfRange { handle });
        }

        if self.consumed[idx] {
            return Err(EmitError::HandleConsumed { handle });
        }

        if self.nodes[idx].is_none() {
            // Should not happen — nodes are only ever set to None after
            // consumption in destructive take below.
            return Err(EmitError::OutOfRange { handle });
        }

        if consume {
            self.consumed[idx] = true;
        }

        Ok(self.nodes[idx].as_ref().unwrap())
    }

    /// Destructively take a node out of the arena (always consume).
    fn take_node_owned(&mut self, ctx: i32, handle: i32) -> Result<EmitNode, EmitError> {
        self.check_ctx(ctx)?;

        if handle <= 0 {
            return Err(EmitError::NullHandle);
        }

        let idx = handle as usize;

        if idx >= self.nodes.len() {
            return Err(EmitError::OutOfRange { handle });
        }

        if self.consumed[idx] {
            return Err(EmitError::HandleConsumed { handle });
        }

        self.consumed[idx] = true;
        self.nodes[idx]
            .take()
            .ok_or(EmitError::OutOfRange { handle })
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Typed extraction — public API for bridges
    // ──────────────────────────────────────────────────────────────────────────

    /// Take a `Statement` from the arena. The handle must refer to a Stmt node.
    pub fn take_stmt(&mut self, ctx: i32, handle: i32) -> Result<Statement, EmitError> {
        let node = self.take_node_owned(ctx, handle)?;
        match node {
            EmitNode::Stmt(s) => Ok(s),
            other => {
                // Put the node back — the handle is not consumed since the
                // kind check failed before the node was used.
                let idx = handle as usize;
                self.consumed[idx] = false;
                self.nodes[idx] = Some(other);
                Err(EmitError::WrongNodeKind {
                    handle,
                    expected: "Stmt",
                })
            }
        }
    }

    /// Take a `Statement` from the arena, or return `None` when handle == 0
    /// (for nullable slots like `else_block_handle`).
    pub fn take_stmt_opt(&mut self, ctx: i32, handle: i32) -> Result<Option<Statement>, EmitError> {
        if handle == 0 {
            self.check_ctx(ctx)?;
            return Ok(None);
        }
        self.take_stmt(ctx, handle).map(Some)
    }

    /// Take an `Expression` from the arena.
    pub fn take_expr(&mut self, ctx: i32, handle: i32) -> Result<Expression, EmitError> {
        let node = self.take_node_owned(ctx, handle)?;
        match node {
            EmitNode::Expr(e) => Ok(e),
            other => {
                let idx = handle as usize;
                self.consumed[idx] = false;
                self.nodes[idx] = Some(other);
                Err(EmitError::WrongNodeKind {
                    handle,
                    expected: "Expr",
                })
            }
        }
    }

    /// Take an `Expression` or return `None` when handle == 0.
    pub fn take_expr_opt(
        &mut self,
        ctx: i32,
        handle: i32,
    ) -> Result<Option<Expression>, EmitError> {
        if handle == 0 {
            self.check_ctx(ctx)?;
            return Ok(None);
        }
        self.take_expr(ctx, handle).map(Some)
    }

    /// Take a `Type` from the arena.
    pub fn take_type(&mut self, ctx: i32, handle: i32) -> Result<Type, EmitError> {
        let node = self.take_node_owned(ctx, handle)?;
        match node {
            EmitNode::Type(t) => Ok(t),
            other => {
                let idx = handle as usize;
                self.consumed[idx] = false;
                self.nodes[idx] = Some(other);
                Err(EmitError::WrongNodeKind {
                    handle,
                    expected: "Type",
                })
            }
        }
    }

    /// Take a `Type` or return `None` (i.e. `Type::Void`) when handle == 0.
    pub fn take_type_opt(&mut self, ctx: i32, handle: i32) -> Result<Option<Type>, EmitError> {
        if handle == 0 {
            self.check_ctx(ctx)?;
            return Ok(None);
        }
        self.take_type(ctx, handle).map(Some)
    }

    /// Take a `PendingFunction` from the arena.
    pub fn take_pending_function(&mut self, ctx: i32, handle: i32) -> Result<Function, EmitError> {
        let node = self.take_node_owned(ctx, handle)?;
        match node {
            EmitNode::PendingFunction(f) => Ok(f),
            other => {
                let idx = handle as usize;
                self.consumed[idx] = false;
                self.nodes[idx] = Some(other);
                Err(EmitError::WrongNodeKind {
                    handle,
                    expected: "PendingFunction",
                })
            }
        }
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Allocation — public API for bridges
    // ──────────────────────────────────────────────────────────────────────────

    pub fn alloc_stmt(&mut self, s: Statement) -> i32 {
        self.insert(EmitNode::Stmt(s))
    }

    pub fn alloc_expr(&mut self, e: Expression) -> i32 {
        self.insert(EmitNode::Expr(e))
    }

    pub fn alloc_type(&mut self, t: Type) -> i32 {
        self.insert(EmitNode::Type(t))
    }

    pub fn alloc_pending_function(&mut self, f: Function) -> i32 {
        self.insert(EmitNode::PendingFunction(f))
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Diagnostic emission
    // ──────────────────────────────────────────────────────────────────────────

    pub fn emit_diagnostic(&mut self, d: EmitDiagnostic) {
        if d.severity >= 2 {
            self.saw_error_severity = true;
        }
        self.diagnostics.push(d);
    }

    /// Push a PLUGIN008 (ConsumedHandleReuse) diagnostic.
    pub fn emit_plugin008(&mut self, handle: i32) {
        self.emit_diagnostic(EmitDiagnostic {
            severity: 2,
            code: "PLUGIN008".to_string(),
            message: format!(
                "handle {} has already been consumed by a previous constructor call \
                 (ConsumedHandleReuse — typed-emission.md §4)",
                handle
            ),
            span_json: String::new(),
        });
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Completion
    // ──────────────────────────────────────────────────────────────────────────

    /// Consume the arena and return its outputs.
    pub fn finish(self) -> (PluginExpansion, Vec<EmitDiagnostic>, bool) {
        (self.expansion, self.diagnostics, self.saw_error_severity)
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Helpers used by declaration emitters
    // ──────────────────────────────────────────────────────────────────────────

    /// Push a statement into `expansion.start_function.body`, creating the
    /// start function if it does not exist yet.
    pub fn push_start_stmt(&mut self, s: Statement) {
        let sf = self.expansion.start_function.get_or_insert_with(|| {
            Function::new(
                "start".to_string(),
                Vec::new(),
                Type::Void,
                Vec::new(),
                None,
            )
        });
        sf.body.push(s);
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Unit tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expression, Statement, Type, Value};

    fn dummy_stmt() -> Statement {
        Statement::Return {
            value: None,
            location: None,
        }
    }

    fn dummy_expr() -> Expression {
        Expression::Literal(Value::Boolean(true))
    }

    fn dummy_type() -> Type {
        Type::Integer
    }

    // ── Test 1: wrong ctx_handle is rejected ─────────────────────────────────
    #[test]
    fn arena_rejects_wrong_ctx() {
        let mut arena = EmitArena::new(42);
        let handle = arena.alloc_stmt(dummy_stmt());
        let result = arena.take_stmt(99 /* wrong ctx */, handle);
        assert_eq!(result, Err(EmitError::WrongCtx));
    }

    // ── Test 2: out-of-range handle is rejected ───────────────────────────────
    #[test]
    fn arena_rejects_out_of_range_handle() {
        let mut arena = EmitArena::new(1);
        // No nodes allocated — handle 5 is out of range.
        let result = arena.take_stmt(1, 5);
        assert_eq!(result, Err(EmitError::OutOfRange { handle: 5 }));
    }

    // ── Test 3: successful take marks as consumed ─────────────────────────────
    #[test]
    fn arena_take_marks_consumed() {
        let mut arena = EmitArena::new(7);
        let handle = arena.alloc_stmt(dummy_stmt());
        // First take succeeds.
        let stmt = arena.take_stmt(7, handle);
        assert!(stmt.is_ok());
        // consumed[handle] is now true; second take returns PLUGIN008.
        let result = arena.take_stmt(7, handle);
        assert_eq!(result, Err(EmitError::HandleConsumed { handle }));
    }

    // ── Test 4: double-take returns HandleConsumed (PLUGIN008) ────────────────
    #[test]
    fn arena_double_take_returns_plugin008() {
        let mut arena = EmitArena::new(3);
        let h = arena.alloc_expr(dummy_expr());
        let _ = arena.take_expr(3, h).expect("first take should succeed");
        let err = arena.take_expr(3, h).unwrap_err();
        assert_eq!(err, EmitError::HandleConsumed { handle: h });
        assert_eq!(err.error_code(), "PLUGIN008");
    }

    // ── Test 5: sentinel handle 0 is valid for nullable slots ─────────────────
    #[test]
    fn arena_sentinel_zero_valid_for_nullable() {
        let mut arena = EmitArena::new(5);
        // take_stmt_opt with handle=0 should return Ok(None).
        let result = arena.take_stmt_opt(5, 0);
        assert_eq!(result, Ok(None));
        // take_expr_opt with handle=0 should return Ok(None).
        let result = arena.take_expr_opt(5, 0);
        assert_eq!(result, Ok(None));
        // take_type_opt with handle=0 should return Ok(None).
        let result = arena.take_type_opt(5, 0);
        assert_eq!(result, Ok(None));
    }

    // ── Test 6: saw_error_severity is set by severity=2 diagnostic ───────────
    #[test]
    fn arena_error_severity_flag() {
        let mut arena = EmitArena::new(1);
        assert!(!arena.saw_error_severity);
        arena.emit_diagnostic(EmitDiagnostic {
            severity: 1,
            code: "W001".to_string(),
            message: "warning".to_string(),
            span_json: String::new(),
        });
        assert!(
            !arena.saw_error_severity,
            "severity=1 should not set the flag"
        );
        arena.emit_diagnostic(EmitDiagnostic {
            severity: 2,
            code: "E001".to_string(),
            message: "error".to_string(),
            span_json: String::new(),
        });
        assert!(arena.saw_error_severity, "severity=2 must set the flag");
    }

    // ── Test 7: WrongNodeKind when expr handle used as stmt ───────────────────
    #[test]
    fn arena_wrong_node_kind() {
        let mut arena = EmitArena::new(2);
        let h = arena.alloc_expr(dummy_expr());
        let err = arena.take_stmt(2, h).unwrap_err();
        assert_eq!(
            err,
            EmitError::WrongNodeKind {
                handle: h,
                expected: "Stmt"
            }
        );
        // The handle should NOT have been consumed — the kind check failed.
        let ok = arena.take_expr(2, h);
        assert!(
            ok.is_ok(),
            "handle should still be takeable as the correct type"
        );
    }

    // ── Test 8: finish() returns the accumulated expansion ───────────────────
    #[test]
    fn arena_finish_returns_expansion() {
        let arena = EmitArena::new(1);
        let (expansion, diags, saw_err) = arena.finish();
        assert!(expansion.functions.is_empty());
        assert!(diags.is_empty());
        assert!(!saw_err);
    }
}
