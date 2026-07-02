/// Diagnostic emitted by a plugin via `_emit_error` during typed AST emission.
///
/// See typed-emission.md §5 for the full contract.
#[derive(Debug, Clone)]
pub struct EmitDiagnostic {
    /// 1 = warning, 2 = error.
    pub severity: u8,
    /// Plugin-supplied error code string (e.g. "MYPLUG001").
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// JSON span: `{"start":N,"end":N,"source":"block_body"|"attrs"|"plugin"}`.
    /// Empty string when the plugin did not supply a span.
    /// Stored for future diagnostic reporting in the IDE language server.
    #[allow(dead_code)]
    pub span_json: String,
}

/// Internal error type used within the arena — never crosses the ABI boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitError {
    /// The ctx handle provided by the caller does not match the arena's ctx.
    WrongCtx,
    /// Handle is 0 in a position where 0 is not allowed (non-nullable slot).
    NullHandle,
    /// Handle index is outside the allocated range.
    OutOfRange { handle: i32 },
    /// Handle was already consumed by a previous constructor call (PLUGIN008).
    HandleConsumed { handle: i32 },
    /// Handle refers to a node of the wrong variant (e.g. Expr used where Stmt expected).
    WrongNodeKind { handle: i32, expected: &'static str },
    /// The arena's sticky-error flag is set. All bridges short-circuit until
    /// the plugin clears it via `_ctx_clear_error(ctx)`. Not an error the
    /// plugin needs to distinguish — bridges just `return 0` — but exposed
    /// as a distinct variant so debug builds / tests can tell it apart from
    /// a genuine ctx mismatch (Amendment 5 / Landing C4).
    StickyErrorActive,
}

impl EmitError {
    /// Return the PLUGIN* error code for this error, for use in diagnostics.
    pub fn error_code(&self) -> &'static str {
        match self {
            EmitError::WrongCtx => "PLUGIN_WRONG_CTX",
            EmitError::NullHandle => "PLUGIN_NULL_HANDLE",
            EmitError::OutOfRange { .. } => "PLUGIN_OUT_OF_RANGE",
            EmitError::HandleConsumed { .. } => "PLUGIN008",
            EmitError::WrongNodeKind { .. } => "PLUGIN_WRONG_NODE_KIND",
            EmitError::StickyErrorActive => "PLUGIN014",
        }
    }
}
