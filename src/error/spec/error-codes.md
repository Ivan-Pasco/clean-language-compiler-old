# Clean Language Error Code Registry

**Authority:** This file is the master index of every diagnostic the compiler can emit.
**Version:** 1.0.0
**Date:** 2026-04-19

Every diagnostic the compiler emits carries a unique code. This file provides a navigable index of all codes, their categories, severity levels, and cross-references. For full rule definitions — conditions, examples, and messages — see `spec/semantic-rules.md`.

---

## Code Ranges

| Range | Category | Phase |
|-------|----------|-------|
| SYN001–SYN099 | Syntax | Parsing (lexer and parser) |
| SEM001–SEM099 | Semantic | HIR validation, resolver, type checker |
| SCOPE001–SCOPE099 | Scope | Resolver — name resolution and visibility |
| FUNC001–FUNC099 | Function | HIR validation — function definitions and calls |
| CLASS001–CLASS099 | Class | HIR validation — class definitions and inheritance |
| IDX001–IDX099 | Index | Type checker — collection access |
| STATE001–STATE099 | State | HIR validation and runtime — state management |
| IMPORT001–IMPORT099 | Import | Module resolver — imports and dependencies |
| PLUGIN001–PLUGIN099 | Plugin | Plugin loader — plugin registration and expansion |
| COM001–COM099 | Compilation | Code generation — WASM emission |
| RUN001–RUN099 | Runtime | WASM execution — runtime assertion failures |
| BLD-* | Build | Multi-file compiler — project layout and discovery |

---

## Severity Levels

| Level | Meaning | Compiler behavior |
|-------|---------|-------------------|
| **Error** | Code is invalid and cannot be compiled | Compilation stops after the current phase |
| **Warning** | Code is valid but may be unintentional | Compilation continues; diagnostic is reported |
| **Runtime** | Condition is checked at runtime, not compile time | Raised during WASM execution |

---

## Full Code Index

### Syntax Codes (SYN)

Emitted during parsing. No semantic information is available at this stage — the compiler has only seen tokens.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| SYN001 | `InvalidToken` | Character is not valid in any lexical context | Error | semantic-rules.md §SYN001 |
| SYN002 | `UnexpectedToken` | Token appears where the grammar does not allow it | Error | semantic-rules.md §SYN002 |
| SYN003 | `InvalidIndentation` | Spaces used instead of tabs, or inconsistent nesting level | Error | semantic-rules.md §SYN003 |
| SYN004 | `UnterminatedConstruct` | String literal, comment, or block not closed before end of file | Error | semantic-rules.md §SYN004 |
| SYN005 | `MalformedConstruct` | Partial syntax structure is missing required elements | Error | semantic-rules.md §SYN005 |
| SYN006 | `IndentationError` | Tab/space mixing detected, or indentation level is wrong for the block | Error | semantic-rules.md §SYN006 |
| SYN007 | `SectionOutOfOrder` | Core top-level sections appear out of the required order: `import → start → state → class → functions` | Error | semantic-rules.md §SYN007 |
| SYN008 | `InvalidPrintBlock` | `print:` block contains no expressions, or a non-expression item | Error | semantic-rules.md §SYN008 |
| SYN100 | `MissingSpecPath` | `spec` AI-metadata statement is missing its string-literal path | Error | semantic-rules.md §SYN100 |
| SYN101 | `MissingIntentDescription` | `intent` AI-metadata statement is missing its string-literal description | Error | semantic-rules.md §SYN101 |

### Semantic Codes (SEM)

Emitted during HIR validation, name resolution, and type checking.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| SEM001 | `TypeMismatch` | Expression type does not match the required type | Error | semantic-rules.md §SEM001 |
| SEM002 | `UndefinedSymbol` | Variable, function, class, or module referenced but not declared | Error | semantic-rules.md §SEM002 |
| SEM003 | `SymbolRedefinition` | Symbol declared more than once in the same scope | Error | semantic-rules.md §SEM003 |
| SEM004 | `InvalidOperationForType` | Operator or operation applied to an unsupported type | Error | semantic-rules.md §SEM004 |
| SEM005 | `AccessViolation` | Private member accessed from outside its allowed scope | Error | semantic-rules.md §SEM005 |
| SEM006 | `InheritanceError` | Class inheritance declaration is invalid | Error | semantic-rules.md §SEM006 |
| SEM007 | `GenericTypeError` | Generic or polymorphic type operation is invalid | Error | semantic-rules.md §SEM007 |
| SEM008 | `InheritanceCycle` | Class inherits from itself directly or indirectly | Error | semantic-rules.md §SEM008 |
| SEM009 | `InvalidTypeSpecification` | Type name does not refer to a valid type | Error | semantic-rules.md §SEM009 |
| SEM010 | `InvalidMatchPattern` | Argument to `string.matches()` is not one of the allowed pattern names | Error | semantic-rules.md §SEM010 |

