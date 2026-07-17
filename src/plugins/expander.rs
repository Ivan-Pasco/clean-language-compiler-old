/*!
 * Plugin Expander - AST transformation pass for framework block expansion
 *
 * This module provides the expansion pass that runs between parsing and HIR
 * transformation. It walks the AST, finds FrameworkBlock nodes, and replaces
 * them with expanded Clean Language statements using registered plugins.
 */

use super::{FrameworkBlock, PluginError, PluginRegistry};
use crate::ast::{Class, ExternalFunction, Function, Program, StateBlock, Statement};
use crate::error::CompilerError;
use crate::plugins::enforcement::validate_plugin_permissions;

/// Merge `incoming` into `target` using the contract described on
/// `PluginExpansion::state`: declarations from `incoming` are appended after
/// any existing declarations (preserving plugin-then-user order across the
/// life of the expander, since plugin-contributed state is accumulated into
/// `pending_state` before the user's `program.state` is folded in by the
/// caller), computed values likewise. At most one `rules:` block survives;
/// duplicate rules blocks would silently overwrite each other so we keep the
/// first non-None and the caller is responsible for raising an error if two
/// distinct sources contribute one. `scope` is taken from the first non-None
/// block; mismatched scopes are also a caller concern (top-level `state:` is
/// always App scope per `foundation/spec/semantic-rules.md`).
fn merge_state_block(target: &mut Option<StateBlock>, incoming: Option<StateBlock>) {
    let incoming = match incoming {
        Some(s) => s,
        None => return,
    };
    match target {
        None => *target = Some(incoming),
        Some(existing) => {
            existing.declarations.extend(incoming.declarations);
            existing.computed.extend(incoming.computed);
            if existing.rules.is_none() {
                existing.rules = incoming.rules;
            }
            // existing.scope and existing.location preserved — first writer wins.
        }
    }
}

/// HYDRATE_AUTO Gap 2 — emit bare-named top-level dispatch functions for
/// every class method declared inside an `events:` block.
///
/// For each `FunctionModifier::EventHandler` method on a class, find the
/// state-block declaration whose declared type is that class, and emit a
/// top-level free function that dispatches through the state global:
///
/// ```clean
/// state:
///     MyToolbar instance_my_toolbar = MyToolbar()
///
/// class MyToolbar
///     events:
///         void fmt_bold()
///             ...
/// ```
///
/// produces an additional top-level function:
///
/// ```clean
/// functions:
///     void fmt_bold()
///         instance_my_toolbar.fmt_bold()
/// ```
///
/// The browser loader looks methods up via
/// `instance.exports[handlerName]()` — a bare-named free function with no
/// `this` parameter. The class method itself stays where it is (the splice
/// and any user code still call `instance.fmt_bold()` through the receiver),
/// but the shim is what reaches the export table by name.
///
/// Strategy for finding the matching instance global is the singleton
/// heuristic: any state declaration whose declared type is a `Named` type
/// with the class's name. Today's component model has one instance per
/// `component:` tag in a file, which produces exactly one such state
/// declaration per class. The multi-instance case (multiple
/// `<my-toolbar id="...">` islands on a page) needs more design work —
/// either a tag-parametrised thunk signature or per-instance dispatch tables
/// on the JS side — and is not in scope here. When multiple matches are
/// found we pick the first deterministic-order candidate and continue;
/// when no match is found we skip emission for that class entirely (the
/// loader-side call site will simply log "not found", same as before).
///
/// Name collisions (a user `functions:` block already declaring a function
/// with the same name as a shim) are resolved by the user winning: we skip
/// the shim. The user's function reaches the export table the normal way,
/// and the event handler method becomes effectively shadowed for loader
/// dispatch — an explicit user override.
pub fn emit_event_handler_shims(program: &mut Program) {
    use crate::ast::{Expression, FunctionModifier, Statement, Type, Visibility};

    if program.classes.is_empty() {
        return;
    }
    let state_decls = match &program.state {
        Some(state) => &state.declarations,
        None => return,
    };

    let mut shims: Vec<Function> = Vec::new();
    for class in &program.classes {
        // Find a state declaration whose declared type names this class.
        // The parser uses Type::Object for `FormatToolbar instance = ...`-style
        // declarations and Type::Class { name, .. } for the generic form
        // (`FormatToolbar<T> instance = ...`); both shapes resolve to the same
        // class symbol downstream so we accept either.
        let state_var_name = state_decls.iter().find_map(|decl| match &decl.type_ {
            Type::Object(name) if name == &class.name => Some(decl.name.clone()),
            Type::Class { name, .. } if name == &class.name => Some(decl.name.clone()),
            _ => None,
        });
        let state_var_name = match state_var_name {
            Some(name) => name,
            None => continue,
        };

        for method in &class.methods {
            if !matches!(method.modifier, FunctionModifier::EventHandler) {
                continue;
            }
            // User-defined top-level function with the same name wins.
            if program.functions.iter().any(|f| f.name == method.name) {
                continue;
            }
            if shims.iter().any(|f| f.name == method.name) {
                continue;
            }
            // Tag the shim with PLUGIN_OUTPUT_V2_ROOT_MARKER so it survives
            // both BFS root selection in `collect_all_called_names_from_mir`
            // and the PLUGIN_OUTPUT_MARKER DCE pass in `mir_codegen::generate`.
            //
            // Without this tag, the shim inherits the source method's
            // location — which for plugin-emitted classes (frame.ui's
            // `expand_component` output) carries PLUGIN_OUTPUT_MARKER. The
            // BFS roots list excludes PLUGIN_OUTPUT_MARKER functions from
            // being seeded, and the DCE pass then drops them entirely
            // unless they're transitively called from user code. Nothing in
            // user code calls these shims — the only call site is the
            // browser loader's `instance.exports[handlerName]()` JS
            // dispatch, which the compiler's static reachability analysis
            // cannot see. V2 root marker explicitly opts the shim into
            // root status, which is exactly the contract documented at
            // `mir_codegen/utilities.rs:1415-1424`.
            let shim_location = crate::ast::SourceLocation {
                file: crate::ast::PLUGIN_OUTPUT_V2_ROOT_MARKER.to_string(),
                line: method.location.as_ref().map(|l| l.line).unwrap_or(0),
                column: method.location.as_ref().map(|l| l.column).unwrap_or(0),
                byte_start: None,
                byte_end: None,
            };
            // Body: `instance_<tag>.<method>()` — single expression statement.
            let dispatch = Expression::MethodCall {
                object: Box::new(Expression::Variable(state_var_name.clone())),
                method: method.name.clone(),
                arguments: Vec::new(),
                location: shim_location.clone(),
            };
            let body = vec![Statement::Expression {
                expr: dispatch,
                location: Some(shim_location.clone()),
            }];
            // Construct as Function::new then patch visibility/syntax so we
            // don't touch every site that builds Function.  The shim is a
            // public top-level free function with the bare method name; it
            // takes no arguments, returns void, and lives in the synthetic
            // V2-root location so it survives DCE.
            let mut shim = Function::new(
                method.name.clone(),
                Vec::new(),
                Type::Void,
                body,
                Some(shim_location),
            );
            shim.visibility = Visibility::Public;
            // EventHandler tag is for class methods inside `events:`. The
            // shim itself is a plain top-level function — leave its modifier
            // as None so downstream passes (e.g. another expansion sweep)
            // don't mistake it for a class method.
            shim.modifier = FunctionModifier::None;
            shims.push(shim);
        }
    }
    program.functions.extend(shims);
}

