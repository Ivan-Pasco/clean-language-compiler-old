use std::collections::HashMap;

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
///
/// Batch builder handles (Amendment 4 §3.14) are stored in a SEPARATE arena
/// (`batch_nodes`) with their own monotonic counter. They are returned to plugins
/// with bit 24 set (the "batch tag bit" — `BATCH_TAG = 0x0100_0000`) so the
/// widened `_emit_helpers_batch` / `_emit_class_full` bridges can distinguish
/// batch spec handles from LP-string pointers at the call site.
///
/// Disambiguation: if `(value >> 24) & 1 == 1`, the value is a batch handle
/// with index `value & 0x00FF_FFFF`. Otherwise it is an LP-string pointer.
/// LP-string pointers live in WASM linear memory and may have any low address;
/// the tag-bit approach is portable across all memory layouts, unlike the
/// numeric-range partition. See typed-emission.md §3.14 "Handle vs LP disambiguation".
use crate::ast::{Expression, Function, Statement, Type};
use crate::plugins::PluginExpansion;

/// The tag bit that distinguishes batch builder handles from LP-string pointers.
/// Batch handles returned from builder bridges always have this bit set.
pub const BATCH_TAG: i32 = 0x0100_0000;

/// Record of the first bridge that tripped the sticky-error flag.
/// The bridge name and a short reason are captured for the auto-emitted
/// PLUGIN014 diagnostic (typed-emission.md §3.15, Amendment 5).
#[derive(Debug, Clone)]
pub struct StickyError {
    #[allow(dead_code)]
    pub bridge: &'static str,
    #[allow(dead_code)]
    pub reason: String,
}

/// All node kinds that can be allocated in the arena.
pub(super) enum EmitNode {
    /// A fully-built statement.
    Stmt(Statement),
    /// A fully-built expression.
    Expr(Expression),
    /// A fully-built type.
    Type(Type),
    /// A pending function that may be consumed by `_emit_class` (as a method)
    /// or `_emit_route` (as a handler). Allocated by `_define_function`;
    /// un-consumed PendingFunctions are silently dropped at arena finish.
    PendingFunction(Function),
}

// ─────────────────────────────────────────────────────────────────────────────
// Batch builder node types (Amendment 4 §3.14)
// ─────────────────────────────────────────────────────────────────────────────

use crate::plugins::typed_emission::batch_schema::{
    BatchExpr, BatchField, BatchFunction, BatchMethod, BatchObjectField, BatchParam, BatchSpec,
    BatchStatement,
};

/// Class spec built via builder API (mirrors ClassSpec but owned for building).
#[derive(Debug)]
pub struct BatchBuilderClass {
    pub name: String,
    pub parent: Option<String>,
    pub fields: Vec<BatchField>,
    pub methods: Vec<BatchMethod>,
}

/// All node kinds storable in the batch builder arena.
#[derive(Debug)]
pub enum BatchNode {
    /// A BatchExpr node — pushed via expression builders.
    Expr(BatchExpr),
    /// A BatchStatement node — pushed via statement builders.
    Stmt(BatchStatement),
    /// A complete BatchFunction — pushed via batch.func.
    Function(BatchFunction),
    /// A BatchField — pushed via batch.classField.
    Field(BatchField),
    /// A BatchMethod — pushed via batch.method.
    Method(BatchMethod),
    /// A BatchParam — pushed via batch.param.
    Param(BatchParam),
    /// A BatchObjectField — pushed via batch.objectField.
    ObjectField(BatchObjectField),
    /// A homogeneous array of batch item handles (raw indices into batch_nodes).
    /// The discriminant of the array is set on first push; subsequent pushes
    /// must match or PLUGIN013 is emitted.
    Array(BatchArray),
    /// Terminal batch spec for _emit_helpers_batch.
    Spec(BatchSpec),
    /// Terminal class spec for _emit_class_full (built by batch.class).
    ClassSpec(BatchBuilderClass),
}

/// Discriminant for what kind of items a BatchArray holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchArrayKind {
    Expr,
    Stmt,
    Function,
    Method,
    Field,
    Param,
    ObjectField,
    /// Unset — array is empty, kind determined on first push.
    Unset,
}

/// In-progress mutable array of batch node indices.
#[derive(Debug)]
pub struct BatchArray {
    pub kind: BatchArrayKind,
    /// Raw batch node indices (not tagged — internal only).
    pub items: Vec<i32>,
}

/// Error returned by `EmitArena::batch_array_from_handles` (Amendments 7 & 8).
///
/// Callers (`batch.stmtSeq` bridge, `batch.args` bridge) translate each variant
/// into PLUGIN008 (Consumed) or PLUGIN013 (all others) diagnostics.
///
/// The `StmtSeqError` alias preserves the Amendment 7 naming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrayFromHandlesError {
    InvalidHandle { handle: i32 },
    OutOfRange { handle: i32 },
    Consumed { handle: i32 },
    KindMismatch { handle: i32, actual: BatchArrayKind },
    NonPushable { handle: i32 },
}