### Scope Codes (SCOPE)

Emitted by the resolver during name resolution.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| SCOPE001 | `UseBeforeDeclaration` | Variable referenced before it is declared | Error | semantic-rules.md §SCOPE001 |
| SCOPE002 | `RedeclarationInScope` | Variable redeclared in the same scope | Error | semantic-rules.md §SCOPE002 |
| SCOPE003 | `MaxScopeDepthExceeded` | Scope nesting depth exceeds implementation limit | Error | semantic-rules.md §SCOPE003 |
| SCOPE004 | `WatchTargetNotState` | Watch block target does not reference a `state:` variable | Error | semantic-rules.md §SCOPE004 |
| SCOPE005 | `ScreenStateAccess` | Screen-local state variable accessed outside its screen block | Error | semantic-rules.md §SCOPE005 |

### Function Codes (FUNC)

Emitted during HIR validation of function definitions and call sites.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| FUNC001 | `FunctionNotDefined` | Function called before it is defined | Error | semantic-rules.md §FUNC001 |
| FUNC002 | `ArgumentCountMismatch` | Argument count does not match parameter count | Error | semantic-rules.md §FUNC002 |
| FUNC003 | `CallOnNonFunction` | Parenthesis-invocation of a non-function symbol | Error | semantic-rules.md §FUNC003 |
| FUNC004 | `MissingReturn` | Non-void function has no return on some execution path | Error | semantic-rules.md §FUNC004 |
| FUNC005 | `EmptyReturnInNonVoid` | `return` with no value in a non-void function | Warning | semantic-rules.md §FUNC005 |
| FUNC006 | `StartBlockHasParameters` | The `start:` entry point block declares parameters | Error | semantic-rules.md §FUNC006 |
| FUNC007 | `StartBlockReturnsValue` | The `start:` entry point returns a value | Warning | semantic-rules.md §FUNC007 |
| FUNC008 | `UnknownNamedArgument` | Named argument label does not match any parameter name | Error | semantic-rules.md §FUNC008 |
| FUNC009 | `DuplicateNamedArgument` | Same named argument label appears more than once in a call | Error | semantic-rules.md §FUNC009 |
| FUNC010 | `PositionalAfterNamed` | Positional argument appears after a named argument in a call | Error | semantic-rules.md §FUNC010 |
| FUNC011 | `NamedArgCoverageError` | Named argument count or coverage does not match parameter count | Error | semantic-rules.md §FUNC011 |
| FUNC012 | `MethodCallOnStandaloneFunction` | A method-call syntax (`receiver.fn()`) was used on a plain function that is not a method | Error | Emitted by resolver when dot-call notation is applied to a non-method symbol |

### Class Codes (CLASS)

Emitted during HIR validation of class definitions.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| CLASS001 | `ParentClassNotFound` | Parent class named in `is ParentName` is not defined | Error | semantic-rules.md §CLASS001 |
| CLASS002 | `DuplicateField` | Two fields in the same class share a name | Error | semantic-rules.md §CLASS002 |
| CLASS003 | `DuplicateMethod` | Two methods in the same class share a name | Error | semantic-rules.md §CLASS003 |
| CLASS004 | `MissingConstructor` | Class is instantiated but has no constructor | Error | semantic-rules.md §CLASS004 |
| CLASS005 | `EnsureAfterLogic` | `ensure` statement appears after non-contract logic in a function body | Error | semantic-rules.md §CLASS005 |
| CLASS006 | `AlwaysConditionNotBoolean` | Expression inside an `always:` invariant block is not boolean | Error | semantic-rules.md §CLASS006 |
| CLASS007 | `ParameterShadowsField` | A method parameter, or a parameter of a constructor with a body, has the name of a field of its class | Error | semantic-rules.md §CLASS007 |

### Index Access Codes (IDX)

Emitted by the type checker when bracket access is used.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| IDX001 | `ArrayIndexNotInteger` | List bracket access uses a non-integer index | Error | semantic-rules.md §IDX001 |
| IDX002 | `MatrixIndexNotInteger` | Matrix bracket access uses a non-integer index | Error | semantic-rules.md §IDX002 |
| IDX003 | `PairsKeyNotString` | Pairs bracket access uses a non-string key | Error | semantic-rules.md §IDX003 |
| IDX004 | `IndexOnNonIndexable` | Bracket access on a type that is not `list`, `matrix`, `pairs`, or `any` | Error | semantic-rules.md §IDX004 |

### State Codes (STATE)