/// Prepend statements to the program's start function body, creating the
/// start function if it doesn't exist. Used by lifecycle slot dispatch to
/// splice plugin-contributed init code ahead of any user `start:` code.
/// See `contracts/lifecycle.md` §3.2 / §3.4.
fn prepend_to_start(program: &mut Program, statements: Vec<Statement>) {
    if statements.is_empty() {
        return;
    }
    match program.start_function.as_mut() {
        Some(start_fn) => {
            // Prepend so the lifecycle slot runs before any user start: code.
            let mut new_body = statements;
            new_body.append(&mut start_fn.body);
            start_fn.body = new_body;
        }
        None => {
            program.start_function = Some(Function::new(
                "start".to_string(),
                Vec::new(),
                crate::ast::Type::Void,
                statements,
                None,
            ));
        }
    }
}

/// AST expander that transforms framework blocks into Clean Language code
pub struct PluginExpander<'a> {
    registry: &'a PluginRegistry,
    /// Track statistics for reporting
    blocks_expanded: usize,
    statements_generated: usize,
    /// Pending start function from plugin expansion
    pending_start: Option<Function>,
    /// Pending functions to add to the program
    pending_functions: Vec<Function>,
    /// Pending class definitions from plugin expansion (e.g., ORM models)
    pending_classes: Vec<crate::ast::Class>,
    /// Pending external functions from plugin expansion
    pending_externals: Vec<ExternalFunction>,
    /// Pending top-level `state:` block contributed by plugin expansion.
    /// Multiple plugin blocks emitting state: blocks (e.g. several `component:`
    /// blocks each contributing one `instance_<tag>` global) accumulate here
    /// via `merge_state_block`. The merged result is folded into
    /// `program.state` by `expand_program` and `expand_program_without_preambles`.
    pending_state: Option<StateBlock>,
    /// Permission violations collected during expansion (non-fatal; reported as errors)
    permission_errors: Vec<CompilerError>,
    /// Plugin Contracts v2 — whether this is a server-target build. Used to
    /// gate the `server_init` lifecycle slot per contracts/lifecycle.md §3.4.
    is_server_build: bool,
    /// Plugin Contracts v2 — whether this is a client-target build (the
    /// nested `frontend.wasm` compilation). The `client_init` slot is
    /// dispatched only here.
    is_client_build: bool,
    /// Plugin Contracts v2 — build context passed to every lifecycle slot
    /// call. See `contracts/lifecycle.md` §2.1. Set via `with_build_context`;
    /// defaults to an empty context populated only with versioning fields.
    build_context: super::BuildContext,
}

impl<'a> PluginExpander<'a> {
    /// Create a new expander with the given plugin registry.
    ///
    /// Defaults to a server-target build. Override via `with_build_target` if
    /// the parent build is producing client (browser) WASM.
    pub fn new(registry: &'a PluginRegistry) -> Self {
        Self {
            registry,
            blocks_expanded: 0,
            statements_generated: 0,
            pending_start: None,
            pending_functions: Vec::new(),
            pending_classes: Vec::new(),
            pending_externals: Vec::new(),
            pending_state: None,
            permission_errors: Vec::new(),
            is_server_build: true,
            is_client_build: false,
            build_context: super::BuildContext::new(),
        }
    }

    /// Plugin Contracts v2 — declare which build shape this expansion pass
    /// produces. Server builds receive `server_init` slot output; client
    /// builds receive `client_init`. Both receive `program_init` and
    /// `module_helpers`. See contracts/lifecycle.md §3.
    pub fn with_build_target(mut self, is_server: bool, is_client: bool) -> Self {
        self.is_server_build = is_server;
        self.is_client_build = is_client;
        self.build_context.target = if is_client {
            "browser".to_string()
        } else {
            "server".to_string()
        };
        self.build_context.client_mode = is_client;
        self
    }

    /// Plugin Contracts v2 — set the build context that gets passed to every
    /// lifecycle slot call. The registry takes a fresh `build_state` snapshot
    /// for every slot dispatch so plugins always see writes from earlier
    /// expand_block calls. See `contracts/lifecycle.md` §2.1, §2.5.
    pub fn with_build_context(mut self, context: super::BuildContext) -> Self {
        // Preserve the target derivation from `with_build_target` if it was
        // called first — the caller's explicit target wins over the default.
        let target = self.build_context.target.clone();
        let client_mode = self.build_context.client_mode;
        self.build_context = context;
        if !target.is_empty() {
            self.build_context.target = target;
            self.build_context.client_mode = client_mode;
        }
        self
    }