/// Amendment 7 alias — retained for compatibility with the Amendment 7 bridge
/// and test suite. New callers should use `ArrayFromHandlesError` directly.
pub type StmtSeqError = ArrayFromHandlesError;

/// Error returned by `EmitArena::batch_array_push`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchArrayPushError {
    /// The array handle is not a valid batch Array node.
    InvalidArrayHandle,
    /// The array handle was already consumed.
    ArrayConsumed { handle: i32 },
    /// The item handle is not a valid batch node.
    InvalidItemHandle,
    /// The item handle was already consumed.
    ItemConsumed { handle: i32 },
    /// The item's kind does not match the array's established kind.
    KindMismatch {
        array_kind: BatchArrayKind,
        item_kind: BatchArrayKind,
    },
    /// The node kind cannot be pushed into any array (e.g. Array, Spec, ClassSpec).
    NonPushableKind { handle: i32 },
}

impl BatchArrayPushError {
    #[allow(dead_code)]
    pub fn plugin_code(&self) -> &'static str {
        match self {
            BatchArrayPushError::ArrayConsumed { .. } => "PLUGIN008",
            BatchArrayPushError::ItemConsumed { .. } => "PLUGIN008",
            _ => "PLUGIN013",
        }
    }

    pub fn message(&self) -> String {
        match self {
            BatchArrayPushError::InvalidArrayHandle => {
                "batch.arrayPush: array_handle is not a valid batch array".to_string()
            }
            BatchArrayPushError::ArrayConsumed { handle } => {
                format!("batch.arrayPush: array handle {} has already been consumed (PLUGIN008 ConsumedHandleReuse)", handle)
            }
            BatchArrayPushError::InvalidItemHandle => {
                "batch.arrayPush: item_handle is not a valid batch handle".to_string()
            }
            BatchArrayPushError::ItemConsumed { handle } => {
                format!("batch.arrayPush: item handle {} has already been consumed (PLUGIN008 ConsumedHandleReuse)", handle)
            }
            BatchArrayPushError::KindMismatch {
                array_kind,
                item_kind,
            } => {
                format!(
                    "batch.arrayPush: kind mismatch — array accepts {:?} items but got {:?}",
                    array_kind, item_kind
                )
            }
            BatchArrayPushError::NonPushableKind { handle } => {
                format!(
                    "batch.arrayPush: handle {} refers to a non-pushable batch node kind (Array/Spec/ClassSpec cannot be array items)",
                    handle
                )
            }
        }
    }
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
            // "^" is the grammar.ebnf §3.2 power_op. The legacy "**" form is
            // also accepted here for backward compatibility with any plugin that
            // used it before the TOML table was co-published, but "^" is canonical.
            "^" | "**" => BinaryOperator::Power,
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

    // ── Sticky-error context (Amendment 5 §3.15, Landing C4) ───────────────
    /// Records the first bridge that failed since the last `_ctx_clear_error`.
    /// Once set, every subsequent bridge call short-circuits (returns 0 or 1)
    /// without doing work — this is the "sticky" part.
    ///
    /// Plugins query this via `_ctx_had_error(ctx) -> integer` (§3.15.2) and
    /// clear it via `_ctx_clear_error(ctx)` (§3.15.4). Plugins MAY set a
    /// custom error code + message via `_ctx_set_error_context(ctx, code, message)`
    /// before making a group of bridge calls, so the auto-emitted diagnostic
    /// on first failure carries a plugin-authored code (§3.15.3).
    sticky_error: Option<StickyError>,

    /// Optional plugin-supplied error context. When present, the first bridge
    /// failure that trips sticky_error auto-emits a diagnostic with this
    /// (code, message) instead of the internal PLUGIN014 default.
    sticky_context: Option<(String, String)>,

    // ── Batch builder arena (Amendment 4 §3.14) ────────────────────────────
    /// 1-indexed storage for batch builder nodes. Index 0 unused (sentinel).
    /// Separate from `nodes` so AST node kinds and batch node kinds never mix.
    batch_nodes: Vec<Option<BatchNode>>,

    /// Consumption tracking for batch handles. Parallel to `batch_nodes`.
    batch_consumed: Vec<bool>,

    /// Source-string origin for statements allocated via `_emit_stmt_from_source`.
    /// Maps stmt handle → the raw Clean source text that was parsed into the
    /// Statement. Used by the assemble_typed → InjectedSource conversion path
    /// to reconstruct compilable source when a statement's body must be
    /// serialized back out. Handles not created from source (batch builders,
    /// _emit_class_full method bodies, etc.) are absent from this map — callers
    /// that need source for such handles must fall back to the loud error.
    /// See COMPILER-EMIT-ARENA-CONVERSION-MISSING (fp 3b15cd54).
    pub stmt_source_origins: HashMap<i32, String>,

    /// Source-string origin for expressions allocated via `_emit_expr_from_source`.
    /// Mirror of `stmt_source_origins` for the expression bridge (Amendment 6
    /// §3.16). Currently unused by the reconstruction path (frame.ui builds
    /// standalone expressions inside function bodies whose source is already
    /// captured on the enclosing stmt handle), but recorded for symmetry with
    /// stmt tracking and future consumers.
    pub expr_source_origins: HashMap<i32, String>,
}