Emitted during HIR validation (compile-time) and during WASM execution (runtime).

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| STATE001 | `GuardConditionNotPure` | Guard expression is not a pure boolean or contains side effects | Error | semantic-rules.md §STATE001 |
| STATE002 | `GuardRejection` | State update rejected at runtime because guard evaluated to false | Runtime | semantic-rules.md §STATE002 |
| STATE003 | `ComputedReturnTypeMismatch` | Computed state body returns a type that does not match the declared type, or circular dependency detected | Error | semantic-rules.md §STATE003 |
| STATE004 | `ComputedStateAssignment` | Assignment to a computed state variable (which is read-only) | Error | semantic-rules.md §STATE004 |
| STATE005 | `RulesExpressionNotBoolean` | Expression inside a `state: rules:` block is not boolean | Error | semantic-rules.md §STATE005 |

### Import Codes (IMPORT)

Emitted by the module resolver.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| IMPORT001 | `CircularDependency` | Two or more modules import each other in a cycle | Error | semantic-rules.md §IMPORT001 |
| IMPORT002 | `ModuleNotFound` | Imported module does not exist in any search path | Error | semantic-rules.md §IMPORT002 |
| IMPORT003 | `SymbolNotInModule` | Specific symbol imported from a module is not exported by that module | Error | semantic-rules.md §IMPORT003 |
| IMPORT004 | `DuplicateImportItem` | Same item appears more than once in an import list | Error | semantic-rules.md §IMPORT004 |

### Plugin Codes (PLUGIN)

Emitted by the plugin loader.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| PLUGIN001 | `PluginNotFound` | Referenced plugin is not installed | Error | semantic-rules.md §PLUGIN001 |
| PLUGIN002 | `PluginRegistrationConflict` | Two plugins attempt to register the same function or block handler | Error | semantic-rules.md §PLUGIN002 |
| PLUGIN006 | `UnsupportedExpansionVersion` | Plugin declares `expansion_version` higher than the compiler's `HIGHEST_SUPPORTED_EXPANSION_VERSION`; the compiler refuses to load it rather than silently falling back to a wrong ABI | Error | typed-emission.md §14 |
| PLUGIN007 | `BridgeHostClassMismatchInEmission` | `_emit_external` was called with a `host_class_lp` that does not match the active build target (e.g. a server-only bridge emitted for a browser build) | Error | typed-emission.md §14 |
| PLUGIN008 | `ConsumedHandleReuse` | A plugin passed a handle to a constructor that had already consumed it; typed-emission enforces single-ownership on every arena node | Error | typed-emission.md §14 |
| PLUGIN009 | `PluginTrapDuringTypedEmission` | The plugin's `expand_block_typed` WASM function trapped (unreachable, OOB memory access, etc.) during a v3 typed-emission call | Error | typed-emission.md §14 |
| PLUGIN010 | `LegacyV1PluginRejected` | A plugin built against the v1 string-emission ABI was refused by a compiler build mode that requires typed emission (e.g. strict mode or a future compiler release that drops v1 support) | Error | typed-emission.md §14 |
| PLUGIN012 | `ExpressionInStatementContext` | The user-authored source fragment passed to `_emit_stmt_from_source` parses as a valid expression but is not a valid statement in a statement context (e.g. bare `1 + 1`) | Error | typed-emission.md §3.11 |
| PLUGIN013 | `InvalidBatchSpec` | The JSON spec passed to `_emit_helpers_batch` or `_emit_class_full` is malformed, exceeds the 100-function batch limit, contains an unknown statement or expression `kind`, or references an unresolvable type or identifier | Error | typed-emission.md §3.12, §3.13 |
| PLUGIN014 | `StickyErrorTripped` | A typed-emission bridge failed (null / consumed / out-of-range handle, LP-string malformed, JSON invalid, etc.) and set the arena's sticky-error flag. All subsequent bridge calls short-circuit until the plugin queries `_ctx_had_error(ctx)` and returns, or calls `_ctx_clear_error(ctx)`. The auto-emitted diagnostic uses this code by default; plugins MAY override the (code, message) pair via `_ctx_set_error_context(ctx, code, message)` before the first failure to attribute the diagnostic to a plugin-authored code | Error | typed-emission.md §3.15 (Amendment 5) |
| PLUGIN015 | `InvalidExpressionSource` | The user-authored source fragment passed to `_emit_expr_from_source` is not a syntactically valid Clean expression | Error | typed-emission.md §3.16 (Amendment 6) |
| PLUGIN016 | `StatementInExpressionContext` | The user-authored source fragment passed to `_emit_expr_from_source` parses as a valid statement but is not a valid expression in an expression context (e.g. `return 42`, `x = 1`) — symmetric to PLUGIN012 | Error | typed-emission.md §3.16 (Amendment 6) |
| PLUGIN018 | `AssembleTypedIllegalBridge` | A structural typed-emission bridge (`_emit_function`, `_emit_class_full`, `_emit_stmt_from_source`, etc.) was called from inside `assemble_typed`. Only `_inject_source_file` is legal in that slot | Error | assemble.md §6.2 |