    /// Expand all framework blocks in a program
    ///
    /// This is the main entry point for the expansion pass. It walks the entire
    /// AST and replaces FrameworkBlock statements with their expanded form.
    ///
    /// # Arguments
    /// * `program` - The parsed program AST
    ///
    /// # Returns
    /// * `Ok(Program)` - The program with all framework blocks expanded
    /// * `Err(PluginError)` - If expansion fails
    pub fn expand_program(&mut self, mut program: Program) -> Result<Program, PluginError> {
        // Expand top-level statements using full expansion (which captures start functions)
        program.statements = self.expand_statements_full(program.statements)?;

        // Expand the user's start: block body. Without this, framework blocks
        // appearing inside start: — e.g. `integer x = Model.find: ...` — are
        // never dispatched to their plugin and reach HIR construction as raw
        // OrmQuery nodes, where they would fail SEM001. Top-level FrameworkBlock
        // statements go through expand_statements_full above, but VariableDecl
        // + OrmQuery initializers and ORM expression statements only become
        // visible to the plugin dispatch inside expand_statements (which is
        // also what we use for function bodies).
        if let Some(ref mut start_fn) = program.start_function {
            let body = std::mem::take(&mut start_fn.body);
            start_fn.body = self.expand_statements(body)?;
        }

        // Expand statements in functions
        program.functions = self.expand_functions(program.functions)?;

        // Expand statements in classes
        program.classes = self.expand_classes(program.classes)?;

        // Merge pending start function into program
        if let Some(start_fn) = self.pending_start.take() {
            if program.start_function.is_none() {
                program.start_function = Some(start_fn);
            } else {
                // Merge: append plugin start function body to existing start function
                if let Some(ref mut existing) = program.start_function {
                    existing.body.extend(start_fn.body);
                }
            }
        }

        // Merge pending functions into program
        program.functions.append(&mut self.pending_functions);

        // Merge pending classes into program (e.g., ORM models from frame.data)
        program.classes.append(&mut self.pending_classes);

        // Merge pending externals into program (deduplicate by name)
        for ext in self.pending_externals.drain(..) {
            if !program.externals.iter().any(|e| e.name == ext.name) {
                program.externals.push(ext);
            }
        }

        // Merge pending state into program. Plugin-contributed declarations
        // come first (accumulated in pending_state); the user's existing
        // state block (if any) is appended after. Keeping that order makes
        // user code able to read plugin-emitted instance globals during its
        // own start: body without ordering surprises.
        if self.pending_state.is_some() {
            let plugin_state = self.pending_state.take();
            let user_state = program.state.take();
            program.state = plugin_state;
            merge_state_block(&mut program.state, user_state);
        }

        // Plugin Contracts v2 — dispatch lifecycle slots.
        // See foundation/spec/plugins/contracts/lifecycle.md §3.
        //
        // Slots are dispatched in a fixed order so plugins can reason about
        // what's available when they're called:
        //   1. module_helpers  → top-level functions/classes (every file)
        //   2. program_init    → program start: prelude (every build)
        //   3. server_init     → server bootstrap (server builds only)
        //   4. client_init     → client _start (client builds only)
        //   5. per_request     → request handler prelude (§2 follow-up — depends
        //                        on route-handler emission rework)
        //
        // Plugins that have not declared a slot in their manifest are skipped
        // at the registry level — no WASM instantiation cost for inactive
        // slots. See PluginRegistry::invoke_lifecycle_slot.
        //
        // module_helpers contributes top-level functions/classes which are
        // merged in-place. The other slots contribute statements that go
        // into the start function. To preserve declaration order across
        // multiple slots, we accumulate their outputs first, then prepend
        // the assembled block to the existing start body in a single splice.
        self.merge_module_helpers(&mut program);
        let mut slot_prelude: Vec<Statement> = Vec::new();
        self.collect_slot_statements(&mut slot_prelude, "program_init");
        if self.is_server_build {
            self.collect_slot_statements(&mut slot_prelude, "server_init");
        }
        if self.is_client_build {
            // §1 — client_init contributes the body of the synthetic client
            // _start. Plugins that opted in (e.g. frame.ui's emit_ui_client_init)
            // emit component-instantiation + onMount() calls here.
            // Closes HYDRATE_AUTO once a plugin declares the slot.
            self.collect_slot_statements(&mut slot_prelude, "client_init");

            // CLIENT_PULLS_SERVER: The user's start: body is server-side code
            // (HTTP listen, migration registration, route setup). Clear it so
            // _start in frontend.wasm contains only browser-safe plugin output
            // (program_init + client_init). Without this, prepend_to_start leaves
            // the server SSR entry reachable from _start, pulling the entire
            // migration chain into the browser WASM via DCE BFS.
            if let Some(ref mut sf) = program.start_function {
                sf.body.clear();
            }
        }
        // per_request lifecycle splicing into route handlers — follow-up
        // commit. Touches route-handler emission which is more involved than
        // the program-level slots above.
        if !slot_prelude.is_empty() {
            prepend_to_start(&mut program, slot_prelude);
        }

        emit_event_handler_shims(&mut program);

        tracing::debug!(
            blocks_expanded = self.blocks_expanded,
            statements_generated = self.statements_generated,
            externals_added = program.externals.len(),
            "Plugin expansion complete"
        );

        Ok(program)
    }

    /// Plugin Contracts v2 — merge `module_helpers` contributions into the
    /// program's top-level functions/classes/externals. See `lifecycle.md`
    /// §3.1. Duplicate-name entries are silently skipped per the v1 preamble
    /// merge semantics.
    fn merge_module_helpers(&self, program: &mut Program) {
        for expansion in self
            .registry
            .invoke_lifecycle_slot("module_helpers", &self.build_context)
        {
            for func in expansion.functions {
                if !program.functions.iter().any(|f| f.name == func.name) {
                    program.functions.push(func);
                }
            }
            for class in expansion.classes {
                if !program.classes.iter().any(|c| c.name == class.name) {
                    program.classes.push(class);
                }
            }
            for ext in expansion.externals {
                if !program.externals.iter().any(|e| e.name == ext.name) {
                    program.externals.push(ext);
                }
            }
            // Statements at module scope are rare for module_helpers; if
            // present, they accumulate into the slot prelude later.
        }
    }

    /// Plugin Contracts v2 — collect a single slot's contributed statements
    /// in declaration order. Used by `expand_program` to accumulate all slot
    /// outputs in fixed order (program_init → server_init → client_init)
    /// before splicing the assembled prelude into the start function.
    /// See `lifecycle.md` §3.
    fn collect_slot_statements(&self, accumulator: &mut Vec<Statement>, slot: &str) {
        for expansion in self
            .registry
            .invoke_lifecycle_slot(slot, &self.build_context)
        {
            let mut stmts: Vec<Statement> = expansion.statements;
            if let Some(start_fn) = expansion.start_function {
                stmts.extend(start_fn.body);
            }
            // Tag every injected statement so downstream passes can distinguish
            // plugin-contributed lifecycle output from user-authored start: code.
            for stmt in &mut stmts {
                let loc = stmt.location_mut();
                match loc {
                    Some(l) => l.file = crate::ast::LIFECYCLE_SLOT_OUTPUT_MARKER.to_string(),
                    None => {
                        *loc = Some(crate::ast::SourceLocation {
                            file: crate::ast::LIFECYCLE_SLOT_OUTPUT_MARKER.to_string(),
                            line: 0,
                            column: 0,
                            byte_start: None,
                            byte_end: None,
                        });
                    }
                }
            }
            accumulator.extend(stmts);
        }
    }

    /// Like `expand_program` but skips preamble injection.
    ///
    /// Used for shared/non-entry modules in a multi-file build.  Preambles
    /// (e.g. `redirect`, `json`, `error` from frame.server) are program-level
    /// helpers that must appear exactly once in the merged HIR.  Injecting
    /// them into every module causes N duplicate symbols in the resolver (E001).
    /// The entry module calls `expand_program` (with preambles); all other
    /// modules call this variant so framework blocks are still expanded but
    /// no preamble helpers are duplicated.
    pub fn expand_program_without_preambles(
        &mut self,
        mut program: Program,
    ) -> Result<Program, PluginError> {
        program.statements = self.expand_statements_full(program.statements)?;
        // See expand_program: start: body must also be walked so plugin
        // dispatch reaches OrmQuery expressions / VariableDecls inside it.
        if let Some(ref mut start_fn) = program.start_function {
            let body = std::mem::take(&mut start_fn.body);
            start_fn.body = self.expand_statements(body)?;
        }
        program.functions = self.expand_functions(program.functions)?;
        program.classes = self.expand_classes(program.classes)?;

        if let Some(start_fn) = self.pending_start.take() {
            if program.start_function.is_none() {
                program.start_function = Some(start_fn);
            } else if let Some(ref mut existing) = program.start_function {
                existing.body.extend(start_fn.body);
            }
        }

        program.functions.append(&mut self.pending_functions);
        program.classes.append(&mut self.pending_classes);

        for ext in self.pending_externals.drain(..) {
            if !program.externals.iter().any(|e| e.name == ext.name) {
                program.externals.push(ext);
            }
        }

        // Mirror expand_program: plugin-contributed state goes first, user
        // state appended. See the equivalent block there for rationale.
        if self.pending_state.is_some() {
            let plugin_state = self.pending_state.take();
            let user_state = program.state.take();
            program.state = plugin_state;
            merge_state_block(&mut program.state, user_state);
        }

        emit_event_handler_shims(&mut program);

        tracing::debug!(
            blocks_expanded = self.blocks_expanded,
            statements_generated = self.statements_generated,
            externals_added = program.externals.len(),
            "Plugin expansion complete (no preambles — shared module)"
        );

        Ok(program)
    }