impl EmitArena {
    /// Construct a fresh arena for a single expand-block call.
    pub fn new(ctx_handle: i32) -> Self {
        // Slot 0 is the sentinel in both arenas; pre-populate so 1-indexed handles align.
        Self {
            ctx_handle,
            nodes: vec![None],     // index 0 = sentinel
            consumed: vec![false], // index 0 = sentinel (never consumed)
            diagnostics: Vec::new(),
            expansion: PluginExpansion::default(),
            saw_error_severity: false,
            sticky_error: None,
            sticky_context: None,
            batch_nodes: vec![None], // index 0 = sentinel
            batch_consumed: vec![false],
            stmt_source_origins: HashMap::new(),
            expr_source_origins: HashMap::new(),
        }
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Sticky-error context (Amendment 5 / Landing C4)
    // ──────────────────────────────────────────────────────────────────────────

    /// Return true if the sticky-error flag is set. Bridges do not call this
    /// directly — the sticky gate is folded into `check_ctx`. Exposed for
    /// `_ctx_had_error` (§3.15) and for unit tests.
    pub fn is_sticky(&self) -> bool {
        self.sticky_error.is_some()
    }

    /// Set the sticky flag if not already set, and auto-emit the PLUGIN014
    /// diagnostic on first trip. Subsequent calls are no-ops. Always returns 0
    /// so bridges can `return arena.trip(...)` in one line.
    ///
    /// If the plugin has set an error context via `_ctx_set_error_context`,
    /// that (code, message) is used for the auto-emitted diagnostic; otherwise
    /// the default PLUGIN014 message names the failing bridge.
    pub fn trip(&mut self, bridge: &'static str, reason: impl Into<String>) -> i32 {
        if self.sticky_error.is_some() {
            return 0;
        }
        let reason = reason.into();
        self.sticky_error = Some(StickyError {
            bridge,
            reason: reason.clone(),
        });
        let (code, message) = match self.sticky_context.clone() {
            Some((c, m)) => (c, format!("{} (first failure: {} — {})", m, bridge, reason)),
            None => (
                "PLUGIN014".to_string(),
                format!(
                    "sticky-error tripped by {}: {} (typed-emission.md §3.15)",
                    bridge, reason
                ),
            ),
        };
        self.emit_diagnostic(EmitDiagnostic {
            severity: 2,
            code,
            message,
            span_json: String::new(),
        });
        0
    }

    /// Public plugin-side query: 1 if sticky is set, else 0.
    pub fn had_error(&self) -> i32 {
        if self.sticky_error.is_some() {
            1
        } else {
            0
        }
    }

    /// Public plugin-side setter for the error context used on first sticky trip.
    /// Empty code + empty message clears the context (equivalent to never having
    /// called it). Does NOT clear an already-set sticky flag.
    pub fn set_error_context(&mut self, code: String, message: String) {
        if code.is_empty() && message.is_empty() {
            self.sticky_context = None;
        } else {
            self.sticky_context = Some((code, message));
        }
    }

    /// Public plugin-side reset. Clears the sticky flag AND the context.
    /// Intended for helper functions that want to recover from a nested
    /// failure and continue emission — rarely needed in practice.
    pub fn clear_error(&mut self) {
        self.sticky_error = None;
        self.sticky_context = None;
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
    /// Amendment 5 / Landing C4: also refuses when the sticky-error flag is
    /// set. Because every existing bridge already calls `check_ctx` and
    /// short-circuits to `return 0` on error, this single-point change
    /// gates all ~57 typed-emission bridges on the sticky flag with no
    /// per-bridge edits.
    pub fn check_ctx(&self, ctx: i32) -> Result<(), EmitError> {
        if ctx != self.ctx_handle {
            Err(EmitError::WrongCtx)
        } else if self.sticky_error.is_some() {
            Err(EmitError::StickyErrorActive)
        } else {
            Ok(())
        }
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Internal take helpers — validate, optionally consume, return the node
    // ──────────────────────────────────────────────────────────────────────────

    /// Consume (or peek at) a node by handle, validating ctx and kind.
    ///
    /// `consume = true` marks the handle as consumed; subsequent calls with
    /// the same handle return `Err(HandleConsumed)` — PLUGIN008.
    /// Reserved for sub-cycle 3 bridge extensions; not yet called in sub-cycle 2.
    #[allow(dead_code)]
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
    /// Used in tests and reserved for nullable type slots in sub-cycle 3 bridges.
    #[allow(dead_code)]
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

    /// Allocate a pre-built `Function` node for use with `_define_function` (sub-cycle 3).
    pub fn alloc_pending_function(&mut self, f: Function) -> i32 {
        self.insert(EmitNode::PendingFunction(f))
    }

    // ──────────────────────────────────────────────────────────────────────────
    // Batch builder arena — public API (Amendment 4 §3.14)
    // ──────────────────────────────────────────────────────────────────────────

    /// Allocate a `BatchNode` and return a tagged handle.
    ///
    /// The returned handle has `BATCH_TAG` (bit 24) set, so `_emit_helpers_batch`
    /// and `_emit_class_full` can distinguish it from an LP-string pointer without
    /// inspecting WASM memory. The internal index is `handle & 0x00FF_FFFF`.
    pub fn alloc_batch(&mut self, node: BatchNode) -> i32 {
        let idx = self.batch_nodes.len() as i32;
        self.batch_nodes.push(Some(node));
        self.batch_consumed.push(false);
        idx | BATCH_TAG
    }

    /// Check whether `handle` is a batch handle (has `BATCH_TAG` set).
    pub fn is_batch_handle(handle: i32) -> bool {
        (handle & BATCH_TAG) != 0
    }

    /// Extract the internal batch index from a tagged batch handle.
    fn batch_index(handle: i32) -> usize {
        (handle & !BATCH_TAG) as usize
    }

    /// Peek at the batch node behind `handle` without consuming it. Returns
    /// `None` for null / non-batch / out-of-range / already-consumed handles.
    /// Used by `_emit_class_full` to disambiguate a `batch.stringLit`-wrapped
    /// class-spec JSON from a `batch.class`-built handle before dispatching.
    pub fn peek_batch_node(&self, handle: i32) -> Option<&BatchNode> {
        if handle <= 0 || !Self::is_batch_handle(handle) {
            return None;
        }
        let idx = Self::batch_index(handle);
        if idx == 0 || idx >= self.batch_nodes.len() || self.batch_consumed[idx] {
            return None;
        }
        self.batch_nodes[idx].as_ref()
    }

    /// Take a `BatchNode` out of the batch arena (consuming it).
    ///
    /// `handle` must be a tagged batch handle (BATCH_TAG set). Returns
    /// `Err(HandleConsumed)` on re-use (PLUGIN008) and `Err(OutOfRange)` for
    /// invalid indices.
    pub fn take_batch_node(&mut self, handle: i32) -> Result<BatchNode, EmitError> {
        if handle <= 0 {
            return Err(EmitError::NullHandle);
        }
        if !Self::is_batch_handle(handle) {
            // Not a batch handle — caller logic error.
            return Err(EmitError::OutOfRange { handle });
        }
        let idx = Self::batch_index(handle);
        if idx == 0 || idx >= self.batch_nodes.len() {
            return Err(EmitError::OutOfRange { handle });
        }
        if self.batch_consumed[idx] {
            return Err(EmitError::HandleConsumed { handle });
        }
        self.batch_consumed[idx] = true;
        self.batch_nodes[idx]
            .take()
            .ok_or(EmitError::OutOfRange { handle })
    }

    /// Core logic for Amendments 7 (`batch.stmtSeq`) and 8 (`batch.args`):
    /// validate a list of batch handles all of `target_kind`, consume each,
    /// and return a fresh `target_kind`-kind `BatchArray` allocated into the
    /// arena. Returns the tagged batch array handle on success, or
    /// `Err(ArrayFromHandlesError)` describing why the input was invalid.
    /// The bridge wrapper is responsible for translating errors into
    /// PLUGIN008/PLUGIN013 diagnostics and calling `trip` for sticky-error
    /// integration.
    ///
    /// This is the sole mutating path; no partial mutation occurs on error
    /// (all handles are validated before any is consumed).
    pub fn batch_array_from_handles(
        &mut self,
        target_kind: BatchArrayKind,
        handles: &[i32],
    ) -> Result<i32, ArrayFromHandlesError> {
        for &h in handles {
            if h == 0 || !Self::is_batch_handle(h) {
                return Err(ArrayFromHandlesError::InvalidHandle { handle: h });
            }
            let idx = (h & !BATCH_TAG) as usize;
            if idx == 0 || idx >= self.batch_nodes.len() {
                return Err(ArrayFromHandlesError::OutOfRange { handle: h });
            }
            if self.batch_consumed[idx] {
                return Err(ArrayFromHandlesError::Consumed { handle: h });
            }
            match self.peek_batch_node_kind(idx) {
                Some(actual) if actual == target_kind => {}
                Some(actual) => {
                    return Err(ArrayFromHandlesError::KindMismatch { handle: h, actual })
                }
                None => return Err(ArrayFromHandlesError::NonPushable { handle: h }),
            }
        }

        let mut items: Vec<i32> = Vec::with_capacity(handles.len());
        for &h in handles {
            let idx = (h & !BATCH_TAG) as usize;
            self.batch_consumed[idx] = true;
            items.push(idx as i32);
        }
        Ok(self.alloc_batch(BatchNode::Array(BatchArray {
            kind: target_kind,
            items,
        })))
    }

    /// Amendment 7 shortcut — thin wrapper delegating to
    /// `batch_array_from_handles(Stmt, handles)`. Retained for the Amendment 7
    /// bridge and its test suite.
    pub fn batch_stmt_seq_from_handles(&mut self, handles: &[i32]) -> Result<i32, StmtSeqError> {
        self.batch_array_from_handles(BatchArrayKind::Stmt, handles)
    }

    /// Peek at the kind of the batch node at raw index `idx` without consuming
    /// it, mapped to the corresponding `BatchArrayKind` variant. Returns
    /// `None` for non-pushable kinds (Array, Spec, ClassSpec) or empty slots.
    /// Callers MUST validate `idx` is in-range and not consumed before calling.
    pub fn peek_batch_node_kind(&self, idx: usize) -> Option<BatchArrayKind> {
        match &self.batch_nodes[idx] {
            Some(BatchNode::Expr(_)) => Some(BatchArrayKind::Expr),
            Some(BatchNode::Stmt(_)) => Some(BatchArrayKind::Stmt),
            Some(BatchNode::Function(_)) => Some(BatchArrayKind::Function),
            Some(BatchNode::Method(_)) => Some(BatchArrayKind::Method),
            Some(BatchNode::Field(_)) => Some(BatchArrayKind::Field),
            Some(BatchNode::Param(_)) => Some(BatchArrayKind::Param),
            Some(BatchNode::ObjectField(_)) => Some(BatchArrayKind::ObjectField),
            _ => None,
        }
    }

    /// Peek at the `BatchArrayKind` of an array handle without consuming it.
    /// Used by `batch.arrayPush` to enforce homogeneity and by unit tests.
    #[allow(dead_code)]
    pub fn peek_batch_array_kind(&self, handle: i32) -> Option<BatchArrayKind> {
        if !Self::is_batch_handle(handle) {
            return None;
        }
        let idx = Self::batch_index(handle);
        if idx == 0 || idx >= self.batch_nodes.len() {
            return None;
        }
        if self.batch_consumed[idx] {
            return None;
        }
        match &self.batch_nodes[idx] {
            Some(BatchNode::Array(arr)) => Some(arr.kind),
            _ => None,
        }
    }

    /// Push an item handle into a batch array (mutation-in-place per Amendment 4).
    ///
    /// `array_handle` must be an unconsumed Array batch handle.
    /// `item_handle` must be a tagged batch handle of a kind compatible with
    /// the array's existing kind (or Unset — first push sets the kind).
    ///
    /// The item handle IS NOT consumed by this operation — the array handle is
    /// the single consumer per Amendment 4: "arrayPush consumes the item but
    /// NOT the array (mutation-in-place)".
    ///
    /// Wait — re-reading Amendment 4: "The array is opaque and single-consumption
    /// at the point it is folded into a parent node... reusing it after consumption
    /// is PLUGIN008." The ITEM is consumed when pushed (owned by the array).
    /// The array itself is consumed when folded into the parent.
    ///
    /// Returns `Ok(kind)` on success or `Err` on mismatch / invalid handles.
    pub fn batch_array_push(
        &mut self,
        array_handle: i32,
        item_handle: i32,
    ) -> Result<(), BatchArrayPushError> {
        // Validate array handle.
        if !Self::is_batch_handle(array_handle) {
            return Err(BatchArrayPushError::InvalidArrayHandle);
        }
        let arr_idx = Self::batch_index(array_handle);
        if arr_idx == 0 || arr_idx >= self.batch_nodes.len() {
            return Err(BatchArrayPushError::InvalidArrayHandle);
        }
        if self.batch_consumed[arr_idx] {
            return Err(BatchArrayPushError::ArrayConsumed {
                handle: array_handle,
            });
        }

        // Validate item handle and determine its kind.
        if !Self::is_batch_handle(item_handle) {
            return Err(BatchArrayPushError::InvalidItemHandle);
        }
        let item_idx = Self::batch_index(item_handle);
        if item_idx == 0 || item_idx >= self.batch_nodes.len() {
            return Err(BatchArrayPushError::InvalidItemHandle);
        }
        if self.batch_consumed[item_idx] {
            return Err(BatchArrayPushError::ItemConsumed {
                handle: item_handle,
            });
        }

        let item_kind = match &self.batch_nodes[item_idx] {
            Some(BatchNode::Expr(_)) => BatchArrayKind::Expr,
            Some(BatchNode::Stmt(_)) => BatchArrayKind::Stmt,
            Some(BatchNode::Function(_)) => BatchArrayKind::Function,
            Some(BatchNode::Method(_)) => BatchArrayKind::Method,
            Some(BatchNode::Field(_)) => BatchArrayKind::Field,
            Some(BatchNode::Param(_)) => BatchArrayKind::Param,
            Some(BatchNode::ObjectField(_)) => BatchArrayKind::ObjectField,
            _ => {
                return Err(BatchArrayPushError::NonPushableKind {
                    handle: item_handle,
                })
            }
        };

        // Extract the array, update kind+items, put it back.
        let arr_node = self.batch_nodes[arr_idx]
            .as_mut()
            .ok_or(BatchArrayPushError::InvalidArrayHandle)?;
        let arr = match arr_node {
            BatchNode::Array(a) => a,
            _ => return Err(BatchArrayPushError::InvalidArrayHandle),
        };

        // Kind enforcement.
        if arr.kind == BatchArrayKind::Unset {
            arr.kind = item_kind;
        } else if arr.kind != item_kind {
            return Err(BatchArrayPushError::KindMismatch {
                array_kind: arr.kind,
                item_kind,
            });
        }

        arr.items.push(item_idx as i32);

        // Consume the item — it is now owned by the array.
        self.batch_consumed[item_idx] = true;

        Ok(())
    }

    /// Drain all items from a batch array (consuming the array).
    /// Returns the raw item indices and the array kind.
    pub fn take_batch_array(
        &mut self,
        handle: i32,
    ) -> Result<(Vec<i32>, BatchArrayKind), EmitError> {
        let node = self.take_batch_node(handle)?;
        match node {
            BatchNode::Array(arr) => Ok((arr.items, arr.kind)),
            _ => Err(EmitError::WrongNodeKind {
                handle,
                expected: "Array",
            }),
        }
    }

    /// Take a `BatchNode` and assert it is an `Expr`. Returns the inner `BatchExpr`.
    pub fn take_batch_expr(&mut self, handle: i32) -> Result<BatchExpr, EmitError> {
        let node = self.take_batch_node(handle)?;
        match node {
            BatchNode::Expr(e) => Ok(e),
            _ => Err(EmitError::WrongNodeKind {
                handle,
                expected: "BatchExpr",
            }),
        }
    }

    /// Take a `BatchNode` and assert it is a `Stmt`. Returns the inner `BatchStatement`.
    pub fn take_batch_stmt(&mut self, handle: i32) -> Result<BatchStatement, EmitError> {
        let node = self.take_batch_node(handle)?;
        match node {
            BatchNode::Stmt(s) => Ok(s),
            _ => Err(EmitError::WrongNodeKind {
                handle,
                expected: "BatchStmt",
            }),
        }
    }

    /// Take a `BatchNode` and assert it is a `Function`. Returns the inner `BatchFunction`.
    #[allow(dead_code)]
    pub fn take_batch_function(&mut self, handle: i32) -> Result<BatchFunction, EmitError> {
        let node = self.take_batch_node(handle)?;
        match node {
            BatchNode::Function(f) => Ok(f),
            _ => Err(EmitError::WrongNodeKind {
                handle,
                expected: "BatchFunction",
            }),
        }
    }

    /// Take a `BatchNode` and assert it is a `Method`. Returns the inner `BatchMethod`.
    #[allow(dead_code)]
    pub fn take_batch_method(&mut self, handle: i32) -> Result<BatchMethod, EmitError> {
        let node = self.take_batch_node(handle)?;
        match node {
            BatchNode::Method(m) => Ok(m),
            _ => Err(EmitError::WrongNodeKind {
                handle,
                expected: "BatchMethod",
            }),
        }
    }

    /// Take a `BatchNode` and assert it is a `Spec`. Returns the inner `BatchSpec`.
    pub fn take_batch_spec(&mut self, handle: i32) -> Result<BatchSpec, EmitError> {
        let node = self.take_batch_node(handle)?;
        match node {
            BatchNode::Spec(s) => Ok(s),
            _ => Err(EmitError::WrongNodeKind {
                handle,
                expected: "BatchSpec",
            }),
        }
    }

    /// Take a `BatchNode` and assert it is a `ClassSpec`. Returns the inner `BatchBuilderClass`.
    pub fn take_batch_class_spec(&mut self, handle: i32) -> Result<BatchBuilderClass, EmitError> {
        let node = self.take_batch_node(handle)?;
        match node {
            BatchNode::ClassSpec(c) => Ok(c),
            _ => Err(EmitError::WrongNodeKind {
                handle,
                expected: "BatchClassSpec",
            }),
        }
    }

    /// Extract a raw batch node by its internal index (bypasses consumed check).
    ///
    /// Used by `drain_batch_*` methods — items in an array are already marked
    /// consumed by `batch_array_push`. When we drain the array we need to move
    /// the data out; `take_batch_node` would refuse because consumed == true.
    fn extract_batch_node_raw(&mut self, raw_idx: i32) -> Result<BatchNode, EmitError> {
        let idx = raw_idx as usize;
        if idx == 0 || idx >= self.batch_nodes.len() {
            return Err(EmitError::OutOfRange {
                handle: raw_idx | BATCH_TAG,
            });
        }
        self.batch_nodes[idx].take().ok_or(EmitError::OutOfRange {
            handle: raw_idx | BATCH_TAG,
        })
    }

    /// Resolve all item indices in `items` into `BatchExpr` values.
    /// Items were already consumed by `batch_array_push`; we extract raw data.
    pub fn drain_batch_exprs(&mut self, items: Vec<i32>) -> Result<Vec<BatchExpr>, EmitError> {
        let mut out = Vec::with_capacity(items.len());
        for raw_idx in items {
            let node = self.extract_batch_node_raw(raw_idx)?;
            match node {
                BatchNode::Expr(e) => out.push(e),
                _ => {
                    return Err(EmitError::WrongNodeKind {
                        handle: raw_idx | BATCH_TAG,
                        expected: "BatchExpr",
                    })
                }
            }
        }
        Ok(out)
    }

    /// Resolve all item indices into `BatchStatement` values.
    pub fn drain_batch_stmts(&mut self, items: Vec<i32>) -> Result<Vec<BatchStatement>, EmitError> {
        let mut out = Vec::with_capacity(items.len());
        for raw_idx in items {
            let node = self.extract_batch_node_raw(raw_idx)?;
            match node {
                BatchNode::Stmt(s) => out.push(s),
                _ => {
                    return Err(EmitError::WrongNodeKind {
                        handle: raw_idx | BATCH_TAG,
                        expected: "BatchStmt",
                    })
                }
            }
        }
        Ok(out)
    }

    /// Resolve all item indices into `BatchParam` values.
    pub fn drain_batch_params(&mut self, items: Vec<i32>) -> Result<Vec<BatchParam>, EmitError> {
        let mut out = Vec::with_capacity(items.len());
        for raw_idx in items {
            let node = self.extract_batch_node_raw(raw_idx)?;
            match node {
                BatchNode::Param(p) => out.push(p),
                _ => {
                    return Err(EmitError::WrongNodeKind {
                        handle: raw_idx | BATCH_TAG,
                        expected: "BatchParam",
                    })
                }
            }
        }
        Ok(out)
    }

    /// Resolve all item indices into `BatchObjectField` values.
    pub fn drain_batch_object_fields(
        &mut self,
        items: Vec<i32>,
    ) -> Result<Vec<BatchObjectField>, EmitError> {
        let mut out = Vec::with_capacity(items.len());
        for raw_idx in items {
            let node = self.extract_batch_node_raw(raw_idx)?;
            match node {
                BatchNode::ObjectField(f) => out.push(f),
                _ => {
                    return Err(EmitError::WrongNodeKind {
                        handle: raw_idx | BATCH_TAG,
                        expected: "BatchObjectField",
                    })
                }
            }
        }
        Ok(out)
    }

    /// Resolve all item indices into `BatchField` (class field) values.
    pub fn drain_batch_fields(&mut self, items: Vec<i32>) -> Result<Vec<BatchField>, EmitError> {
        let mut out = Vec::with_capacity(items.len());
        for raw_idx in items {
            let node = self.extract_batch_node_raw(raw_idx)?;
            match node {
                BatchNode::Field(f) => out.push(f),
                _ => {
                    return Err(EmitError::WrongNodeKind {
                        handle: raw_idx | BATCH_TAG,
                        expected: "BatchField",
                    })
                }
            }
        }
        Ok(out)
    }

    /// Resolve all item indices into `BatchMethod` values.
    pub fn drain_batch_methods(&mut self, items: Vec<i32>) -> Result<Vec<BatchMethod>, EmitError> {
        let mut out = Vec::with_capacity(items.len());
        for raw_idx in items {
            let node = self.extract_batch_node_raw(raw_idx)?;
            match node {
                BatchNode::Method(m) => out.push(m),
                _ => {
                    return Err(EmitError::WrongNodeKind {
                        handle: raw_idx | BATCH_TAG,
                        expected: "BatchMethod",
                    })
                }
            }
        }
        Ok(out)
    }

    /// Resolve all item indices into `BatchFunction` values.
    pub fn drain_batch_functions(
        &mut self,
        items: Vec<i32>,
    ) -> Result<Vec<BatchFunction>, EmitError> {
        let mut out = Vec::with_capacity(items.len());
        for raw_idx in items {
            let node = self.extract_batch_node_raw(raw_idx)?;
            match node {
                BatchNode::Function(f) => out.push(f),
                _ => {
                    return Err(EmitError::WrongNodeKind {
                        handle: raw_idx | BATCH_TAG,
                        expected: "BatchFunction",
                    })
                }
            }
        }
        Ok(out)
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
        self.push_start_stmt_with_source(s, None);
    }

    /// Variant of `push_start_stmt` that also records the original Clean source
    /// text (when known — e.g. when the statement came from
    /// `_emit_stmt_from_source`) into `expansion.start_body_sources`. The
    /// parallel array stays 1:1 with `start_function.body` so downstream
    /// reconstruction can join sources without misalignment.
    pub fn push_start_stmt_with_source(&mut self, s: Statement, source: Option<String>) {
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
        self.expansion.start_body_sources.push(source);
    }

    /// Push a statement into `expansion.statements`, the inline-replacement
    /// vector consumed by the compiler's FrameworkBlock arm at expander.rs:585.
    ///
    /// Contrasts with `push_start_stmt` (§3.5 `_emit_statement_into_start`)
    /// which writes to `expansion.start_function.body`. Use this bridge when
    /// the statement should splice into the block's caller position, not the
    /// program's start. See Amendment 11 / §3.5.1.
    #[allow(dead_code)]
    pub fn push_inline_stmt(&mut self, s: Statement) {
        self.push_inline_stmt_with_source(s, None);
    }

    /// Variant of `push_inline_stmt` that also records the original Clean source
    /// text (when known) into `expansion.inline_stmt_sources`.
    pub fn push_inline_stmt_with_source(&mut self, s: Statement, source: Option<String>) {
        self.expansion.statements.push(s);
        self.expansion.inline_stmt_sources.push(source);
    }

    /// Look up the recorded source origin (if any) for a statement handle.
    /// Returns `None` for handles allocated by non-source bridges (batch
    /// builders, etc.).
    pub fn stmt_source_origin(&self, handle: i32) -> Option<&str> {
        self.stmt_source_origins.get(&handle).map(|s| s.as_str())
    }

    /// Record the source origin for a freshly-allocated statement handle.
    /// Called by `_emit_stmt_from_source` after `alloc_stmt`.
    pub fn record_stmt_source(&mut self, handle: i32, source: String) {
        self.stmt_source_origins.insert(handle, source);
    }

    /// Record the source origin for a freshly-allocated expression handle.
    /// Called by `_emit_expr_from_source` after `alloc_expr`.
    pub fn record_expr_source(&mut self, handle: i32, source: String) {
        self.expr_source_origins.insert(handle, source);
    }

    /// Push a function into `expansion.functions` and record its body source
    /// origin (if any) into the parallel `function_body_sources` array.
    /// Callers should pass the source of the body statement when it was
    /// allocated via `_emit_stmt_from_source`; pass `None` for structurally-
    /// built bodies.
    pub fn push_function_with_source(&mut self, f: Function, body_source: Option<String>) {
        self.expansion.functions.push(f);
        self.expansion.function_body_sources.push(body_source);
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

    #[allow(dead_code)]
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

    // ── Sticky-error tests (Amendment 5 / Landing C4) ─────────────────────────

    #[test]
    fn sticky_not_set_by_default() {
        let arena = EmitArena::new(1);
        assert!(!arena.is_sticky());
        assert_eq!(arena.had_error(), 0);
    }

    #[test]
    fn trip_sets_sticky_and_emits_plugin014_by_default() {
        let mut arena = EmitArena::new(1);
        let rv = arena.trip("_test_bridge", "handle out of range");
        assert_eq!(rv, 0);
        assert!(arena.is_sticky());
        assert_eq!(arena.had_error(), 1);
        assert!(arena.saw_error_severity);
        // Diagnostic should carry PLUGIN014.
        assert_eq!(arena.diagnostics.len(), 1);
        assert_eq!(arena.diagnostics[0].code, "PLUGIN014");
        assert!(arena.diagnostics[0].message.contains("_test_bridge"));
    }

    #[test]
    fn trip_uses_plugin_error_context_when_set() {
        let mut arena = EmitArena::new(1);
        arena.set_error_context("FRAME-AUTH-E001".to_string(), "helper failed".to_string());
        arena.trip("_batch_stringLit", "malformed lp");
        assert_eq!(arena.diagnostics[0].code, "FRAME-AUTH-E001");
        assert!(arena.diagnostics[0].message.starts_with("helper failed"));
        assert!(arena.diagnostics[0].message.contains("_batch_stringLit"));
    }

    #[test]
    fn trip_is_idempotent_only_first_emits() {
        let mut arena = EmitArena::new(1);
        arena.trip("_a", "reason1");
        arena.trip("_b", "reason2");
        arena.trip("_c", "reason3");
        assert_eq!(
            arena.diagnostics.len(),
            1,
            "only the first trip should emit a diagnostic"
        );
        assert!(arena.diagnostics[0].message.contains("_a"));
    }

    #[test]
    fn check_ctx_rejects_when_sticky() {
        let mut arena = EmitArena::new(7);
        // Correct ctx before sticky: OK.
        assert!(arena.check_ctx(7).is_ok());
        arena.trip("_setup", "seed");
        // Correct ctx after sticky: refused with StickyErrorActive.
        assert_eq!(arena.check_ctx(7), Err(EmitError::StickyErrorActive));
    }

    #[test]
    fn clear_error_resets_sticky_and_context() {
        let mut arena = EmitArena::new(1);
        arena.set_error_context("CODE".to_string(), "msg".to_string());
        arena.trip("_a", "x");
        assert!(arena.is_sticky());
        arena.clear_error();
        assert!(!arena.is_sticky());
        // Context also cleared: next trip emits default PLUGIN014.
        arena.trip("_b", "y");
        assert_eq!(arena.diagnostics.len(), 2);
        assert_eq!(arena.diagnostics[1].code, "PLUGIN014");
    }

    #[test]
    fn set_error_context_empty_clears() {
        let mut arena = EmitArena::new(1);
        arena.set_error_context("CODE".to_string(), "msg".to_string());
        arena.set_error_context(String::new(), String::new());
        arena.trip("_a", "x");
        assert_eq!(arena.diagnostics[0].code, "PLUGIN014");
    }
}