### Compilation Codes (COM)

Emitted during WASM code generation.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| COM001 | `WasmGenerationError` | Code generator failed to produce valid WASM for a construct | Error | semantic-rules.md §COM001 |
| COM002 | `OptimizationError` | Optimization pass produced invalid code | Error | semantic-rules.md §COM002 |
| COM003 | `MemoryLayoutError` | Memory allocation or layout calculation failed | Error | semantic-rules.md §COM003 |
| COM004 | `ModuleResolutionError` | Multi-file compilation failed to resolve module dependencies | Error | semantic-rules.md §COM004 |
| COM005 | `TargetSpecificError` | Compilation target does not support a required feature | Error | semantic-rules.md §COM005 |
| COM006 | `FunctionNotFoundDuringCompilation` | Function passed semantic analysis but could not be located during code generation | Error | semantic-rules.md §COM006 |

### Build Codes (BLD)

Emitted by the multi-file compiler during project layout discovery.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| BLD-LAYOUT | `ObsoleteRenderTargetLayout` | Project still uses the pre-frame.ui-2.7 `app/web/` folder convention while the active plugins describe the nested `app/ui/web/` layout | Error | semantic-rules.md §BLD-LAYOUT |
| BLD-MANIFEST | `CodeAfterPackageManifest` | A `package:` manifest file contains a top-level line (`start:`, `functions:`, ...) after the manifest; that code would never be compiled | Error | semantic-rules.md §BLD-MANIFEST |

### Runtime Codes (RUN)

Not emitted by the compiler. Raised during WASM execution by the host runtime.

| Code | Name | Short Description | Severity | Defined In |
|------|------|-------------------|----------|------------|
| RUN001 | `MemoryViolation` | WASM execution accessed memory outside allocated bounds | Runtime | semantic-rules.md §RUN001 |
| RUN002 | `StackError` | WASM stack overflow or underflow during execution | Runtime | semantic-rules.md §RUN002 |
| RUN003 | `ArithmeticError` | Division by zero or other invalid arithmetic at runtime | Runtime | semantic-rules.md §RUN003 |
| RUN004 | `ReferenceError` | None or invalid reference accessed at runtime (raised by `expr!` on none) | Runtime | semantic-rules.md §RUN004 |
| RUN005 | `AssertionFailure` | A `require` statement evaluated to false at runtime | Runtime | semantic-rules.md §RUN005 |

---

## Diagnostic Format

When the compiler emits a diagnostic, the message follows this format:

```
<file>:<line>:<column>: <severity> [<CODE>] <message>
```

**Example:**
```
main.cln:12:5: error [SEM001] type mismatch: expected 'integer', got 'string'
```

Every compiler-produced diagnostic must include a code. A diagnostic without a code is incomplete and violates Principle 4.

---

## Reserved Ranges

The following code numbers are reserved for future use. Do not assign them without updating this registry and `spec/semantic-rules.md`.

| Range | Status |
|-------|--------|
| SYN009–SYN099 | Reserved |
| SYN102–SYN199 | Reserved (extension range for AI-metadata syntax codes) |
| SEM011–SEM099 | Reserved |
| SCOPE006–SCOPE099 | Reserved |
| FUNC013–FUNC099 | Reserved |
| CLASS008–CLASS099 | Reserved |
| IDX005–IDX099 | Reserved |
| STATE006–STATE099 | Reserved |
| IMPORT005–IMPORT099 | Reserved |
| PLUGIN003–PLUGIN005 | Reserved |
| PLUGIN006–PLUGIN010 | Assigned — typed-emission ABI errors (sub-cycle 2, typed-emission.md §14) |
| PLUGIN012 | Assigned — `ExpressionInStatementContext` (typed-emission.md §3.11) |
| PLUGIN013 | Assigned — `InvalidBatchSpec` (typed-emission.md §3.12, §3.13; Amendment 3) |
| PLUGIN014 | Assigned — `StickyErrorTripped` (typed-emission.md §3.15; Amendment 5) |
| PLUGIN015 | Assigned — `InvalidExpressionSource` (typed-emission.md §3.16; Amendment 6) |
| PLUGIN016 | Assigned — `StatementInExpressionContext` (typed-emission.md §3.16; Amendment 6) |
| PLUGIN018 | Assigned — `AssembleTypedIllegalBridge` (assemble.md §6.2) |
| PLUGIN011, PLUGIN017, PLUGIN019–PLUGIN099 | Reserved |
| COM007–COM099 | Reserved |
| RUN006–RUN099 | Reserved |