    /// Expand framework blocks using full expansion (preserves start functions)
    fn expand_statements_full(
        &mut self,
        statements: Vec<Statement>,
    ) -> Result<Vec<Statement>, PluginError> {
        let mut result = Vec::with_capacity(statements.len());

        for stmt in statements {
            match stmt {
                Statement::FrameworkBlock {
                    name,
                    content,
                    attributes,
                    location,
                } => {
                    let block = FrameworkBlock {
                        name: name.clone(),
                        content,
                        attributes,
                        location: location.clone(),
                    };

                    if self.registry.handles(&name) {
                        // Use expand_full to preserve start function
                        let expansion = self.registry.expand_full(&block)?;
                        self.blocks_expanded += 1;
                        self.statements_generated += expansion.statements.len();

                        // Validate that the expanded code only calls bridge functions
                        // that the plugin declared in its [bridge] section.
                        if let Some(plugin_name) = self
                            .registry
                            .get_handler(&name)
                            .map(|h| h.name().to_string())
                        {
                            let violations = validate_plugin_permissions(
                                self.registry,
                                &plugin_name,
                                &expansion.statements,
                            );
                            if !violations.is_empty() {
                                tracing::warn!(
                                    plugin = %plugin_name,
                                    block = %name,
                                    violation_count = violations.len(),
                                    "Plugin permission violations detected in expanded code"
                                );
                                self.permission_errors.extend(violations);
                            }
                        }

                        // Capture start function if plugin generated one
                        if let Some(start_fn) = expansion.start_function {
                            if self.pending_start.is_none() {
                                self.pending_start = Some(start_fn);
                            } else {
                                // Merge start functions
                                if let Some(ref mut existing) = self.pending_start {
                                    existing.body.extend(start_fn.body);
                                }
                            }
                        }

                        // Capture additional functions
                        self.pending_functions.extend(expansion.functions);

                        // Capture class definitions (e.g., ORM models from frame.data)
                        self.pending_classes.extend(expansion.classes);

                        // Capture external functions
                        self.pending_externals.extend(expansion.externals);

                        // Capture top-level state: block. Required for the
                        // frame.ui v2.12.3 hydration pattern, where each
                        // `component:` block contributes one
                        // `instance_<tag>` declaration that the `client_init`
                        // splice later dispatches through.
                        merge_state_block(&mut self.pending_state, expansion.state);

                        // Add expanded statements
                        let expanded = self.expand_statements_full(expansion.statements)?;
                        result.extend(expanded);
                    } else {
                        // No plugin registered for this block. Nothing downstream
                        // handles a pass-through FrameworkBlock — resolver, type
                        // checker, HIR, MIR, and codegen all skip it — so it would
                        // vanish silently and the program would compile with a
                        // missing feature. Fail here with a clear error so the
                        // author knows to add the owning plugin to `plugins:`.
                        return Err(PluginError::UnknownBlockType {
                            block_name: name,
                            location,
                        });
                    }
                }
                other => {
                    // Use regular expansion for non-framework statements
                    result.extend(self.expand_statements(vec![other])?);
                }
            }
        }

        Ok(result)
    }

    /// Expand framework blocks in a list of statements
    fn expand_statements(
        &mut self,
        statements: Vec<Statement>,
    ) -> Result<Vec<Statement>, PluginError> {
        let mut result = Vec::with_capacity(statements.len());

        for stmt in statements {
            match stmt {
                Statement::FrameworkBlock {
                    name,
                    content,
                    attributes,
                    location,
                } => {
                    // Build the framework block struct
                    let block = FrameworkBlock {
                        name: name.clone(),
                        content,
                        attributes,
                        location: location.clone(),
                    };

                    // Check if we have a handler
                    if self.registry.handles(&name) {
                        let expanded = self.registry.expand(&block)?;
                        self.blocks_expanded += 1;
                        self.statements_generated += expanded.len();

                        tracing::trace!(
                            block_name = %name,
                            expanded_count = expanded.len(),
                            "Expanded framework block"
                        );

                        // Recursively expand any nested framework blocks
                        let expanded = self.expand_statements(expanded)?;
                        result.extend(expanded);
                    } else {
                        // See matching branch in expand_statements_full: unhandled
                        // FrameworkBlocks are invisible to every stage after the
                        // expander, so pass-through means silent feature drop.
                        return Err(PluginError::UnknownBlockType {
                            block_name: name,
                            location,
                        });
                    }
                }

                // Recursively expand nested statements
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                    location,
                } => {
                    let expanded_then = self.expand_statements(then_branch)?;
                    let expanded_else = else_branch
                        .map(|stmts| self.expand_statements(stmts))
                        .transpose()?;
                    result.push(Statement::If {
                        condition,
                        then_branch: expanded_then,
                        else_branch: expanded_else,
                        location,
                    });
                }

                Statement::Iterate {
                    iterator,
                    collection,
                    body,
                    location,
                } => {
                    let expanded_body = self.expand_statements(body)?;
                    result.push(Statement::Iterate {
                        iterator,
                        collection,
                        body: expanded_body,
                        location,
                    });
                }

                Statement::RangeIterate {
                    iterator,
                    start,
                    end,
                    step,
                    inclusive,
                    body,
                    location,
                } => {
                    let expanded_body = self.expand_statements(body)?;
                    result.push(Statement::RangeIterate {
                        iterator,
                        start,
                        end,
                        step,
                        inclusive,
                        body: expanded_body,
                        location,
                    });
                }

                Statement::Test {
                    name,
                    body,
                    location,
                } => {
                    let expanded_body = self.expand_statements(body)?;
                    result.push(Statement::Test {
                        name,
                        body: expanded_body,
                        location,
                    });
                }

                Statement::FunctionsBlock {
                    functions,
                    location,
                } => {
                    let expanded_functions = self.expand_functions(functions)?;
                    result.push(Statement::FunctionsBlock {
                        functions: expanded_functions,
                        location,
                    });
                }

                Statement::OnErrorBlock {
                    expression,
                    error_block,
                    location,
                } => {
                    let expanded_block = self.expand_statements(error_block)?;
                    result.push(Statement::OnErrorBlock {
                        expression,
                        error_block: expanded_block,
                        location,
                    });
                }

                Statement::StandaloneErrorHandler { body, location } => {
                    let expanded_body = self.expand_statements(body)?;
                    result.push(Statement::StandaloneErrorHandler {
                        body: expanded_body,
                        location,
                    });
                }

                Statement::ClassDefinition { class, location } => {
                    let expanded_class = self.expand_class(class)?;
                    result.push(Statement::ClassDefinition {
                        class: expanded_class,
                        location,
                    });
                }

                // Variable declaration whose initializer is an ORM query block expression.
                //
                // `list<User> rows = User.find:\n\tjoin: ...\n\twhere: ...`
                //
                // Convert this to a FrameworkBlock so the frame.data plugin can expand it.
                // The block content carries the variable binding information as a header line
                // (`type name =`) so the plugin can emit the correct assignment statement.
                Statement::VariableDecl {
                    ref name,
                    ref type_,
                    initializer:
                        Some(crate::ast::Expression::OrmQuery {
                            ref model,
                            ref verb,
                            ref content,
                            ref location,
                        }),
                    ..
                } => {
                    let block_name = format!("{}.{}", model, verb);
                    // Encode the variable binding as the first line of block content so
                    // the plugin knows the LHS name and type.
                    let type_str = format!("{}", type_);
                    let block_content = format!("{} {} =\n{}", type_str, name, content);
                    let block = FrameworkBlock {
                        name: block_name.clone(),
                        content: block_content,
                        attributes: Vec::new(),
                        location: Some(location.clone()),
                    };

                    if self.registry.handles_as_expression(&block_name) {
                        // Use expand_full so start_function.body statements
                        // emitted by v3 typed plugins (frame.data et al.) via
                        // `_emit_statement_into_start` land at the ORM block's
                        // splice point. Without this, per-verb expanders that
                        // emit an assignment statement (`n = __Model_count(...)`)
                        // silently discard those statements — see post-plan
                        // follow-up 2026-07-04 §8.
                        let expansion = self.registry.expand_full(&block)?;
                        let mut expanded = expansion.statements;
                        if let Some(sf) = expansion.start_function {
                            expanded.extend(sf.body);
                        }
                        self.pending_functions.extend(expansion.functions);
                        self.pending_classes.extend(expansion.classes);
                        self.pending_externals.extend(expansion.externals);
                        merge_state_block(&mut self.pending_state, expansion.state);
                        self.blocks_expanded += 1;
                        self.statements_generated += expanded.len();

                        tracing::trace!(
                            block_name = %block_name,
                            expanded_count = expanded.len(),
                            "Expanded ORM query expression"
                        );

                        // Recursively expand any nested framework blocks
                        let expanded = self.expand_statements(expanded)?;
                        result.extend(expanded);
                    } else {
                        // No plugin registered for this expression pattern — keep the
                        // statement as-is.  Semantic analysis will emit a helpful error.
                        result.push(stmt);
                    }
                }

                // Return statement whose value is an ORM query block expression.
                //
                // `return Model.<verb>:` for expression-returning verbs (count, exists,
                // first, find, findOrFail, insert_id, paginate, cursor). The expander
                // dispatches the block to its plugin and re-wraps the resulting bare
                // expression as `Return { value: Some(expr) }`. Without this arm the
                // OrmQuery survives to HIR and trips SEM001 even though the verb is
                // registered (typed-binding form works because the VariableDecl arm
                // above handles it).
                Statement::Return {
                    value:
                        Some(crate::ast::Expression::OrmQuery {
                            ref model,
                            ref verb,
                            ref content,
                            ref location,
                        }),
                    location: ref return_location,
                } => {
                    let block_name = format!("{}.{}", model, verb);
                    // No binding header — plugin emits the bare expression.
                    let block = FrameworkBlock {
                        name: block_name.clone(),
                        content: content.clone(),
                        attributes: Vec::new(),
                        location: Some(location.clone()),
                    };

                    if self.registry.handles_as_expression(&block_name) {
                        // expand_full so start_function.body statements land at
                        // the splice point (see VariableDecl+OrmQuery arm above).
                        let expansion = self.registry.expand_full(&block)?;
                        let mut expanded = expansion.statements;
                        if let Some(sf) = expansion.start_function {
                            expanded.extend(sf.body);
                        }
                        self.pending_functions.extend(expansion.functions);
                        self.pending_classes.extend(expansion.classes);
                        self.pending_externals.extend(expansion.externals);
                        merge_state_block(&mut self.pending_state, expansion.state);
                        self.blocks_expanded += 1;
                        self.statements_generated += expanded.len();

                        tracing::trace!(
                            block_name = %block_name,
                            expanded_count = expanded.len(),
                            "Expanded return-form ORM query expression"
                        );

                        // Expression-returning verbs produce a single bare expression
                        // (e.g. `__Model_count(...)`) which parses as one
                        // `Statement::Expression`. Re-wrap that as a Return; recursively
                        // expand first in case the expansion contains nested blocks.
                        let mut expanded = self.expand_statements(expanded)?;
                        if expanded.len() == 1 {
                            if let Statement::Expression { expr, .. } = expanded.remove(0) {
                                result.push(Statement::Return {
                                    value: Some(expr),
                                    location: return_location.clone(),
                                });
                                continue;
                            }
                        }
                        // Plugin returned a non-expression or multi-statement shape —
                        // not legal as the operand of `return`. Emit the statements
                        // followed by a bare `return` so codegen still terminates the
                        // function. A type error will surface for non-void functions.
                        result.extend(expanded);
                        result.push(Statement::Return {
                            value: None,
                            location: return_location.clone(),
                        });
                    } else {
                        // No plugin claims this verb — leave intact so HIR emits SEM001.
                        result.push(stmt);
                    }
                }

                // Expression statement whose expression is a standalone ORM query block.
                //
                // `User.find:\n\twhere: ...` — used as a statement (result discarded).
                Statement::Expression {
                    expr:
                        crate::ast::Expression::OrmQuery {
                            ref model,
                            ref verb,
                            ref content,
                            ref location,
                        },
                    ..
                } => {
                    let block_name = format!("{}.{}", model, verb);
                    let block = FrameworkBlock {
                        name: block_name.clone(),
                        content: content.clone(),
                        attributes: Vec::new(),
                        location: Some(location.clone()),
                    };

                    if self.registry.handles_as_expression(&block_name) {
                        // expand_full so start_function.body statements land at
                        // the splice point (see VariableDecl+OrmQuery arm above).
                        let expansion = self.registry.expand_full(&block)?;
                        let mut expanded = expansion.statements;
                        if let Some(sf) = expansion.start_function {
                            expanded.extend(sf.body);
                        }
                        self.pending_functions.extend(expansion.functions);
                        self.pending_classes.extend(expansion.classes);
                        self.pending_externals.extend(expansion.externals);
                        merge_state_block(&mut self.pending_state, expansion.state);
                        self.blocks_expanded += 1;
                        self.statements_generated += expanded.len();

                        let expanded = self.expand_statements(expanded)?;
                        result.extend(expanded);
                    } else {
                        result.push(stmt);
                    }
                }

                // Pass through statements that don't contain nested statements
                other => result.push(other),
            }
        }

        Ok(result)
    }

    /// Expand framework blocks in functions
    fn expand_functions(&mut self, functions: Vec<Function>) -> Result<Vec<Function>, PluginError> {
        functions
            .into_iter()
            .map(|mut func| {
                func.body = self.expand_statements(func.body)?;
                Ok(func)
            })
            .collect()
    }

    /// Expand framework blocks in classes
    fn expand_classes(&mut self, classes: Vec<Class>) -> Result<Vec<Class>, PluginError> {
        classes
            .into_iter()
            .map(|class| self.expand_class(class))
            .collect()
    }

    /// Expand framework blocks in a single class
    fn expand_class(&mut self, mut class: Class) -> Result<Class, PluginError> {
        // Expand methods
        class.methods = self.expand_functions(class.methods)?;

        // Expand constructor body if present
        if let Some(constructor) = class.constructor.take() {
            let expanded_body = self.expand_statements(constructor.body)?;
            class.constructor = Some(crate::ast::Constructor {
                parameters: constructor.parameters,
                body: expanded_body,
                location: constructor.location,
            });
        }

        Ok(class)
    }

    /// Get the number of blocks expanded
    pub fn blocks_expanded(&self) -> usize {
        self.blocks_expanded
    }

    /// Get the number of statements generated
    pub fn statements_generated(&self) -> usize {
        self.statements_generated
    }

    /// Take ownership of any permission violation errors collected during expansion.
    ///
    /// Callers should check this after `expand_program` returns and incorporate
    /// the errors into the compilation diagnostics.  Errors are non-fatal with
    /// respect to expansion itself (expansion still succeeds), but the caller
    /// can choose to fail compilation on any violation.
    pub fn take_permission_errors(&mut self) -> Vec<CompilerError> {
        std::mem::take(&mut self.permission_errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expression, Value};
    use crate::plugins::{FrameworkPlugin, PluginExpansion, PluginResult};
    use std::sync::Arc;

    struct EchoPlugin;

    impl FrameworkPlugin for EchoPlugin {
        fn name(&self) -> &'static str {
            "test.echo"
        }

        fn handles(&self) -> &'static [&'static str] {
            &["echo"]
        }

        fn expand(&self, block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
            // Generate a print statement with the block content
            Ok(vec![Statement::Print {
                expression: Expression::Literal(Value::String(block.content.clone())),
                newline: true,
                location: block.location.clone(),
            }])
        }
    }

    fn make_test_program(statements: Vec<Statement>) -> Program {
        Program {
            statements,
            functions: vec![],
            classes: vec![],
            imports: vec![],
            plugins: vec![],
            start_function: None,
            tests: vec![],
            screens: vec![],
            state: None,
            watch_blocks: Vec::new(),
            screen_blocks: Vec::new(),
            externals: Vec::new(),
            source_block: None,
            location: None,
        }
    }

    #[test]
    fn test_expand_empty_program() {
        let registry = PluginRegistry::new();
        let program = make_test_program(vec![]);

        let mut expander = PluginExpander::new(&registry);
        let result = expander.expand_program(program);
        assert!(result.is_ok());
    }

    #[test]
    fn test_expand_framework_block() {
        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(EchoPlugin)).unwrap();

        let program = make_test_program(vec![Statement::FrameworkBlock {
            name: "echo".to_string(),
            content: "Hello, World!".to_string(),
            attributes: vec![],
            location: None,
        }]);

        let mut expander = PluginExpander::new(&registry);
        let result = expander.expand_program(program).unwrap();

        assert_eq!(result.statements.len(), 1);
        assert!(matches!(result.statements[0], Statement::Print { .. }));
    }

    #[test]
    fn test_unknown_block_is_hard_error() {
        // COMPILER-IMPLICIT-PLUGIN-ACTIVATION (dashboard fp 73cfeba24d14).
        // A FrameworkBlock whose owning plugin isn't registered used to pass
        // through the expander with a comment claiming "will fail later in
        // semantic analysis" — but no downstream stage handled it, so it
        // vanished silently. Now the expander itself fails fast with
        // UnknownBlockType, forcing the author to either add the plugin to
        // `plugins:` or remove the block.
        let registry = PluginRegistry::new();

        let program = make_test_program(vec![Statement::FrameworkBlock {
            name: "unknown".to_string(),
            content: "content".to_string(),
            attributes: vec![],
            location: None,
        }]);

        let mut expander = PluginExpander::new(&registry);
        let result = expander.expand_program(program);
        assert!(matches!(
            result,
            Err(PluginError::UnknownBlockType { ref block_name, .. }) if block_name == "unknown"
        ));
    }

    // ========================================================================
    // Plugin Contracts v2 — lifecycle slot dispatch (§2)
    // foundation/spec/plugins/contracts/lifecycle.md
    // ========================================================================

    /// Mock plugin that injects a deterministic Print statement for the
    /// `program_init` and `server_init` lifecycle slots. Lets us verify the
    /// expander wires the slot output into the program's start function.
    ///
    /// `handles` returns a non-empty block list because the registry's plugin
    /// dispatch table is keyed by block name. Production plugins (frame.server,
    /// frame.ui, etc.) always declare at least one block, so this matches the
    /// real-world topology.
    struct LifecycleMockPlugin {
        marker: &'static str,
        slots: Vec<&'static str>,
    }

    impl FrameworkPlugin for LifecycleMockPlugin {
        fn name(&self) -> &'static str {
            "test.lifecycle"
        }

        fn handles(&self) -> &'static [&'static str] {
            // Synthetic block; never expanded in these tests.
            &["__lifecycle_mock_block"]
        }

        fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
            Ok(Vec::new())
        }

        fn invoke_lifecycle_slot(
            &self,
            slot_name: &str,
            _context: &crate::plugins::BuildContext,
        ) -> PluginResult<PluginExpansion> {
            if !self.slots.contains(&slot_name) {
                return Ok(PluginExpansion::default());
            }
            Ok(PluginExpansion {
                statements: vec![Statement::Print {
                    expression: Expression::Literal(Value::String(format!(
                        "{}:{}",
                        self.marker, slot_name
                    ))),
                    newline: true,
                    location: None,
                }],
                ..PluginExpansion::default()
            })
        }
    }

    fn lifecycle_manifest(
        plugin_name: &str,
        slots: &[(&str, &str)],
    ) -> crate::plugins::PluginManifest {
        use crate::plugins::plugin_abi::{
            PluginCompatibility, PluginExports, PluginHandles, PluginInfo, PluginLanguage,
            PluginLifecycle, PluginManifest,
        };
        let mut lifecycle = PluginLifecycle::default();
        for (slot, export) in slots {
            match *slot {
                "module_helpers" => lifecycle.module_helpers = Some((*export).to_string()),
                "program_init" => lifecycle.program_init = Some((*export).to_string()),
                "client_init" => lifecycle.client_init = Some((*export).to_string()),
                "server_init" => lifecycle.server_init = Some((*export).to_string()),
                "per_request" => lifecycle.per_request = Some((*export).to_string()),
                "artifact_emitters" => lifecycle.artifact_emitters = Some((*export).to_string()),
                _ => {}
            }
        }
        PluginManifest {
            plugin: PluginInfo {
                name: plugin_name.to_string(),
                version: "1.0.0".to_string(),
                description: String::new(),
                author: String::new(),
            },
            compatibility: PluginCompatibility::default(),
            handles: PluginHandles {
                blocks: Vec::new(),
                expressions: Vec::new(),
            },
            exports: PluginExports::default(),
            bridge: Default::default(),
            language: PluginLanguage::default(),
            ai: Default::default(),
            paths: Default::default(),
            enforcement: Default::default(),
            memory: Default::default(),
            build: Default::default(),
            lifecycle,
            artifacts: Vec::new(),
            blocks: Default::default(),
        }
    }

    #[test]
    fn test_lifecycle_program_init_is_spliced_into_start() {
        let registry = PluginRegistry::builder()
            .add(LifecycleMockPlugin {
                marker: "P",
                slots: vec!["program_init"],
            })
            .add_manifest(
                "test.lifecycle".to_string(),
                lifecycle_manifest("test.lifecycle", &[("program_init", "emit_program_init")]),
            )
            .build()
            .expect("registry build should succeed");

        let mut expander = PluginExpander::new(&registry);
        let result = expander
            .expand_program(make_test_program(Vec::new()))
            .expect("expand_program should succeed");

        let start = result
            .start_function
            .expect("program_init must synthesize a start function when none exists");
        assert_eq!(
            start.body.len(),
            1,
            "program_init slot output should appear once in start body"
        );
        match &start.body[0] {
            Statement::Print { expression, .. } => match expression {
                Expression::Literal(Value::String(s)) => {
                    assert_eq!(s, "P:program_init");
                }
                _ => panic!("expected string literal print"),
            },
            other => panic!("expected Print statement, got {:?}", other),
        }
    }

    #[test]
    fn test_lifecycle_server_init_only_runs_on_server_target() {
        // Plugin declares both program_init and server_init.
        let registry = PluginRegistry::builder()
            .add(LifecycleMockPlugin {
                marker: "X",
                slots: vec!["program_init", "server_init"],
            })
            .add_manifest(
                "test.lifecycle".to_string(),
                lifecycle_manifest(
                    "test.lifecycle",
                    &[
                        ("program_init", "emit_program_init"),
                        ("server_init", "emit_server_init"),
                    ],
                ),
            )
            .build()
            .expect("registry build should succeed");

        // Server build receives both program_init and server_init.
        let mut expander = PluginExpander::new(&registry);
        let server_result = expander
            .expand_program(make_test_program(Vec::new()))
            .expect("server expand should succeed");
        let server_start = server_result
            .start_function
            .expect("server build should have a start function");
        assert_eq!(server_start.body.len(), 2);

        // Client build skips server_init.
        let registry2 = PluginRegistry::builder()
            .add(LifecycleMockPlugin {
                marker: "X",
                slots: vec!["program_init", "server_init"],
            })
            .add_manifest(
                "test.lifecycle".to_string(),
                lifecycle_manifest(
                    "test.lifecycle",
                    &[
                        ("program_init", "emit_program_init"),
                        ("server_init", "emit_server_init"),
                    ],
                ),
            )
            .build()
            .unwrap();
        let mut client_expander = PluginExpander::new(&registry2).with_build_target(false, true);
        let client_result = client_expander
            .expand_program(make_test_program(Vec::new()))
            .expect("client expand should succeed");
        let client_start = client_result
            .start_function
            .expect("client build should have a start function");
        assert_eq!(
            client_start.body.len(),
            1,
            "client build should receive only program_init, not server_init"
        );
        match &client_start.body[0] {
            Statement::Print { expression, .. } => match expression {
                Expression::Literal(Value::String(s)) => assert_eq!(s, "X:program_init"),
                _ => panic!("expected string literal"),
            },
            other => panic!("expected Print, got {:?}", other),
        }
    }

    #[test]
    fn test_lifecycle_slots_run_before_user_start() {
        use crate::ast::FunctionSyntax;
        let registry = PluginRegistry::builder()
            .add(LifecycleMockPlugin {
                marker: "L",
                slots: vec!["program_init"],
            })
            .add_manifest(
                "test.lifecycle".to_string(),
                lifecycle_manifest("test.lifecycle", &[("program_init", "emit_program_init")]),
            )
            .build()
            .unwrap();

        let mut user_program = make_test_program(Vec::new());
        user_program.start_function = Some(Function {
            name: "start".to_string(),
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters: Vec::new(),
            return_type: crate::ast::Type::Void,
            body: vec![Statement::Print {
                expression: Expression::Literal(Value::String("user".to_string())),
                newline: true,
                location: None,
            }],
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: crate::ast::Visibility::Public,
            modifier: crate::ast::FunctionModifier::None,
            location: None,
        });

        let mut expander = PluginExpander::new(&registry);
        let result = expander.expand_program(user_program).unwrap();
        let start = result.start_function.unwrap();
        assert_eq!(
            start.body.len(),
            2,
            "plugin lifecycle output should be prepended, not replace, user start body"
        );
        // First the plugin's L:program_init, then the user's "user".
        match (&start.body[0], &start.body[1]) {
            (
                Statement::Print {
                    expression: Expression::Literal(Value::String(s0)),
                    ..
                },
                Statement::Print {
                    expression: Expression::Literal(Value::String(s1)),
                    ..
                },
            ) => {
                assert_eq!(s0, "L:program_init");
                assert_eq!(s1, "user");
            }
            other => panic!("expected two prints, got {:?}", other),
        }
    }

    #[test]
    fn test_lifecycle_client_init_dispatched_only_on_client_build() {
        // Plugin declares both program_init and client_init.
        let server_registry = PluginRegistry::builder()
            .add(LifecycleMockPlugin {
                marker: "Y",
                slots: vec!["program_init", "client_init"],
            })
            .add_manifest(
                "test.lifecycle".to_string(),
                lifecycle_manifest(
                    "test.lifecycle",
                    &[
                        ("program_init", "emit_program_init"),
                        ("client_init", "emit_client_init"),
                    ],
                ),
            )
            .build()
            .unwrap();

        // Server build: program_init runs, client_init does not.
        let mut server_expander = PluginExpander::new(&server_registry);
        let server_result = server_expander
            .expand_program(make_test_program(Vec::new()))
            .unwrap();
        let server_body = server_result.start_function.unwrap().body;
        assert_eq!(server_body.len(), 1);
        match &server_body[0] {
            Statement::Print { expression, .. } => match expression {
                Expression::Literal(Value::String(s)) => assert_eq!(s, "Y:program_init"),
                _ => panic!("expected string literal"),
            },
            other => panic!("expected Print, got {:?}", other),
        }

        // Client build: program_init AND client_init both run.
        let client_registry = PluginRegistry::builder()
            .add(LifecycleMockPlugin {
                marker: "Y",
                slots: vec!["program_init", "client_init"],
            })
            .add_manifest(
                "test.lifecycle".to_string(),
                lifecycle_manifest(
                    "test.lifecycle",
                    &[
                        ("program_init", "emit_program_init"),
                        ("client_init", "emit_client_init"),
                    ],
                ),
            )
            .build()
            .unwrap();
        let mut client_expander =
            PluginExpander::new(&client_registry).with_build_target(false, true);
        let client_result = client_expander
            .expand_program(make_test_program(Vec::new()))
            .unwrap();
        let client_body = client_result.start_function.unwrap().body;
        assert_eq!(
            client_body.len(),
            2,
            "client build should receive program_init AND client_init"
        );
        // Order matches dispatch order: program_init first, then client_init.
        let extract_marker = |stmt: &Statement| -> String {
            match stmt {
                Statement::Print {
                    expression: Expression::Literal(Value::String(s)),
                    ..
                } => s.clone(),
                _ => String::new(),
            }
        };
        assert_eq!(extract_marker(&client_body[0]), "Y:program_init");
        assert_eq!(extract_marker(&client_body[1]), "Y:client_init");
    }

    #[test]
    fn test_lifecycle_plugins_without_slot_contribute_nothing() {
        let registry = PluginRegistry::builder()
            .add(LifecycleMockPlugin {
                marker: "L",
                slots: vec!["client_init"], // declared but not invoked in server build
            })
            .add_manifest(
                "test.lifecycle".to_string(),
                lifecycle_manifest("test.lifecycle", &[("client_init", "emit_client_init")]),
            )
            .build()
            .unwrap();

        let mut expander = PluginExpander::new(&registry);
        let result = expander
            .expand_program(make_test_program(Vec::new()))
            .unwrap();
        // Server build: client_init isn't dispatched yet (§1 follow-up), and
        // program_init wasn't declared. No start_function should appear.
        assert!(result.start_function.is_none());
    }

    // -----------------------------------------------------------------------
    // PluginExpansion.state propagation (HYDRATE_AUTO Gap 1 closure)
    // -----------------------------------------------------------------------
    //
    // frame.ui v2.12.3's `expand_component` emits a top-level `state:` block
    // declaring an instance global (`MyToolbar instance_my_toolbar =
    // MyToolbar()`) that the `client_init` splice later references. Before
    // PR A landed, the parser would parse the state block but
    // `PluginExpansion` had no state field, so it was silently dropped on
    // the way to `program.state`. The splice then failed with
    // `Undefined variable 'instance_my_toolbar'`. These tests cover the
    // propagation + merge path that closes that gap.

    use crate::ast::{StateBlock, StateDeclaration, StateScope, Type};

    /// Mock plugin that emits a state: block on `expand_full`. Mirrors what
    /// frame.ui v2.12.3's `expand_component` returns: a single instance
    /// declaration with a deterministic identifier so tests can grep for it.
    /// `handles()` always returns `["state_emitter"]`; tests that need two
    /// distinct plugins register two instances and rely on the registry
    /// rejecting the duplicate (so they call `merge_state_block` directly
    /// instead — see `multiple_plugin_state_blocks_accumulate`).
    struct StateEmittingPlugin {
        var_name: &'static str,
    }

    impl FrameworkPlugin for StateEmittingPlugin {
        fn name(&self) -> &'static str {
            "test.state"
        }

        fn handles(&self) -> &'static [&'static str] {
            &["state_emitter"]
        }

        fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
            Ok(Vec::new())
        }

        fn expand_full(&self, _block: &FrameworkBlock) -> PluginResult<PluginExpansion> {
            // Single declaration: `<block> <var_name> = "ok"`.  Using String
            // instead of a class type avoids hauling in a Class fixture; the
            // merge plumbing is type-agnostic.
            let decl = StateDeclaration {
                name: self.var_name.to_string(),
                type_: Type::String,
                initializer: Expression::Literal(Value::String("ok".to_string())),
                guard: None,
                is_private: false,
                location: None,
            };
            Ok(PluginExpansion {
                state: Some(StateBlock {
                    declarations: vec![decl],
                    computed: Vec::new(),
                    rules: None,
                    scope: StateScope::App,
                    location: None,
                }),
                ..PluginExpansion::default()
            })
        }
    }

    fn user_state_with(decl_name: &str) -> StateBlock {
        StateBlock {
            declarations: vec![StateDeclaration {
                name: decl_name.to_string(),
                type_: Type::String,
                initializer: Expression::Literal(Value::String("user".to_string())),
                guard: None,
                is_private: false,
                location: None,
            }],
            computed: Vec::new(),
            rules: None,
            scope: StateScope::App,
            location: None,
        }
    }

    /// Single plugin block contributing a state: declaration: the merged
    /// program.state must contain the plugin's declaration. The baseline
    /// regression — silently-dropped state — would leave program.state None.
    #[test]
    fn plugin_state_block_reaches_program_state() {
        let mut registry = PluginRegistry::new();
        registry
            .register(Arc::new(StateEmittingPlugin {
                var_name: "instance_my_toolbar",
            }))
            .unwrap();

        let program = make_test_program(vec![Statement::FrameworkBlock {
            name: "state_emitter".to_string(),
            content: String::new(),
            attributes: Vec::new(),
            location: None,
        }]);

        let mut expander = PluginExpander::new(&registry);
        let result = expander.expand_program(program).unwrap();

        let state = result
            .state
            .expect("plugin-emitted state: block must reach program.state");
        assert_eq!(state.declarations.len(), 1);
        assert_eq!(state.declarations[0].name, "instance_my_toolbar");
    }

    /// User and plugin both contribute state. Plugin declarations land first
    /// (so user start: code can reference plugin-emitted globals), user
    /// declarations follow. Order matters because semantic analysis runs
    /// initializers left-to-right.
    #[test]
    fn plugin_state_merges_with_user_state_plugin_first() {
        let mut registry = PluginRegistry::new();
        registry
            .register(Arc::new(StateEmittingPlugin {
                var_name: "plugin_var",
            }))
            .unwrap();

        let mut program = make_test_program(vec![Statement::FrameworkBlock {
            name: "state_emitter".to_string(),
            content: String::new(),
            attributes: Vec::new(),
            location: None,
        }]);
        program.state = Some(user_state_with("user_var"));

        let mut expander = PluginExpander::new(&registry);
        let result = expander.expand_program(program).unwrap();

        let state = result.state.expect("merged state must survive");
        let names: Vec<&str> = state.declarations.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["plugin_var", "user_var"],
            "plugin-contributed declarations must appear before user declarations"
        );
    }

    /// Multiple plugin blocks each contributing one state: declaration must
    /// all land in program.state. The frame.ui hydration flow emits one
    /// `instance_<tag>` per component block in the source file; if only the
    /// first block's state survived, multi-component pages would silently
    /// lose all but the first instance global.
    ///
    /// `StateEmittingPlugin::handles()` returns a single static block name,
    /// so registering two instances with different `var_name`s would collide
    /// at the registry. The accumulation semantics themselves are what this
    /// asserts, so we drive `merge_state_block` directly — that's the same
    /// function `expand_statements_full` calls per block.
    #[test]
    fn multiple_plugin_state_blocks_accumulate() {
        let plugin_block_a = StateBlock {
            declarations: vec![StateDeclaration {
                name: "first_var".to_string(),
                type_: Type::String,
                initializer: Expression::Literal(Value::String("a".to_string())),
                guard: None,
                is_private: false,
                location: None,
            }],
            computed: Vec::new(),
            rules: None,
            scope: StateScope::App,
            location: None,
        };
        let plugin_block_b = StateBlock {
            declarations: vec![StateDeclaration {
                name: "second_var".to_string(),
                type_: Type::String,
                initializer: Expression::Literal(Value::String("b".to_string())),
                guard: None,
                is_private: false,
                location: None,
            }],
            computed: Vec::new(),
            rules: None,
            scope: StateScope::App,
            location: None,
        };

        let mut accumulator: Option<StateBlock> = None;
        merge_state_block(&mut accumulator, Some(plugin_block_a));
        merge_state_block(&mut accumulator, Some(plugin_block_b));

        let merged = accumulator.expect("merge of two state blocks must produce a result");
        let names: Vec<&str> = merged
            .declarations
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert_eq!(names, vec!["first_var", "second_var"]);
    }

    /// merge_state_block with no incoming leaves target untouched.
    #[test]
    fn merge_state_block_no_op_for_none_incoming() {
        let original = user_state_with("keep_me");
        let mut target = Some(original.clone());
        merge_state_block(&mut target, None);
        assert_eq!(target.as_ref().unwrap().declarations[0].name, "keep_me");
    }

    /// merge_state_block with target None and incoming Some sets target.
    #[test]
    fn merge_state_block_adopts_incoming_when_target_empty() {
        let incoming = user_state_with("from_incoming");
        let mut target: Option<StateBlock> = None;
        merge_state_block(&mut target, Some(incoming));
        assert!(target.is_some());
        assert_eq!(target.unwrap().declarations[0].name, "from_incoming");
    }
}
