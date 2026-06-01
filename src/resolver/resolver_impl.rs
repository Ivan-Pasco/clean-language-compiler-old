//! Name Resolver Implementation
//!
//! This module implements the name resolution algorithm that transforms HIR
//! into Resolved HIR by resolving all symbol references.

use super::*;
use crate::error::CompilerError;

/// Name resolver that transforms HIR to Resolved HIR
pub struct NameResolver {
    symbol_table: GlobalSymbolTable,
    current_class: Option<SymbolId>,
    current_function: Option<SymbolId>,
    /// Return type of the function currently being resolved. Used to inject
    /// the synthetic `result` variable when resolving `ensure` postconditions.
    current_function_return_type: Option<HirType>,
    /// Name of the screen currently being resolved (for SCOPE005 enforcement).
    /// `None` when resolving code that is not inside a screen block.
    current_screen: Option<String>,
    errors: Vec<CompilerError>,
    warnings: Vec<CompilerError>,
    expression_recursion_depth: usize,
    /// Plugin bridge functions deferred until after `register_top_level_symbols` runs,
    /// so they are applied AFTER builtins and are never overwritten by them.
    pending_bridge_functions: Vec<crate::plugins::BridgeFunction>,
    /// Language-name aliases deferred alongside `pending_bridge_functions`.
    pending_language_aliases: std::collections::HashMap<String, String>,
    /// Optional language-function definitions (from plugin.toml `[language].functions`)
    /// used to override bridge return types and param lists for language aliases.
    pending_language_fn_defs:
        std::collections::HashMap<String, crate::plugins::plugin_abi::PluginFunctionDef>,
    /// Default argument values for language alias functions that have `param_defaults`.
    /// Keyed by language function name. Values are per-param default strings (empty = required).
    language_fn_defaults: std::collections::HashMap<String, Vec<String>>,
}

impl NameResolver {
    /// Create a new name resolver
    pub fn new() -> Self {
        Self {
            symbol_table: GlobalSymbolTable::new(),
            current_class: None,
            current_function: None,
            current_function_return_type: None,
            current_screen: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            expression_recursion_depth: 0,
            pending_bridge_functions: Vec::new(),
            pending_language_aliases: std::collections::HashMap::new(),
            pending_language_fn_defs: std::collections::HashMap::new(),
            language_fn_defaults: std::collections::HashMap::new(),
        }
    }

    /// Resolve names in a HIR program
    pub fn resolve(hir: HirProgram) -> Result<ResolutionResult, Vec<CompilerError>> {
        let mut resolver = Self::new();
        match resolver.resolve_program(hir) {
            Ok(resolved_hir) => Ok(ResolutionResult {
                resolved_hir,
                warnings: resolver.warnings,
            }),
            Err(_) => Err(resolver.errors),
        }
    }

    /// Resolve the entire HIR program
    fn resolve_program(&mut self, hir: HirProgram) -> Result<ResolvedHirProgram, ()> {
        // First pass: Register all top-level symbols (includes builtins)
        self.register_top_level_symbols(&hir)?;

        // Apply plugin bridge registrations AFTER builtins so plugin signatures
        // are never overwritten by hardcoded builtin entries.
        let pending_bridges = std::mem::take(&mut self.pending_bridge_functions);
        let pending_aliases = std::mem::take(&mut self.pending_language_aliases);
        self.register_plugin_bridge_functions(&pending_bridges);
        if !pending_aliases.is_empty() {
            let bridge_by_name: std::collections::HashMap<&str, &crate::plugins::BridgeFunction> =
                pending_bridges
                    .iter()
                    .map(|bf| (bf.name.as_str(), bf))
                    .collect();
            self.register_language_function_aliases(&pending_aliases, &bridge_by_name);
        }

        // Second pass: Resolve all symbol references
        let resolved_functions = self.resolve_functions(&hir.functions)?;
        let resolved_classes = self.resolve_classes(&hir.classes)?;

        let resolved_start_function = if let Some(start_fn) = hir.start_function {
            Some(self.resolve_function(start_fn)?)
        } else {
            None
        };
        let resolved_imports = self.resolve_imports(&hir.imports)?;
        let resolved_tests = self.resolve_tests(&hir.tests)?;

        // Resolve state block if present
        let resolved_state = if let Some(ref state_block) = hir.state {
            Some(self.resolve_state_block(state_block)?)
        } else {
            None
        };

        // Resolve top-level watch blocks.
        // Watch blocks reference state variables that must already be registered
        // in the global scope. Each body is resolved independently.
        let mut resolved_watch_blocks: Vec<crate::resolver::ResolvedHirWatchBlock> = Vec::new();
        for watch in &hir.watch_blocks {
            // Resolve and validate each target name against the symbol table.
            let mut target_symbol_ids: Vec<SymbolId> = Vec::new();
            let mut target_resolution_failed = false;
            for target_name in &watch.targets {
                match self.symbol_table.lookup_symbol(target_name) {
                    Some(sid) => target_symbol_ids.push(sid),
                    None => {
                        tracing::warn!(
                            name = %target_name,
                            "Watch target '{}' not found in symbol table — \
                             it may be defined later or come from a plugin",
                            target_name
                        );
                        // Use a sentinel SymbolId(0) so that the rest of the pipeline
                        // can still operate; the runtime will validate at execution time.
                        target_symbol_ids.push(SymbolId(0));
                        target_resolution_failed = true;
                    }
                }
            }

            // Even when target resolution had warnings, still resolve the body so that
            // type errors in the handler are caught at compile time.
            let _ = target_resolution_failed; // informational only
            let resolved_body = self.resolve_block(&watch.body)?;

            resolved_watch_blocks.push(crate::resolver::ResolvedHirWatchBlock {
                targets: watch.targets.clone(),
                target_symbol_ids,
                body: resolved_body,
                location: watch.location.clone(),
            });
        }

        // Resolve external functions (WASM imports)
        let resolved_externals = self.resolve_externals(&hir.externals)?;

        // Resolve screen blocks — each body is resolved with current_screen set so
        // that SCOPE005 access checks know they're inside the owning screen.
        for screen in &hir.screen_blocks {
            self.current_screen = Some(screen.name.clone());
            if let Some(ref screen_state) = screen.state {
                let _ = self.resolve_state_block(screen_state);
            }
            for watch in &screen.watch_blocks {
                let _ = self.resolve_block(&watch.body);
            }
            for func in &screen.functions {
                let _ = self.resolve_function(func.clone());
            }
            self.current_screen = None;
        }

        // Surface any SCOPE003 (max nesting depth exceeded) errors accumulated
        // during symbol lookups into the resolver error list.
        for msg in self.symbol_table.take_scope_depth_errors() {
            self.errors.push(CompilerError::Validation {
                context: Box::new(
                    crate::error::ErrorContext::new(
                        msg,
                        Some("Reduce block nesting depth — maximum is 50 levels.".to_string()),
                        crate::error::ErrorType::Validation,
                        None,
                    )
                    .with_error_code("SCOPE003"),
                ),
            });
        }

        Ok(ResolvedHirProgram {
            functions: resolved_functions,
            classes: resolved_classes,
            start_function: resolved_start_function,
            imports: resolved_imports,
            tests: resolved_tests,
            state: resolved_state,
            watch_blocks: resolved_watch_blocks,
            symbol_table: self.symbol_table.clone(),
            location: hir.location,
            externals: resolved_externals,
        })
    }

    /// First pass: Register all top-level symbols (functions, classes)
    fn register_top_level_symbols(&mut self, hir: &HirProgram) -> Result<(), ()> {
        // Register builtin functions so they pass validation
        self.register_builtin_functions();

        // Register functions
        for function in &hir.functions {
            // Check for duplicates BEFORE creating the symbol
            if self
                .symbol_table
                .has_symbol_in_current_scope(&function.name)
            {
                self.error(
                    &format!("Function '{}' is already defined", function.name),
                    function.location.clone(),
                );
            } else {
                let symbol_id = self.symbol_table.create_symbol(
                    function.name.clone(),
                    SymbolKind::Function {
                        parameters: function
                            .parameters
                            .iter()
                            .map(|p| p.param_type.clone())
                            .collect(),
                        return_type: function.return_type.clone(),
                    },
                    self.symbol_table.current_scope_id(),
                    function.location.clone(),
                );
                // Propagate inline private: visibility (SEM005).
                if function.is_private {
                    self.symbol_table
                        .mark_as_private(symbol_id, "<module>".to_string());
                }
            }
        }

        // Register start function
        if let Some(start_fn) = &hir.start_function {
            // Check for duplicates BEFORE creating the symbol
            if self
                .symbol_table
                .has_symbol_in_current_scope(&start_fn.name)
            {
                self.error(
                    &format!("Function '{}' conflicts with start function", start_fn.name),
                    start_fn.location.clone(),
                );
            } else {
                let _symbol_id = self.symbol_table.create_symbol(
                    start_fn.name.clone(),
                    SymbolKind::Function {
                        parameters: start_fn
                            .parameters
                            .iter()
                            .map(|p| p.param_type.clone())
                            .collect(),
                        return_type: start_fn.return_type.clone(),
                    },
                    self.symbol_table.current_scope_id(),
                    start_fn.location.clone(),
                );
            }
        }

        // Register classes
        for class in &hir.classes {
            // Check for duplicates BEFORE creating the symbol
            if self.symbol_table.has_symbol_in_current_scope(&class.name) {
                self.error(
                    &format!("Class '{}' is already defined", class.name),
                    class.location.clone(),
                );
            } else {
                let class_symbol_id = self.symbol_table.create_symbol(
                    class.name.clone(),
                    SymbolKind::Class {
                        fields: Vec::new(),  // Will be filled in second pass
                        methods: Vec::new(), // Will be filled in second pass
                        parent: None,        // Will be resolved in second pass
                    },
                    self.symbol_table.current_scope_id(),
                    class.location.clone(),
                );

                // NOTE: Register constructor symbol in first pass
                // This allows global functions to reference constructors before classes are fully resolved
                let constructor_params = if let Some(constructor) = &class.constructor {
                    constructor
                        .parameters
                        .iter()
                        .map(|p| p.param_type.clone())
                        .collect()
                } else {
                    vec![] // Default constructor has no parameters
                };

                let _constructor_symbol_id = self.symbol_table.create_symbol(
                    format!("{}.constructor", class.name),
                    SymbolKind::Constructor {
                        class_id: class_symbol_id,
                        parameters: constructor_params,
                    },
                    self.symbol_table.current_scope_id(),
                    class.location.clone(),
                );
            }
        }

        // Register state variables (global scope)
        if let Some(state_block) = &hir.state {
            for state_decl in &state_block.declarations {
                // Check for duplicates BEFORE creating the symbol
                if self
                    .symbol_table
                    .has_symbol_in_current_scope(&state_decl.name)
                {
                    self.error(
                        &format!(
                            "State variable '{}' conflicts with existing symbol",
                            state_decl.name
                        ),
                        state_decl.location.clone(),
                    );
                } else {
                    let symbol_id = self.symbol_table.create_symbol(
                        state_decl.name.clone(),
                        SymbolKind::StateVariable {
                            var_type: state_decl.state_type.clone(),
                            scope: state_block.scope,
                            has_guard: state_decl.guard.is_some(),
                            is_computed: false,
                            screen_name: None,
                        },
                        self.symbol_table.current_scope_id(),
                        state_decl.location.clone(),
                    );
                    // Propagate inline private: visibility (SEM005).
                    if state_decl.is_private {
                        self.symbol_table
                            .mark_as_private(symbol_id, "<module>".to_string());
                    }
                }
            }

            // Register computed state variables (read-only)
            for computed_decl in &state_block.computed {
                if self
                    .symbol_table
                    .has_symbol_in_current_scope(&computed_decl.name)
                {
                    self.error(
                        &format!(
                            "Computed state '{}' conflicts with existing symbol",
                            computed_decl.name
                        ),
                        computed_decl.location.clone(),
                    );
                } else {
                    let _symbol_id = self.symbol_table.create_symbol(
                        computed_decl.name.clone(),
                        SymbolKind::StateVariable {
                            var_type: computed_decl.computed_type.clone(),
                            scope: state_block.scope,
                            has_guard: false,  // Computed state doesn't have guards
                            is_computed: true, // Computed state is read-only (STATE004)
                            screen_name: None,
                        },
                        self.symbol_table.current_scope_id(),
                        computed_decl.location.clone(),
                    );
                }
            }
        }

        // Register imported modules as Namespace symbols in the global scope.
        // This must happen in the first pass so that `Module.function()` call
        // sites can be resolved regardless of where they appear in the source.
        for import in &hir.imports {
            if !self
                .symbol_table
                .has_symbol_in_current_scope(&import.module_name)
            {
                let _ns_id = self.symbol_table.create_symbol(
                    import.module_name.clone(),
                    SymbolKind::Namespace { functions: vec![] },
                    self.symbol_table.current_scope_id(),
                    import.location.clone(),
                );
            }
        }

        // Register external functions (WASM imports)
        // External functions are treated like builtins - they have no body in Clean code
        for external in &hir.externals {
            if self
                .symbol_table
                .has_symbol_in_current_scope(&external.name)
            {
                // If an external function declaration collides with a builtin (e.g. a
                // user-defined wrapper that shadows a builtin name), skip re-registration
                // rather than emitting a spurious conflict error.  Plugin bridge functions
                // are no longer hardcoded here — they are registered after builtins via
                // `pending_bridge_functions`, so there is no collision to defend against.
                if let Some(existing_symbol_id) = self.symbol_table.lookup_symbol(&external.name) {
                    if self.symbol_table.is_builtin(existing_symbol_id) {
                        tracing::debug!(
                            name = %external.name,
                            "External function matches existing builtin, skipping registration"
                        );
                        continue;
                    }
                }
                // Only error if it conflicts with a non-builtin symbol
                self.error(
                    &format!(
                        "External function '{}' conflicts with existing symbol",
                        external.name
                    ),
                    external.location.clone(),
                );
            } else {
                let symbol_id = self.symbol_table.create_symbol(
                    external.name.clone(),
                    SymbolKind::Function {
                        parameters: external
                            .parameters
                            .iter()
                            .map(|p| p.param_type.clone())
                            .collect(),
                        return_type: Some(external.return_type.clone()),
                    },
                    self.symbol_table.current_scope_id(),
                    external.location.clone(),
                );
                // Mark as builtin so it doesn't require a body
                self.symbol_table.mark_as_builtin(symbol_id);
            }
        }

        // Register screen-local state variables (SCOPE005).
        // Each screen's state variables are registered globally so that the symbol table
        // can look them up, but they carry the owning screen name so that access from
        // outside that screen can be rejected at resolution time.
        for screen in &hir.screen_blocks {
            if let Some(ref screen_state) = screen.state {
                for state_decl in &screen_state.declarations {
                    // Screen-local variables share the global symbol table but carry
                    // their screen name so SCOPE005 can be enforced during resolution.
                    if !self
                        .symbol_table
                        .has_symbol_in_current_scope(&state_decl.name)
                    {
                        self.symbol_table.create_symbol(
                            state_decl.name.clone(),
                            SymbolKind::StateVariable {
                                var_type: state_decl.state_type.clone(),
                                scope: crate::hir::HirStateScope::Screen,
                                has_guard: state_decl.guard.is_some(),
                                is_computed: false,
                                screen_name: Some(screen.name.clone()),
                            },
                            self.symbol_table.current_scope_id(),
                            state_decl.location.clone(),
                        );
                    }
                }
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(())
        }
    }

    /// Resolve all functions
    fn resolve_functions(
        &mut self,
        functions: &[HirFunction],
    ) -> Result<Vec<ResolvedHirFunction>, ()> {
        let mut resolved_functions = Vec::new();

        for function in functions {
            resolved_functions.push(self.resolve_function(function.clone())?);
        }

        Ok(resolved_functions)
    }

    /// Resolve a single function
    fn resolve_function(&mut self, function: HirFunction) -> Result<ResolvedHirFunction, ()> {
        // Find function symbol
        let function_symbol_id =
            self.symbol_table
                .lookup_symbol(&function.name)
                .ok_or_else(|| {
                    self.error(
                        &format!("Function '{}' not found in symbol table", function.name),
                        function.location.clone(),
                    );
                })?;

        // Create function scope
        let function_scope = self.symbol_table.create_scope(
            None,
            ScopeType::Function {
                function_id: function_symbol_id,
            },
        );
        self.symbol_table.enter_scope(function_scope);
        self.current_function = Some(function_symbol_id);
        let previous_return_type = self.current_function_return_type.clone();
        self.current_function_return_type = function.return_type.clone();

        // Resolve parameters
        let mut resolved_parameters = Vec::new();
        for param in &function.parameters {
            let param_symbol_id = self.symbol_table.create_symbol(
                param.name.clone(),
                SymbolKind::Parameter {
                    param_type: param.param_type.clone(),
                },
                function_scope,
                param.location.clone(),
            );

            // Resolve default value expression if present
            let default_value = if let Some(default_expr) = &param.default_value {
                Some(self.resolve_expression(default_expr)?)
            } else {
                None
            };

            resolved_parameters.push(ResolvedHirParameter {
                name: param.name.clone(),
                symbol_id: param_symbol_id,
                param_type: param.param_type.clone(),
                default_value,
                is_variadic: false, // Variadic not yet in language spec
                location: param.location.clone(),
            });
        }

        // If this function belongs to a screen (owner_screen is set), temporarily
        // set current_screen so that SCOPE005 allows access to that screen's state.
        let previous_screen = self.current_screen.clone();
        if let Some(ref screen_name) = function.owner_screen {
            self.current_screen = Some(screen_name.clone());
        }

        // Resolve function body
        let resolved_body = self.resolve_block(&function.body)?;

        // Exit function scope
        self.symbol_table.exit_scope();
        self.current_function = None;
        self.current_function_return_type = previous_return_type;
        self.current_screen = previous_screen;

        Ok(ResolvedHirFunction {
            name: function.name,
            symbol_id: function_symbol_id,
            parameters: resolved_parameters,
            return_type: function.return_type,
            body: resolved_body,
            is_start: function.is_start,
            is_background: false, // Async detection handled by runtime analysis
            location: function.location,
        })
    }

    /// Resolve all classes
    fn resolve_classes(&mut self, classes: &[HirClass]) -> Result<Vec<ResolvedHirClass>, ()> {
        let mut resolved_classes = Vec::new();

        for class in classes {
            resolved_classes.push(self.resolve_class(class.clone())?);
        }

        Ok(resolved_classes)
    }

    /// Resolve a single class
    fn resolve_class(&mut self, class: HirClass) -> Result<ResolvedHirClass, ()> {
        // Find class symbol
        let class_symbol_id = self
            .symbol_table
            .lookup_symbol(&class.name)
            .ok_or_else(|| {
                self.error(
                    &format!("Class '{}' not found in symbol table", class.name),
                    class.location.clone(),
                );
            })?;

        // Resolve parent class if exists
        let parent_symbol_id = if let Some(parent_name) = &class.parent {
            let parent_id = self
                .symbol_table
                .lookup_symbol(parent_name)
                .ok_or_else(|| {
                    self.error(
                        &format!("Parent class '{}' not found", parent_name),
                        class.location.clone(),
                    );
                })?;
            Some(parent_id)
        } else {
            None
        };

        // Lookup constructor symbol - it was already created in the first pass (register_top_level_symbols)
        // This ensures constructors are available before global functions are resolved
        let constructor_name = format!("{}.constructor", class.name);
        let constructor_symbol_id = self
            .symbol_table
            .lookup_symbol(&constructor_name)
            .ok_or_else(|| {
                self.error(
                    &format!("Constructor symbol for class '{}' not found - this is an internal compiler error", class.name),
                    class.location.clone(),
                );
            })?;

        // Create class scope
        let class_scope = self.symbol_table.create_scope(
            None,
            ScopeType::Class {
                class_id: class_symbol_id,
            },
        );
        self.symbol_table.enter_scope(class_scope);
        self.current_class = Some(class_symbol_id);

        // Resolve fields
        let mut resolved_fields = Vec::new();
        let mut field_symbol_ids = Vec::new();

        for field in &class.fields {
            let field_symbol_id = self.symbol_table.create_symbol(
                field.name.clone(),
                SymbolKind::Field {
                    class_id: class_symbol_id,
                    field_type: field.field_type.clone(),
                },
                class_scope,
                field.location.clone(),
            );
            // Propagate inline private: visibility (SEM005).
            if field.is_private {
                self.symbol_table
                    .mark_as_private(field_symbol_id, class.name.clone());
            }

            field_symbol_ids.push(field_symbol_id);

            let resolved_initializer = if let Some(init) = &field.initializer {
                Some(self.resolve_expression(init)?)
            } else {
                None
            };

            resolved_fields.push(ResolvedHirField {
                name: field.name.clone(),
                symbol_id: field_symbol_id,
                field_type: field.field_type.clone(),
                initializer: resolved_initializer,
                location: field.location.clone(),
            });
        }

        // Update class symbol with fields and parent immediately after creating them
        if let Some(class_symbol) = self.symbol_table.get_symbol_mut(class_symbol_id) {
            if let SymbolKind::Class { fields, parent, .. } = &mut class_symbol.kind {
                *fields = field_symbol_ids.clone();
                *parent = parent_symbol_id;
            }
        }

        // Resolve constructor (symbol was already created in global scope)
        let resolved_constructor = if let Some(constructor) = &class.constructor {
            // Use explicit constructor
            Some(self.resolve_constructor(constructor, class_symbol_id, constructor_symbol_id)?)
        } else {
            // NOTE: Generate default constructor with empty body
            Some(ResolvedHirConstructor {
                symbol_id: constructor_symbol_id,
                parameters: vec![],
                body: ResolvedHirBlock {
                    statements: vec![], // Empty body for default constructor
                    location: class.location.clone(),
                },
                location: class.location.clone(),
            })
        };

        // Resolve methods — two-pass to allow mutual method references.
        // Pass 1: register all method symbols in class_scope so that any method
        //         body can call any other method (including private ones) by name.
        let mut resolved_methods = Vec::new();
        let mut method_symbol_ids = Vec::new();

        for method in &class.methods {
            let method_symbol_id = self.symbol_table.create_symbol(
                method.name.clone(),
                SymbolKind::Method {
                    class_id: class_symbol_id,
                    parameters: method
                        .parameters
                        .iter()
                        .map(|p| p.param_type.clone())
                        .collect(),
                    return_type: method.return_type.clone(),
                },
                class_scope,
                method.location.clone(),
            );
            if method.is_private {
                self.symbol_table
                    .mark_as_private(method_symbol_id, class.name.clone());
            }
            method_symbol_ids.push(method_symbol_id);
        }

        // Update class symbol with method IDs now so lookup_class_member works
        // during body resolution below.
        if let Some(class_symbol) = self.symbol_table.get_symbol_mut(class_symbol_id) {
            if let SymbolKind::Class { methods, .. } = &mut class_symbol.kind {
                *methods = method_symbol_ids.clone();
            }
        }

        // Pass 2: resolve method bodies (all symbols are visible in class_scope).
        for (method, &method_symbol_id) in class.methods.iter().zip(method_symbol_ids.iter()) {
            let resolved_method =
                self.resolve_method(method.clone(), class_symbol_id, method_symbol_id)?;
            resolved_methods.push(resolved_method);
        }

        // Update class symbol with fields and parent (methods were set in pass 1 above).
        if let Some(class_symbol) = self.symbol_table.get_symbol_mut(class_symbol_id) {
            if let SymbolKind::Class { fields, parent, .. } = &mut class_symbol.kind {
                *fields = field_symbol_ids.clone();
                *parent = parent_symbol_id;
            }
        }

        // Resolve always: block expressions in class scope (fields are visible here)
        let mut resolved_invariants = Vec::new();
        for invariant_expr in &class.invariants {
            match self.resolve_expression(invariant_expr) {
                Ok(resolved) => resolved_invariants.push(resolved),
                Err(()) => {
                    // Error already logged by resolve_expression; skip this condition
                }
            }
        }

        // Exit class scope
        self.symbol_table.exit_scope();
        self.current_class = None;

        Ok(ResolvedHirClass {
            name: class.name,
            symbol_id: class_symbol_id,
            parent: parent_symbol_id,
            fields: resolved_fields,
            constructor: resolved_constructor,
            methods: resolved_methods,
            invariants: resolved_invariants,
            location: class.location,
        })
    }

    /// Resolve a constructor
    fn resolve_constructor(
        &mut self,
        constructor: &HirConstructor,
        class_id: SymbolId,
        constructor_symbol_id: SymbolId,
    ) -> Result<ResolvedHirConstructor, ()> {
        // Create constructor scope
        let constructor_scope = self
            .symbol_table
            .create_scope(None, ScopeType::Constructor { class_id });
        self.symbol_table.enter_scope(constructor_scope);

        // Set current class for implicit field access
        let previous_class = self.current_class;
        self.current_class = Some(class_id);

        // Resolve parameters
        let mut resolved_parameters = Vec::new();
        for param in &constructor.parameters {
            let param_symbol_id = self.symbol_table.create_symbol(
                param.name.clone(),
                SymbolKind::Parameter {
                    param_type: param.param_type.clone(),
                },
                constructor_scope,
                param.location.clone(),
            );

            // Resolve default value expression if present
            let default_value = if let Some(default_expr) = &param.default_value {
                Some(self.resolve_expression(default_expr)?)
            } else {
                None
            };

            resolved_parameters.push(ResolvedHirParameter {
                name: param.name.clone(),
                symbol_id: param_symbol_id,
                param_type: param.param_type.clone(),
                default_value,
                is_variadic: false, // Variadic not yet in language spec
                location: param.location.clone(),
            });
        }

        // Resolve body
        let resolved_body = self.resolve_block(&constructor.body)?;

        // Exit constructor scope and restore previous class context
        self.symbol_table.exit_scope();
        self.current_class = previous_class;

        Ok(ResolvedHirConstructor {
            symbol_id: constructor_symbol_id,
            parameters: resolved_parameters,
            body: resolved_body,
            location: constructor.location.clone(),
        })
    }

    /// Resolve a method
    fn resolve_method(
        &mut self,
        method: HirMethod,
        class_id: SymbolId,
        method_id: SymbolId,
    ) -> Result<ResolvedHirMethod, ()> {
        // Create method scope
        let method_scope = self.symbol_table.create_scope(
            None,
            ScopeType::Method {
                method_id,
                class_id,
            },
        );
        self.symbol_table.enter_scope(method_scope);
        self.current_function = Some(method_id);

        // Set current class for implicit field access
        let previous_class = self.current_class;
        self.current_class = Some(class_id);
        let previous_return_type = self.current_function_return_type.clone();
        self.current_function_return_type = Some(method.return_type.clone());

        // Resolve parameters
        let mut resolved_parameters = Vec::new();
        for param in &method.parameters {
            let param_symbol_id = self.symbol_table.create_symbol(
                param.name.clone(),
                SymbolKind::Parameter {
                    param_type: param.param_type.clone(),
                },
                method_scope,
                param.location.clone(),
            );

            // Resolve default value expression if present
            let default_value = if let Some(default_expr) = &param.default_value {
                Some(self.resolve_expression(default_expr)?)
            } else {
                None
            };

            resolved_parameters.push(ResolvedHirParameter {
                name: param.name.clone(),
                symbol_id: param_symbol_id,
                param_type: param.param_type.clone(),
                default_value,
                is_variadic: false, // Variadic not yet in language spec
                location: param.location.clone(),
            });
        }

        // Resolve body
        let resolved_body = self.resolve_block(&method.body)?;

        // Exit method scope and restore previous class context
        self.symbol_table.exit_scope();
        self.current_function = None;
        self.current_class = previous_class;
        self.current_function_return_type = previous_return_type;

        Ok(ResolvedHirMethod {
            name: method.name,
            symbol_id: method_id,
            parameters: resolved_parameters,
            return_type: method.return_type,
            body: resolved_body,
            location: method.location,
        })
    }

    /// Resolve imports (basic implementation)
    fn resolve_imports(&mut self, imports: &[HirImport]) -> Result<Vec<ResolvedHirImport>, ()> {
        let mut resolved_imports = Vec::new();

        for import in imports {
            // Create a placeholder module entry
            let module_id = self.symbol_table.create_module(
                import.module_name.clone(),
                format!("{}.cln", import.module_name),
            );

            // Register the module name as a Namespace symbol in the current scope.
            // This allows `ModuleName.function(args)` syntax to resolve correctly:
            // the resolver recognises the receiver as a Namespace and emits a qualified call.
            let _namespace_symbol_id = self.symbol_table.create_symbol(
                import.module_name.clone(),
                SymbolKind::Namespace { functions: vec![] },
                self.symbol_table.current_scope_id(),
                import.location.clone(),
            );

            let resolved_items = if let Some(items) = &import.items {
                let mut resolved_items = Vec::new();
                for item in items {
                    // Register each explicitly-imported item as a Function symbol.
                    // The full resolution of parameter/return types happens during
                    // the actual module compilation; here we create placeholders that
                    // are sufficient for the resolver to proceed without errors.
                    let symbol_id = self.symbol_table.create_symbol(
                        item.clone(),
                        SymbolKind::Function {
                            parameters: vec![],
                            return_type: None,
                        },
                        self.symbol_table.current_scope_id(),
                        import.location.clone(),
                    );
                    resolved_items.push((item.clone(), symbol_id));
                }
                Some(resolved_items)
            } else {
                None
            };

            resolved_imports.push(ResolvedHirImport {
                module_name: import.module_name.clone(),
                module_id,
                items: resolved_items,
                location: import.location.clone(),
            });
        }

        Ok(resolved_imports)
    }

    /// Resolve tests
    fn resolve_tests(&mut self, tests: &[HirTest]) -> Result<Vec<ResolvedHirTest>, ()> {
        let mut resolved_tests = Vec::new();

        for test in tests {
            // Create test scope
            let test_scope = self.symbol_table.create_scope(None, ScopeType::Test);
            self.symbol_table.enter_scope(test_scope);

            let resolved_body = self.resolve_block(&test.body)?;

            self.symbol_table.exit_scope();

            resolved_tests.push(ResolvedHirTest {
                name: test.name.clone(),
                description: test.description.clone(),
                body: resolved_body,
                location: test.location.clone(),
            });
        }

        Ok(resolved_tests)
    }

    /// Resolve external functions (WASM imports)
    fn resolve_externals(
        &mut self,
        externals: &[crate::hir::HirExternalFunction],
    ) -> Result<Vec<crate::resolver::ResolvedHirExternalFunction>, ()> {
        let mut resolved_externals = Vec::new();

        for external in externals {
            // External function parameters - convert to resolved parameters
            let resolved_parameters: Vec<ResolvedHirParameter> = external
                .parameters
                .iter()
                .enumerate()
                .map(|(idx, p)| ResolvedHirParameter {
                    name: p.name.clone(),
                    symbol_id: SymbolId(10000 + idx), // Synthetic ID for external params
                    param_type: p.param_type.clone(),
                    default_value: None,
                    is_variadic: false,
                    location: p.location.clone(),
                })
                .collect();

            resolved_externals.push(crate::resolver::ResolvedHirExternalFunction {
                name: external.name.clone(),
                parameters: resolved_parameters,
                return_type: external.return_type.clone(),
                module: external.module.clone(),
                location: external.location.clone(),
            });
        }

        Ok(resolved_externals)
    }

    /// Resolve state block
    fn resolve_state_block(
        &mut self,
        state_block: &HirStateBlock,
    ) -> Result<ResolvedHirStateBlock, ()> {
        let mut resolved_declarations = Vec::new();

        for decl in &state_block.declarations {
            // Look up symbol_id from symbol table
            let symbol_id = self
                .symbol_table
                .lookup_symbol_in_scope(&decl.name, ScopeId(0))
                .ok_or_else(|| {
                    tracing::error!("Undefined state variable: {}", decl.name);
                })?;

            // Resolve the initializer expression
            let resolved_initializer = self.resolve_expression(&decl.initializer)?;

            // Resolve guard clause if present
            // Guards use a special 'value' binding that represents the proposed new value
            let resolved_guard = if let Some(ref guard) = decl.guard {
                // Create a temporary scope for the guard context
                let guard_scope = self
                    .symbol_table
                    .create_scope(Some(self.symbol_table.current_scope_id()), ScopeType::Block);
                self.symbol_table.enter_scope(guard_scope);

                // Add 'value' as a parameter in the guard scope with the state variable's type
                let value_symbol_id = self.symbol_table.create_symbol(
                    "value".to_string(),
                    SymbolKind::Parameter {
                        param_type: decl.state_type.clone(),
                    },
                    guard_scope,
                    guard.location.clone(),
                );

                // Resolve the guard condition with 'value' available
                let resolved_condition = self.resolve_expression(&guard.condition)?;

                // Exit the guard scope
                self.symbol_table.exit_scope();

                Some(ResolvedHirGuardClause {
                    condition: resolved_condition,
                    value_symbol_id,
                    error_message: guard.error_message.clone(),
                    location: guard.location.clone(),
                })
            } else {
                None
            };

            resolved_declarations.push(ResolvedHirStateDeclaration {
                symbol_id,
                name: decl.name.clone(),
                state_type: decl.state_type.clone(),
                initializer: resolved_initializer,
                guard: resolved_guard,
                location: decl.location.clone(),
            });
        }

        // Resolve computed state declarations.
        // Each computed declaration body is resolved in its own block scope so
        // that temporaries defined inside the body don't leak into the state scope.
        let mut resolved_computed: Vec<crate::resolver::ResolvedHirComputedDeclaration> =
            Vec::new();
        for comp in &state_block.computed {
            // The symbol was already registered in register_top_level_symbols; look it up.
            let symbol_id = self
                .symbol_table
                .lookup_symbol_in_scope(&comp.name, ScopeId(0))
                .ok_or_else(|| {
                    tracing::error!("Undefined computed state variable: {}", comp.name);
                })?;

            let resolved_body = self.resolve_block(&comp.body)?;

            resolved_computed.push(crate::resolver::ResolvedHirComputedDeclaration {
                symbol_id,
                name: comp.name.clone(),
                computed_type: comp.computed_type.clone(),
                body: resolved_body,
                location: comp.location.clone(),
            });
        }

        // Resolve state invariant rules
        let mut resolved_rules = Vec::new();
        for rule_expr in &state_block.rules {
            let resolved_rule = self.resolve_expression(rule_expr)?;
            resolved_rules.push(resolved_rule);
        }

        Ok(ResolvedHirStateBlock {
            declarations: resolved_declarations,
            computed: resolved_computed,
            rules: resolved_rules,
            scope: state_block.scope,
            location: state_block.location.clone(),
        })
    }

    /// Resolve a block of statements
    fn resolve_block(&mut self, block: &HirBlock) -> Result<ResolvedHirBlock, ()> {
        // Create block scope
        let block_scope = self.symbol_table.create_scope(None, ScopeType::Block);
        self.symbol_table.enter_scope(block_scope);

        let mut resolved_statements = Vec::new();
        for statement in &block.statements {
            resolved_statements.push(self.resolve_statement(statement)?);
        }

        self.symbol_table.exit_scope();

        Ok(ResolvedHirBlock {
            statements: resolved_statements,
            location: block.location.clone(),
        })
    }

    /// Resolve a statement
    fn resolve_statement(&mut self, statement: &HirStatement) -> Result<ResolvedHirStatement, ()> {
        match statement {
            HirStatement::VariableDeclaration {
                name,
                var_type,
                initializer,
                is_mutable,
                location,
            } => {
                let initializer_resolved = if let Some(init) = initializer {
                    Some(self.resolve_expression(init)?)
                } else {
                    None
                };

                // SCOPE002: check for redeclaration in the *same* scope (not shadowing).
                // Shadowing a name from an outer scope is allowed (SCOPE003 is a warning, not an error).
                let current_scope = self.symbol_table.current_scope_id();
                if let Some(existing_id) = self.symbol_table.lookup_symbol_in_current_scope(name) {
                    if let Some(existing) = self.symbol_table.get_symbol(existing_id) {
                        if existing.scope_id == current_scope {
                            self.error_with_code(
                                &format!("Variable '{}' is already declared in this scope", name),
                                "SCOPE002",
                                location.clone(),
                            );
                        }
                    }
                }

                let symbol_id = self.symbol_table.create_symbol(
                    name.clone(),
                    SymbolKind::Variable {
                        var_type: var_type.clone(),
                        is_mutable: *is_mutable,
                    },
                    current_scope,
                    location.clone(),
                );

                Ok(ResolvedHirStatement::VariableDeclaration {
                    name: name.clone(),
                    symbol_id,
                    var_type: var_type.clone(),
                    initializer: initializer_resolved,
                    location: location.clone(),
                })
            }

            HirStatement::Assignment {
                target,
                value,
                location,
            } => {
                let resolved_target = self.resolve_lvalue(target)?;
                let resolved_value = self.resolve_expression(value)?;

                Ok(ResolvedHirStatement::Assignment {
                    target: resolved_target,
                    value: resolved_value,
                    location: location.clone(),
                })
            }

            HirStatement::Expression {
                expression,
                location,
            } => {
                let resolved_expression = self.resolve_expression(expression)?;

                Ok(ResolvedHirStatement::Expression {
                    expression: resolved_expression,
                    location: location.clone(),
                })
            }

            HirStatement::Return { value, location } => {
                let resolved_value = if let Some(val) = value {
                    Some(self.resolve_expression(val)?)
                } else {
                    None
                };

                Ok(ResolvedHirStatement::Return {
                    value: resolved_value,
                    location: location.clone(),
                })
            }

            HirStatement::If {
                condition,
                then_branch,
                else_branch,
                location,
            } => {
                let resolved_condition = self.resolve_expression(condition)?;
                let resolved_then = self.resolve_block(then_branch)?;
                let resolved_else = if let Some(else_block) = else_branch {
                    Some(self.resolve_block(else_block)?)
                } else {
                    None
                };

                Ok(ResolvedHirStatement::If {
                    condition: resolved_condition,
                    then_branch: resolved_then,
                    else_branch: resolved_else,
                    location: location.clone(),
                })
            }

            HirStatement::For {
                variable,
                iterable,
                body,
                location,
            } => {
                let resolved_iterable = self.resolve_expression(iterable)?;

                // Create new scope for loop variable
                let loop_scope = self.symbol_table.create_scope(None, ScopeType::Block);
                self.symbol_table.enter_scope(loop_scope);

                let var_symbol_id = self.symbol_table.create_symbol(
                    variable.clone(),
                    SymbolKind::Variable {
                        var_type: HirType::Inferred {
                            id: 0,
                            location: location.clone(),
                        },
                        is_mutable: true, // Loop variables are mutable
                    },
                    loop_scope,
                    location.clone(),
                );

                let resolved_body = self.resolve_block(body)?;

                self.symbol_table.exit_scope();

                Ok(ResolvedHirStatement::For {
                    variable: variable.clone(),
                    variable_symbol_id: var_symbol_id,
                    iterable: resolved_iterable,
                    body: resolved_body,
                    location: location.clone(),
                })
            }

            HirStatement::While {
                condition,
                body,
                location,
            } => {
                let resolved_condition = self.resolve_expression(condition)?;

                // Create new scope for while body
                let loop_scope = self.symbol_table.create_scope(None, ScopeType::Block);
                self.symbol_table.enter_scope(loop_scope);

                let resolved_body = self.resolve_block(body)?;

                self.symbol_table.exit_scope();

                Ok(ResolvedHirStatement::While {
                    condition: resolved_condition,
                    body: resolved_body,
                    location: location.clone(),
                })
            }

            HirStatement::Print {
                expression,
                newline,
                location,
            } => {
                let resolved_expression = self.resolve_expression(expression)?;

                Ok(ResolvedHirStatement::Print {
                    expression: resolved_expression,
                    newline: *newline,
                    location: location.clone(),
                })
            }

            HirStatement::LaterAssignment {
                variable,
                expression,
                location,
            } => {
                let resolved_expression = self.resolve_expression(expression)?;

                // Create symbol for the variable being assigned
                let symbol_id = self.symbol_table.create_symbol(
                    variable.clone(),
                    SymbolKind::Variable {
                        var_type: HirType::Void, // Type will be inferred later
                        is_mutable: true,        // Later assignments make variables mutable
                    },
                    self.symbol_table.current_scope_id(),
                    location.clone(),
                );

                Ok(ResolvedHirStatement::LaterAssignment {
                    variable: variable.clone(),
                    symbol_id,
                    expression: resolved_expression,
                    location: location.clone(),
                })
            }

            HirStatement::Background {
                expression,
                location,
            } => {
                let resolved_expression = self.resolve_expression(expression)?;
                Ok(ResolvedHirStatement::Background {
                    expression: resolved_expression,
                    location: location.clone(),
                })
            }

            HirStatement::Break { location } => Ok(ResolvedHirStatement::Break {
                location: location.clone(),
            }),

            HirStatement::Continue { location } => Ok(ResolvedHirStatement::Continue {
                location: location.clone(),
            }),

            HirStatement::Require {
                condition,
                location,
            } => {
                let resolved_condition = self.resolve_expression(condition)?;
                Ok(ResolvedHirStatement::Require {
                    condition: resolved_condition,
                    location: location.clone(),
                })
            }

            HirStatement::Ensure {
                condition,
                location,
            } => {
                // `result` in ensure conditions is a synthetic variable that refers to the
                // function's return value. It does not appear in the source — the MIR builder
                // captures the actual return value under this name just before evaluating
                // postconditions.
                //
                // To let the resolver accept `result` references without errors, we create a
                // temporary block scope and inject a `result` symbol with the enclosing
                // function's return type. The scope is exited immediately after the condition
                // is resolved, so `result` does not leak into the surrounding function scope.
                let ensure_scope = self.symbol_table.create_scope(None, ScopeType::Block);
                self.symbol_table.enter_scope(ensure_scope);

                if let Some(return_type) = &self.current_function_return_type.clone() {
                    // Only inject `result` for non-void functions (ensure is a no-op for void).
                    if !matches!(return_type, HirType::Void) {
                        self.symbol_table.create_symbol(
                            "result".to_string(),
                            SymbolKind::Variable {
                                var_type: return_type.clone(),
                                is_mutable: false,
                            },
                            ensure_scope,
                            location.clone(),
                        );
                    }
                }

                let resolved_condition = self.resolve_expression(condition);

                self.symbol_table.exit_scope();

                Ok(ResolvedHirStatement::Ensure {
                    condition: resolved_condition?,
                    location: location.clone(),
                })
            }
        }
    }

    /// Resolve an expression
    fn resolve_expression(
        &mut self,
        expression: &HirExpression,
    ) -> Result<ResolvedHirExpression, ()> {
        // Check recursion depth to prevent stack overflow
        const MAX_EXPRESSION_RECURSION: usize = 50; // Increased from 5 to allow more complex expressions
        if self.expression_recursion_depth >= MAX_EXPRESSION_RECURSION {
            self.error(
                &format!(
                    "Maximum expression recursion depth exceeded ({})",
                    MAX_EXPRESSION_RECURSION
                ),
                expression.location().clone(),
            );
            return Err(());
        }

        // Increment recursion depth
        self.expression_recursion_depth += 1;

        let result = self.resolve_expression_internal(expression);

        // Decrement recursion depth
        self.expression_recursion_depth -= 1;

        result
    }

    /// Internal expression resolution logic
    fn resolve_expression_internal(
        &mut self,
        expression: &HirExpression,
    ) -> Result<ResolvedHirExpression, ()> {
        match expression {
            HirExpression::Literal { value, location } => Ok(ResolvedHirExpression::Literal {
                value: value.clone(),
                location: location.clone(),
            }),

            HirExpression::Variable { name, location } => {
                // Special case: `this` inside a class method resolves to the current instance
                if name == "this" {
                    if let Some(current_class_id) = self.current_class {
                        return Ok(ResolvedHirExpression::This {
                            class_symbol_id: current_class_id,
                            location: location.clone(),
                        });
                    }
                    // `this` outside a class context is an error
                    self.error(
                        "'this' can only be used inside a class method",
                        location.clone(),
                    );
                    return Err(());
                }

                // IMPORTANT: Check for local variables/parameters FIRST before class fields
                // This follows standard variable shadowing rules:
                // - A local parameter shadows a field with the same name
                // - Use explicit `this.field` to access the field when shadowed
                // This enables patterns like: constructor(string name) { this.name = name }
                // where right-hand side `name` refers to the parameter
                if let Some(symbol_id) = self.symbol_table.lookup_symbol(name) {
                    // Check if this is a parameter or local variable (not a field)
                    if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                        match &symbol.kind {
                            SymbolKind::Parameter { .. } | SymbolKind::Variable { .. } => {
                                // Found a local variable/parameter - use it (shadows any field)
                                return Ok(ResolvedHirExpression::Variable {
                                    name: name.clone(),
                                    symbol_id,
                                    location: location.clone(),
                                });
                            }
                            _ => {
                                // Not a local variable/parameter, continue to check fields
                            }
                        }
                    }
                }

                // If no local variable/parameter found, check for class fields (implicit field access)
                if let Some(current_class_id) = self.current_class {
                    if let Some(class_symbol) = self.symbol_table.get_symbol(current_class_id) {
                        if let SymbolKind::Class { fields, parent, .. } = &class_symbol.kind {
                            // Check current class fields
                            for &field_id in fields {
                                if let Some(field_symbol) = self.symbol_table.get_symbol(field_id) {
                                    if field_symbol.name == *name {
                                        // Convert variable access to field access
                                        return Ok(ResolvedHirExpression::FieldAccess {
                                            object: Box::new(ResolvedHirExpression::This {
                                                class_symbol_id: current_class_id,
                                                location: location.clone(),
                                            }),
                                            field: name.clone(),
                                            field_symbol_id: field_id,
                                            location: location.clone(),
                                        });
                                    }
                                }
                            }

                            // Check parent class fields if inheritance is involved
                            if let Some(parent_class_id) = parent {
                                if let Some(parent_symbol) =
                                    self.symbol_table.get_symbol(*parent_class_id)
                                {
                                    if let SymbolKind::Class {
                                        fields: parent_fields,
                                        ..
                                    } = &parent_symbol.kind
                                    {
                                        for &parent_field_id in parent_fields {
                                            if let Some(parent_field_symbol) =
                                                self.symbol_table.get_symbol(parent_field_id)
                                            {
                                                if parent_field_symbol.name == *name {
                                                    // Convert variable access to inherited field access
                                                    return Ok(
                                                        ResolvedHirExpression::FieldAccess {
                                                            object: Box::new(
                                                                ResolvedHirExpression::This {
                                                                    class_symbol_id:
                                                                        current_class_id,
                                                                    location: location.clone(),
                                                                },
                                                            ),
                                                            field: name.clone(),
                                                            field_symbol_id: parent_field_id,
                                                            location: location.clone(),
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Try to find the variable in normal scope (for non-shadowed cases)
                if let Some(symbol_id) = self.symbol_table.lookup_symbol(name) {
                    // SCOPE005: Screen-local state cannot be accessed outside its screen.
                    if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                        if let SymbolKind::StateVariable {
                            screen_name: Some(ref owner_screen),
                            ..
                        } = &symbol.kind
                        {
                            let currently_in_owner = self
                                .current_screen
                                .as_deref()
                                .map(|s| s == owner_screen)
                                .unwrap_or(false);
                            if !currently_in_owner {
                                self.errors.push(CompilerError::Validation {
                                    context: Box::new(
                                        crate::error::ErrorContext::new(
                                            format!(
                                                "State variable '{}' is local to screen '{}' and cannot be accessed here",
                                                name, owner_screen
                                            ),
                                            Some(format!(
                                                "Screen-local state is private to its screen. \
                                                 Access '{}' only from within screen '{}'.",
                                                name, owner_screen
                                            )),
                                            crate::error::ErrorType::Validation,
                                            Some(location.clone()),
                                        )
                                        .with_error_code("SCOPE005"),
                                    ),
                                });
                                return Err(());
                            }
                        }
                    }

                    return Ok(ResolvedHirExpression::Variable {
                        name: name.clone(),
                        symbol_id,
                        location: location.clone(),
                    });
                }

                // If still not found, report error (SCOPE001: UseBeforeDeclaration)
                self.error_with_code(
                    &format!("Variable '{}' not found", name),
                    "SCOPE001",
                    location.clone(),
                );
                Err(())
            }

            HirExpression::BinaryOp {
                left,
                op,
                right,
                location,
            } => {
                let resolved_left = self.resolve_expression(left)?;
                let resolved_right = self.resolve_expression(right)?;

                Ok(ResolvedHirExpression::BinaryOp {
                    left: Box::new(resolved_left),
                    op: op.clone(),
                    right: Box::new(resolved_right),
                    location: location.clone(),
                })
            }

            HirExpression::UnaryOp {
                op,
                operand,
                location,
            } => {
                let resolved_operand = self.resolve_expression(operand)?;

                Ok(ResolvedHirExpression::UnaryOp {
                    op: op.clone(),
                    operand: Box::new(resolved_operand),
                    location: location.clone(),
                })
            }

            HirExpression::Call {
                function,
                arguments,
                location,
            } => {
                // Lookup function in symbol table (includes builtin functions)
                let function_symbol_opt = self.symbol_table.lookup_symbol(function);

                // If we're inside a class, check whether the resolved symbol is a method of
                // the current class.  Methods need `this` as an implicit first argument, so
                // we emit MethodCall { receiver: This } rather than a bare Call.  This applies
                // whether the method was found through scope chain walking (two-pass registration)
                // or via the lookup_class_member fallback.
                if let Some(current_class_id) = self.current_class {
                    let method_symbol_id_opt = function_symbol_opt
                        .and_then(|sym_id| {
                            // Accept it only if it belongs to the current class.
                            if let Some(sym) = self.symbol_table.get_symbol(sym_id) {
                                if let crate::resolver::symbol_table::SymbolKind::Method {
                                    class_id,
                                    ..
                                } = &sym.kind
                                {
                                    if *class_id == current_class_id {
                                        return Some(sym_id);
                                    }
                                }
                            }
                            None
                        })
                        .or_else(|| {
                            // Fallback: look it up by name in case it isn't yet reachable via scope.
                            self.symbol_table
                                .lookup_class_member(current_class_id, function)
                        });

                    if let Some(method_symbol_id) = method_symbol_id_opt {
                        let mut resolved_arguments = Vec::new();
                        for arg in arguments {
                            resolved_arguments.push(self.resolve_expression(arg)?);
                        }
                        return Ok(ResolvedHirExpression::MethodCall {
                            receiver: Box::new(ResolvedHirExpression::This {
                                class_symbol_id: current_class_id,
                                location: location.clone(),
                            }),
                            method: function.clone(),
                            method_symbol_id: Some(method_symbol_id),
                            arguments: resolved_arguments,
                            location: location.clone(),
                        });
                    }
                }

                // If still not found, try response-helper rewrites before erroring.
                // html(content) is a frame.server response helper — rewrite to htmlResponse()
                // which is generated by the endpoints block expansion. This handles the case
                // where html is not yet registered as a namespace (frame.ui not loaded).
                let (function, function_symbol_opt) =
                    if function_symbol_opt.is_none() && function == "html" {
                        let resolved = self.symbol_table.lookup_symbol("htmlResponse");
                        ("htmlResponse".to_string(), resolved)
                    } else {
                        (function.clone(), function_symbol_opt)
                    };

                // If still not found, emit error
                let function_symbol_id = function_symbol_opt.ok_or_else(|| {
                    self.error(
                        &format!("Function '{}' not found", function),
                        location.clone(),
                    );
                })?;

                let mut resolved_arguments = Vec::new();
                for arg in arguments {
                    resolved_arguments.push(self.resolve_expression(arg)?);
                }

                // Inject default arguments for language-alias bridge functions that declare
                // `param_defaults` in their plugin.toml `[language].functions` section.
                // When a caller passes fewer args than the max param count, fill the gap with
                // the declared default values (integer literals or string literals).
                if let Some(defaults) = self.language_fn_defaults.get(function.as_str()).cloned() {
                    let provided = resolved_arguments.len();
                    let expected = defaults.len();
                    if provided < expected {
                        for i in provided..expected {
                            let default_str = defaults.get(i).map(|s| s.as_str()).unwrap_or("");
                            if default_str.is_empty() {
                                // Required parameter — do not inject; type checker will flag it
                                break;
                            }
                            let default_expr = if let Ok(n) = default_str.parse::<i64>() {
                                ResolvedHirExpression::Literal {
                                    value: crate::ast::Value::Integer(n),
                                    location: location.clone(),
                                }
                            } else {
                                ResolvedHirExpression::Literal {
                                    value: crate::ast::Value::String(default_str.to_string()),
                                    location: location.clone(),
                                }
                            };
                            resolved_arguments.push(default_expr);
                        }
                    }
                }

                // Check if this is actually a namespace (cannot be called directly).
                //
                // Special case: `input(prompt)` is sugar for `input.string(prompt)` per
                // stdlib-reference.md §Console I/O.  The spec lists `input(prompt) → string`
                // as a valid call form even though `input` is registered as a namespace so
                // that `input.integer(...)` etc. also resolve.  Allow the direct call by
                // rewriting the function name to `input.string`.
                if let Some(symbol) = self.symbol_table.get_symbol(function_symbol_id) {
                    if matches!(symbol.kind, SymbolKind::Namespace { .. }) {
                        if function == "input" {
                            // Rewrite `input(prompt)` → `input.string(prompt)`
                            let input_string_id = self
                                .symbol_table
                                .lookup_symbol("input.string")
                                .or_else(|| self.symbol_table.lookup_symbol("input"));
                            let resolved_id = input_string_id.unwrap_or(function_symbol_id);
                            return Ok(ResolvedHirExpression::Call {
                                function: "input.string".to_string(),
                                function_symbol_id: resolved_id,
                                arguments: resolved_arguments,
                                location: location.clone(),
                            });
                        }
                        if function == "json" {
                            // Rewrite `json(value)` → `json.encode(value)`
                            // json is registered as a namespace (json.get, json.encode, etc.) but
                            // calling json(x) directly is valid shorthand for json.encode(x) —
                            // used by frame.server response helpers alongside frame.data/frame.ui.
                            let json_encode_id = self
                                .symbol_table
                                .lookup_symbol("json.encode")
                                .or_else(|| self.symbol_table.lookup_symbol("json"));
                            let resolved_id = json_encode_id.unwrap_or(function_symbol_id);
                            return Ok(ResolvedHirExpression::Call {
                                function: "json.encode".to_string(),
                                function_symbol_id: resolved_id,
                                arguments: resolved_arguments,
                                location: location.clone(),
                            });
                        }
                        if function == "html" {
                            // Rewrite `html(content)` → `htmlResponse(content)`
                            // html is registered as a namespace (frame.ui html: blocks) but
                            // calling html(x) directly is valid shorthand for the htmlResponse()
                            // helper generated by frame.server's endpoints block expansion.
                            let html_response_id = self
                                .symbol_table
                                .lookup_symbol("htmlResponse")
                                .or_else(|| self.symbol_table.lookup_symbol("html"));
                            let resolved_id = html_response_id.unwrap_or(function_symbol_id);
                            return Ok(ResolvedHirExpression::Call {
                                function: "htmlResponse".to_string(),
                                function_symbol_id: resolved_id,
                                arguments: resolved_arguments,
                                location: location.clone(),
                            });
                        }
                        self.error(
                            &format!(
                                "'{}' is a namespace, not a function — use '{}.function_name()' syntax",
                                function, function
                            ),
                            location.clone(),
                        );
                        return Err(());
                    }
                }

                // Check if this is actually a constructor call (call to a class)
                if let Some(symbol) = self.symbol_table.get_symbol(function_symbol_id) {
                    if matches!(symbol.kind, SymbolKind::Class { .. }) {
                        // This is a constructor call, not a function call
                        // Look up the constructor's SymbolId by name
                        let constructor_name = format!("{}.constructor", function);
                        let constructor_symbol_id = self
                            .symbol_table
                            .lookup_symbol(&constructor_name)
                            .ok_or_else(|| {
                                self.error(
                                    &format!("Constructor for class '{}' not found", function),
                                    location.clone(),
                                );
                            })?;

                        return Ok(ResolvedHirExpression::Constructor {
                            class_name: function.clone(),
                            class_symbol_id: function_symbol_id,
                            constructor_symbol_id,
                            arguments: resolved_arguments,
                            location: location.clone(),
                        });
                    }
                }

                Ok(ResolvedHirExpression::Call {
                    function: function.clone(),
                    function_symbol_id,
                    arguments: resolved_arguments,
                    location: location.clone(),
                })
            }

            HirExpression::MethodCall {
                receiver,
                method,
                arguments,
                location,
            } => {
                // Check if the receiver is a class name (for static method calls)
                if let HirExpression::Variable {
                    name: class_name, ..
                } = receiver.as_ref()
                {
                    // Check if this variable name is actually a class symbol
                    if let Some(class_symbol_id) = self.symbol_table.lookup_symbol(class_name) {
                        if let Some(class_symbol) = self.symbol_table.get_symbol(class_symbol_id) {
                            if let SymbolKind::Class { .. } = &class_symbol.kind {
                                // This is a static method call on a class
                                let mut resolved_arguments = Vec::new();
                                for arg in arguments {
                                    resolved_arguments.push(self.resolve_expression(arg)?);
                                }

                                // Look up the static method in the class
                                let method_symbol_id = self
                                    .symbol_table
                                    .lookup_class_member(class_symbol_id, method)
                                    .unwrap_or({
                                        // Create a placeholder symbol for built-in static methods if not found
                                        // This allows built-in static methods to work even if not explicitly defined
                                        SymbolId(0)
                                    });

                                return Ok(ResolvedHirExpression::StaticMethodCall {
                                    namespace: vec![], // Two-level call
                                    class_name: class_name.clone(),
                                    class_symbol_id,
                                    method: method.clone(),
                                    method_symbol_id,
                                    arguments: resolved_arguments,
                                    location: location.clone(),
                                });
                            } else if let SymbolKind::Namespace { .. } = &class_symbol.kind {
                                // This is a namespace function call (e.g., logical.and, conditional.integer, string.length)
                                // NOTE: Use dot notation to match stdlib function registration
                                let qualified_name = format!("{}.{}", class_name, method);

                                // Try to look up the qualified function name in the symbol table
                                // CRITICAL: Stdlib functions (string.length, math.max, etc.) are NOT in the symbol table
                                // They're registered directly in MirCodeGenerator, so we use a placeholder SymbolId
                                let function_symbol_id =
                                    self.symbol_table.lookup_symbol(&qualified_name).unwrap_or({
                                        // Use SymbolId(0) as placeholder for stdlib namespace functions
                                        // The actual function lookup will happen during code generation
                                        SymbolId(0)
                                    });

                                // Resolve all arguments
                                let mut resolved_arguments = Vec::new();
                                for arg in arguments {
                                    resolved_arguments.push(self.resolve_expression(arg)?);
                                }

                                // Return as a regular function call (namespace functions are just functions)
                                return Ok(ResolvedHirExpression::Call {
                                    function: qualified_name,
                                    function_symbol_id,
                                    arguments: resolved_arguments,
                                    location: location.clone(),
                                });
                            }
                        }
                    }
                }

                // NOTE: Check if receiver is a FieldAccess that represents a namespace path
                // This handles three-level calls like compare.integer.greaterThan()
                if let HirExpression::FieldAccess {
                    object,
                    field: class_part,
                    ..
                } = receiver.as_ref()
                {
                    if let HirExpression::Variable {
                        name: namespace_part,
                        ..
                    } = object.as_ref()
                    {
                        // Check if namespace_part is a namespace
                        if let Some(ns_symbol_id) = self.symbol_table.lookup_symbol(namespace_part)
                        {
                            if let Some(ns_symbol) = self.symbol_table.get_symbol(ns_symbol_id) {
                                if matches!(ns_symbol.kind, SymbolKind::Namespace { .. }) {
                                    // This is a three-level call: namespace.class.method()
                                    // Resolve arguments
                                    let mut resolved_arguments = Vec::new();
                                    for arg in arguments {
                                        resolved_arguments.push(self.resolve_expression(arg)?);
                                    }

                                    // Look up the full class name
                                    let full_class_name =
                                        format!("{}.{}", namespace_part, class_part);
                                    let class_symbol_id = self
                                        .symbol_table
                                        .lookup_symbol(&full_class_name)
                                        .unwrap_or(SymbolId(0)); // Placeholder for built-in classes

                                    let method_symbol_id = SymbolId(0); // Placeholder for built-in methods

                                    return Ok(ResolvedHirExpression::StaticMethodCall {
                                        namespace: vec![namespace_part.clone()],
                                        class_name: class_part.clone(),
                                        class_symbol_id,
                                        method: method.clone(),
                                        method_symbol_id,
                                        arguments: resolved_arguments,
                                        location: location.clone(),
                                    });
                                }
                            }
                        }
                    }
                }

                // FUNC012 — Reject method-style calls on user-defined standalone functions.
                //
                // At this point we have already ruled out class static calls and namespace
                // calls above. If `method` resolves to a SymbolKind::Function in the
                // symbol table AND the symbol is not a builtin, it is a user-defined
                // standalone function and the caller must use regular call syntax:
                // `functionName(value, args)`.
                //
                // Builtin functions like `toString` and `toInteger` are registered as
                // SymbolKind::Function but are valid as method-style calls (e.g.
                // `value.toString()`), so they are excluded from this check.
                if let Some(symbol_id) = self.symbol_table.lookup_symbol(method) {
                    if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                        if matches!(symbol.kind, SymbolKind::Function { .. })
                            && !self.symbol_table.is_builtin(symbol_id)
                        {
                            self.errors
                                .push(CompilerError::method_call_on_standalone_function(
                                    method.as_str(),
                                    location.clone(),
                                ));
                            return Err(());
                        }
                    }
                }

                // Regular instance method call
                let resolved_receiver = self.resolve_expression(receiver)?;

                let mut resolved_arguments = Vec::new();
                for arg in arguments {
                    resolved_arguments.push(self.resolve_expression(arg)?);
                }

                // Method resolution is complex and depends on receiver type
                // For now, we'll resolve it as None (built-in method)
                let method_symbol_id = None;

                Ok(ResolvedHirExpression::MethodCall {
                    receiver: Box::new(resolved_receiver),
                    method: method.clone(),
                    method_symbol_id,
                    arguments: resolved_arguments,
                    location: location.clone(),
                })
            }

            HirExpression::FieldAccess {
                object,
                field,
                location,
            } => {
                // NOTE: Check if object is a Variable that refers to a namespace
                // This handles cases like compare.integer where "compare" is a namespace, not a variable
                if let HirExpression::Variable { name: obj_name, .. } = object.as_ref() {
                    if let Some(obj_symbol_id) = self.symbol_table.lookup_symbol(obj_name) {
                        if let Some(obj_symbol) = self.symbol_table.get_symbol(obj_symbol_id) {
                            if matches!(obj_symbol.kind, SymbolKind::Namespace { .. }) {
                                // This is a namespace access (e.g., compare.integer)
                                // The field is either another namespace or a class within the namespace
                                // Don't resolve as a regular field access - return as a variable with dotted name
                                // This will be handled later when we encounter the method call

                                // For now, create a placeholder Variable with the full dotted path
                                // This is not ideal but allows the rest of the pipeline to work
                                let full_name = format!("{}.{}", obj_name, field);

                                // Check if this full name is a known symbol (namespace or class)
                                if let Some(full_symbol_id) =
                                    self.symbol_table.lookup_symbol(&full_name)
                                {
                                    return Ok(ResolvedHirExpression::Variable {
                                        name: full_name,
                                        symbol_id: full_symbol_id,
                                        location: location.clone(),
                                    });
                                }

                                // If not found as a single symbol, this might be part of a three-level call
                                // Create a placeholder symbol for namespace path
                                return Ok(ResolvedHirExpression::Variable {
                                    name: full_name,
                                    symbol_id: SymbolId(0), // Placeholder
                                    location: location.clone(),
                                });
                            }
                        }
                    }
                }

                // Normal field access - resolve object first
                let resolved_object = self.resolve_expression(object)?;

                // Field resolution depends on object type - for now use placeholder
                let field_symbol_id = SymbolId(0);

                Ok(ResolvedHirExpression::FieldAccess {
                    object: Box::new(resolved_object),
                    field: field.clone(),
                    field_symbol_id,
                    location: location.clone(),
                })
            }

            HirExpression::Index {
                array,
                index,
                location,
            } => {
                let resolved_array = self.resolve_expression(array)?;
                let resolved_index = self.resolve_expression(index)?;

                Ok(ResolvedHirExpression::Index {
                    array: Box::new(resolved_array),
                    index: Box::new(resolved_index),
                    location: location.clone(),
                })
            }

            HirExpression::Array {
                elements,
                element_type,
                location,
            } => {
                let mut resolved_elements = Vec::new();
                for element in elements {
                    resolved_elements.push(self.resolve_expression(element)?);
                }

                Ok(ResolvedHirExpression::Array {
                    elements: resolved_elements,
                    element_type: element_type.clone(),
                    location: location.clone(),
                })
            }

            HirExpression::Constructor {
                class_name,
                arguments,
                location,
            } => {
                let class_symbol_id =
                    self.symbol_table.lookup_symbol(class_name).ok_or_else(|| {
                        self.error(
                            &format!("Class '{}' not found", class_name),
                            location.clone(),
                        );
                    })?;

                // Look up the constructor's SymbolId by name
                let constructor_name = format!("{}.constructor", class_name);
                let constructor_symbol_id = self
                    .symbol_table
                    .lookup_symbol(&constructor_name)
                    .ok_or_else(|| {
                        self.error(
                            &format!("Constructor for class '{}' not found", class_name),
                            location.clone(),
                        );
                    })?;

                let mut resolved_arguments = Vec::new();
                for arg in arguments {
                    resolved_arguments.push(self.resolve_expression(arg)?);
                }

                Ok(ResolvedHirExpression::Constructor {
                    class_name: class_name.clone(),
                    class_symbol_id,
                    constructor_symbol_id,
                    arguments: resolved_arguments,
                    location: location.clone(),
                })
            }

            HirExpression::Cast {
                expression,
                target_type,
                location,
            } => {
                let resolved_expression = self.resolve_expression(expression)?;

                Ok(ResolvedHirExpression::Cast {
                    expression: Box::new(resolved_expression),
                    target_type: target_type.clone(),
                    location: location.clone(),
                })
            }

            HirExpression::Assignment {
                target,
                value,
                location,
            } => {
                let resolved_target = self.resolve_lvalue(target)?;
                let resolved_value = self.resolve_expression(value)?;

                Ok(ResolvedHirExpression::Assignment {
                    target: resolved_target,
                    value: Box::new(resolved_value),
                    location: location.clone(),
                })
            }

            HirExpression::NamespaceCall {
                namespace,
                function,
                arguments,
                location,
            } => {
                // NOTE: Handle field access chains like test.flag.toString()
                // The namespace "test.flag" needs to be converted to a field access expression
                // BUT only if the first part is a variable, not a namespace
                if namespace.contains('.') {
                    // Split the namespace into parts (e.g., "test.flag" -> ["test", "flag"])
                    let parts: Vec<&str> = namespace.split('.').collect();
                    let base_name = parts[0];

                    // Check if the first part is a variable (not a namespace)
                    if let Some(base_symbol_id) = self.symbol_table.lookup_symbol(base_name) {
                        // Check if it's a namespace - if so, don't convert
                        let is_namespace =
                            if let Some(symbol) = self.symbol_table.get_symbol(base_symbol_id) {
                                matches!(symbol.kind, SymbolKind::Namespace { .. })
                            } else {
                                false
                            };

                        if !is_namespace {
                            // This is a variable with field accesses - convert to field access chain
                            let mut receiver = ResolvedHirExpression::Variable {
                                name: base_name.to_string(),
                                symbol_id: base_symbol_id,
                                location: location.clone(),
                            };

                            // Chain field accesses for remaining parts
                            for field_name in &parts[1..] {
                                receiver = ResolvedHirExpression::FieldAccess {
                                    object: Box::new(receiver),
                                    field: field_name.to_string(),
                                    field_symbol_id: SymbolId(0), // Placeholder, will be resolved by type checker
                                    location: location.clone(),
                                };
                            }

                            // Resolve arguments
                            let mut resolved_arguments = Vec::new();
                            for arg in arguments {
                                resolved_arguments.push(self.resolve_expression(arg)?);
                            }

                            // Return as a method call on the field access chain
                            return Ok(ResolvedHirExpression::MethodCall {
                                receiver: Box::new(receiver),
                                method: function.clone(),
                                method_symbol_id: None, // Will be resolved based on receiver type
                                arguments: resolved_arguments,
                                location: location.clone(),
                            });
                        }
                    }
                    // If base_name not found, continue with normal namespace processing below
                }

                // NOTE: Check if the "namespace" is actually a field (method call on field)
                // This handles cases like x.toString() where 'x' is a field, not a namespace
                if let Some(current_class_id) = self.current_class {
                    if let Some(class_symbol) = self.symbol_table.get_symbol(current_class_id) {
                        if let SymbolKind::Class { fields, parent, .. } = &class_symbol.kind {
                            // Check current class fields
                            for &field_id in fields {
                                if let Some(field_symbol) = self.symbol_table.get_symbol(field_id) {
                                    if field_symbol.name == *namespace {
                                        // Create field access for the receiver
                                        let receiver = ResolvedHirExpression::FieldAccess {
                                            object: Box::new(ResolvedHirExpression::This {
                                                class_symbol_id: current_class_id,
                                                location: location.clone(),
                                            }),
                                            field: namespace.clone(),
                                            field_symbol_id: field_id,
                                            location: location.clone(),
                                        };

                                        // Resolve arguments
                                        let mut resolved_arguments = Vec::new();
                                        for arg in arguments {
                                            resolved_arguments.push(self.resolve_expression(arg)?);
                                        }

                                        // Return as a method call
                                        return Ok(ResolvedHirExpression::MethodCall {
                                            receiver: Box::new(receiver),
                                            method: function.clone(),
                                            method_symbol_id: None,
                                            arguments: resolved_arguments,
                                            location: location.clone(),
                                        });
                                    }
                                }
                            }

                            // Check parent class fields if inheritance is involved
                            if let Some(parent_class_id) = parent {
                                if let Some(parent_symbol) =
                                    self.symbol_table.get_symbol(*parent_class_id)
                                {
                                    if let SymbolKind::Class {
                                        fields: parent_fields,
                                        ..
                                    } = &parent_symbol.kind
                                    {
                                        for &field_id in parent_fields {
                                            if let Some(field_symbol) =
                                                self.symbol_table.get_symbol(field_id)
                                            {
                                                if field_symbol.name == *namespace {
                                                    // Create field access for the receiver
                                                    let receiver =
                                                        ResolvedHirExpression::FieldAccess {
                                                            object: Box::new(
                                                                ResolvedHirExpression::This {
                                                                    class_symbol_id:
                                                                        current_class_id,
                                                                    location: location.clone(),
                                                                },
                                                            ),
                                                            field: namespace.clone(),
                                                            field_symbol_id: field_id,
                                                            location: location.clone(),
                                                        };

                                                    // Resolve arguments
                                                    let mut resolved_arguments = Vec::new();
                                                    for arg in arguments {
                                                        resolved_arguments
                                                            .push(self.resolve_expression(arg)?);
                                                    }

                                                    // Return as a method call
                                                    return Ok(ResolvedHirExpression::MethodCall {
                                                        receiver: Box::new(receiver),
                                                        method: function.clone(),
                                                        method_symbol_id: None,
                                                        arguments: resolved_arguments,
                                                        location: location.clone(),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // NOTE: Check if the "namespace" is actually a variable (method call) or a static class method
                // This handles cases like value.toString() where 'value' is a variable, not a namespace
                // However, do NOT convert if it's a known namespace or a class (static method call)
                if let Some(symbol_id) = self.symbol_table.lookup_symbol(namespace) {
                    // Check the symbol kind to determine if it's a true namespace or class
                    let symbol_kind = self
                        .symbol_table
                        .get_symbol(symbol_id)
                        .map(|symbol| symbol.kind.clone());

                    match symbol_kind {
                        Some(SymbolKind::Namespace { .. }) => {
                            // This is a legitimate namespace, continue with normal namespace processing
                            // Continue to normal namespace processing below
                        }
                        Some(SymbolKind::Class { methods, .. }) => {
                            // This is a static method call on a class (e.g., MathUtils.add(5, 3))
                            // Look for the method in the class
                            let method_symbol_id = methods.iter().find_map(|&method_id| {
                                self.symbol_table
                                    .get_symbol(method_id)
                                    .filter(|s| s.name == *function)
                                    .map(|_| method_id)
                            });

                            if let Some(method_id) = method_symbol_id {
                                // Resolve arguments
                                let mut resolved_arguments = Vec::new();
                                for arg in arguments {
                                    resolved_arguments.push(self.resolve_expression(arg)?);
                                }

                                // Return as a static method call
                                return Ok(ResolvedHirExpression::StaticMethodCall {
                                    namespace: vec![], // Two-level call
                                    class_name: namespace.clone(),
                                    class_symbol_id: symbol_id,
                                    method: function.clone(),
                                    method_symbol_id: method_id,
                                    arguments: resolved_arguments,
                                    location: location.clone(),
                                });
                            } else {
                                return {
                                    self.error(
                                        &format!(
                                            "Static method '{}' not found in class '{}'",
                                            function, namespace
                                        ),
                                        location.clone(),
                                    );
                                    Err(())
                                };
                            }
                        }
                        Some(_) => {
                            // This is actually a method call on a variable, not a namespace call
                            // Create a variable expression for the receiver
                            let receiver = ResolvedHirExpression::Variable {
                                name: namespace.clone(),
                                symbol_id,
                                location: location.clone(),
                            };

                            // Resolve arguments
                            let mut resolved_arguments = Vec::new();
                            for arg in arguments {
                                resolved_arguments.push(self.resolve_expression(arg)?);
                            }

                            // Return as a method call
                            return Ok(ResolvedHirExpression::MethodCall {
                                receiver: Box::new(receiver),
                                method: function.clone(),
                                method_symbol_id: None, // Will be resolved based on receiver type
                                arguments: resolved_arguments,
                                location: location.clone(),
                            });
                        }
                        None => {
                            // Symbol not found, continue to error handling below
                        }
                    }
                }

                // Original namespace call logic for true namespaces (like math.sin)
                // NOTE: Use dot notation to match stdlib function registration
                // stdlib registers as "string.length", not "string_length"
                let qualified_name = format!("{}.{}", namespace, function);

                // Try to look up the qualified function name in the symbol table
                // CRITICAL: Stdlib functions (string.length, math.max, etc.) are NOT in the symbol table
                // They're registered directly in MirCodeGenerator, so we use a placeholder SymbolId
                let function_symbol_id =
                    self.symbol_table.lookup_symbol(&qualified_name).unwrap_or({
                        // Use SymbolId(0) as placeholder for stdlib namespace functions
                        // The actual function lookup will happen during code generation
                        SymbolId(0)
                    });

                // Resolve all arguments
                let mut resolved_arguments = Vec::new();
                for arg in arguments {
                    resolved_arguments.push(self.resolve_expression(arg)?);
                }

                // Return as a regular function call
                Ok(ResolvedHirExpression::Call {
                    function: qualified_name,
                    function_symbol_id,
                    arguments: resolved_arguments,
                    location: location.clone(),
                })
            }

            HirExpression::StaticMethodCall {
                namespace,
                class_name,
                method,
                arguments,
                location,
            } => {
                // Handle namespace.class.method() calls (e.g., compare.integer.greaterThan)
                let full_class_name = if !namespace.is_empty() {
                    format!("{}.{}", namespace.join("."), class_name)
                } else {
                    class_name.clone()
                };

                // Look up the class symbol
                let class_symbol_id = self
                    .symbol_table
                    .lookup_symbol(&full_class_name)
                    .unwrap_or(SymbolId(0)); // Placeholder for built-in classes

                let method_symbol_id = SymbolId(0); // Placeholder for built-in methods

                // Resolve arguments
                let mut resolved_arguments = Vec::new();
                for arg in arguments {
                    resolved_arguments.push(self.resolve_expression(arg)?);
                }

                Ok(ResolvedHirExpression::StaticMethodCall {
                    namespace: namespace.clone(),
                    class_name: class_name.clone(),
                    class_symbol_id,
                    method: method.clone(),
                    method_symbol_id,
                    arguments: resolved_arguments,
                    location: location.clone(),
                })
            }

            HirExpression::OnError {
                expression,
                fallback,
                location,
            } => {
                let resolved_expression = self.resolve_expression(expression)?;
                let resolved_fallback = self.resolve_expression(fallback)?;

                Ok(ResolvedHirExpression::OnError {
                    expression: Box::new(resolved_expression),
                    fallback: Box::new(resolved_fallback),
                    location: location.clone(),
                })
            }

            HirExpression::Conditional {
                condition,
                then_expr,
                else_expr,
                location,
            } => {
                let resolved_condition = self.resolve_expression(condition)?;
                let resolved_then = self.resolve_expression(then_expr)?;
                let resolved_else = self.resolve_expression(else_expr)?;

                Ok(ResolvedHirExpression::Conditional {
                    condition: Box::new(resolved_condition),
                    then_expr: Box::new(resolved_then),
                    else_expr: Box::new(resolved_else),
                    location: location.clone(),
                })
            }

            HirExpression::BaseCall {
                arguments,
                location,
            } => {
                // Resolve the parent class symbol from the current class
                let parent_class_symbol_id = if let Some(current_class_id) = self.current_class {
                    if let Some(class_symbol) = self.symbol_table.get_symbol(current_class_id) {
                        if let SymbolKind::Class { parent, .. } = &class_symbol.kind {
                            parent.ok_or_else(|| {
                                self.error(
                                    "base() can only be called in a derived class constructor",
                                    location.clone(),
                                );
                            })?
                        } else {
                            self.error(
                                "base() can only be called inside a class",
                                location.clone(),
                            );
                            return Err(());
                        }
                    } else {
                        self.error("Current class symbol not found", location.clone());
                        return Err(());
                    }
                } else {
                    self.error(
                        "base() can only be called inside a class constructor",
                        location.clone(),
                    );
                    return Err(());
                };

                // Resolve arguments
                let mut resolved_arguments = Vec::new();
                for arg in arguments {
                    resolved_arguments.push(self.resolve_expression(arg)?);
                }

                Ok(ResolvedHirExpression::BaseCall {
                    parent_class_symbol_id,
                    arguments: resolved_arguments,
                    location: location.clone(),
                })
            }

            HirExpression::Range {
                start,
                end,
                step,
                inclusive,
                location,
            } => {
                // Resolve start and end expressions
                let resolved_start = Box::new(self.resolve_expression(start)?);
                let resolved_end = Box::new(self.resolve_expression(end)?);
                let resolved_step = if let Some(s) = step {
                    Some(Box::new(self.resolve_expression(s)?))
                } else {
                    None
                };

                Ok(ResolvedHirExpression::Range {
                    start: resolved_start,
                    end: resolved_end,
                    step: resolved_step,
                    inclusive: *inclusive,
                    location: location.clone(),
                })
            }
        }
    }

    /// Resolve an L-value
    fn resolve_lvalue(&mut self, lvalue: &HirLValue) -> Result<ResolvedHirLValue, ()> {
        match lvalue {
            HirLValue::Variable { name, location } => {
                // If we're in a class method, check for class fields first (implicit field access)
                if let Some(current_class_id) = self.current_class {
                    if let Some(class_symbol) = self.symbol_table.get_symbol(current_class_id) {
                        if let SymbolKind::Class { fields, parent, .. } = &class_symbol.kind {
                            tracing::debug!(
                                "resolve_lvalue: Looking for field '{}' in class {} ({:?}), fields: {:?}",
                                name, class_symbol.name, current_class_id, fields
                            );
                            // Check current class fields
                            for &field_id in fields {
                                if let Some(field_symbol) = self.symbol_table.get_symbol(field_id) {
                                    tracing::debug!(
                                        "  Checking field {:?}: name='{}' vs target='{}'",
                                        field_id,
                                        field_symbol.name,
                                        name
                                    );
                                    if field_symbol.name == *name {
                                        tracing::debug!("  MATCH FOUND - returning FieldAccess");
                                        // Convert variable assignment to field assignment
                                        return Ok(ResolvedHirLValue::FieldAccess {
                                            object: Box::new(ResolvedHirExpression::This {
                                                class_symbol_id: current_class_id,
                                                location: location.clone(),
                                            }),
                                            field: name.clone(),
                                            field_symbol_id: field_id,
                                            location: location.clone(),
                                        });
                                    }
                                }
                            }

                            // Check parent class fields if inheritance is involved
                            if let Some(parent_class_id) = parent {
                                if let Some(parent_symbol) =
                                    self.symbol_table.get_symbol(*parent_class_id)
                                {
                                    if let SymbolKind::Class {
                                        fields: parent_fields,
                                        ..
                                    } = &parent_symbol.kind
                                    {
                                        for &parent_field_id in parent_fields {
                                            if let Some(parent_field_symbol) =
                                                self.symbol_table.get_symbol(parent_field_id)
                                            {
                                                if parent_field_symbol.name == *name {
                                                    // Convert variable assignment to inherited field assignment
                                                    return Ok(ResolvedHirLValue::FieldAccess {
                                                        object: Box::new(
                                                            ResolvedHirExpression::This {
                                                                class_symbol_id: current_class_id,
                                                                location: location.clone(),
                                                            },
                                                        ),
                                                        field: name.clone(),
                                                        field_symbol_id: parent_field_id,
                                                        location: location.clone(),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // If not a field, try to find the variable in normal scope
                if let Some(symbol_id) = self.symbol_table.lookup_symbol(name) {
                    return Ok(ResolvedHirLValue::Variable {
                        name: name.clone(),
                        symbol_id,
                        location: location.clone(),
                    });
                }

                // If still not found, report error (SCOPE001: UseBeforeDeclaration)
                self.error_with_code(
                    &format!("Variable '{}' not found", name),
                    "SCOPE001",
                    location.clone(),
                );
                Err(())
            }

            HirLValue::FieldAccess {
                object,
                field,
                location,
            } => {
                let resolved_object = self.resolve_expression(object)?;

                // Field resolution depends on object type - for now use placeholder
                let field_symbol_id = SymbolId(0);

                Ok(ResolvedHirLValue::FieldAccess {
                    object: Box::new(resolved_object),
                    field: field.clone(),
                    field_symbol_id,
                    location: location.clone(),
                })
            }

            HirLValue::Index {
                array,
                index,
                location,
            } => {
                let resolved_array = self.resolve_expression(array)?;
                let resolved_index = self.resolve_expression(index)?;

                Ok(ResolvedHirLValue::Index {
                    array: Box::new(resolved_array),
                    index: Box::new(resolved_index),
                    location: location.clone(),
                })
            }
        }
    }

    /// Report an error
    fn error(&mut self, message: &str, location: SourceLocation) {
        self.errors
            .push(CompilerError::validation_error(message, location));
    }

    /// Report an error with a specific error code (e.g. "SCOPE001").
    fn error_with_code(&mut self, message: &str, code: &str, location: SourceLocation) {
        self.errors.push(CompilerError::Validation {
            context: Box::new(
                crate::error::ErrorContext::new(
                    message,
                    None,
                    crate::error::ErrorType::Validation,
                    Some(location),
                )
                .with_error_code(code),
            ),
        });
    }

    /// Report a warning
    #[allow(dead_code)]
    fn warning(&mut self, message: &str, location: SourceLocation) {
        self.warnings
            .push(CompilerError::validation_warning(message, location));
    }

    /// Return the name of the current access context for SEM005 checks.
    ///
    /// - When inside a class body, returns the class name.
    /// - When at module scope (no current class), returns `"<module>"`.
    ///
    /// This value is compared against `Symbol::owner_scope_name` to decide
    /// whether a private-symbol access is permitted.
    #[allow(dead_code)]
    fn current_class_name(&self) -> String {
        if let Some(class_id) = self.current_class {
            if let Some(symbol) = self.symbol_table.get_symbol(class_id) {
                return symbol.name.clone();
            }
        }
        "<module>".to_string()
    }

    /// Register builtin functions in the symbol table to allow validation
    /// This is a simplified version that registers commonly used builtins
    /// Full validation is done in the semantic analyzer
    fn register_builtin_functions(&mut self) {
        // Create a dummy source location for builtin functions
        let builtin_location = SourceLocation {
            line: 0,
            column: 0,
            file: "<builtin>".to_string(),
            byte_start: None,
            byte_end: None,
        };

        // Common IO functions
        self.register_builtin_fn(
            "print",
            vec![HirType::String],
            Some(HirType::Void),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "println",
            vec![HirType::String],
            Some(HirType::Void),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "printl",
            vec![HirType::String],
            Some(HirType::Void),
            builtin_location.clone(),
        );

        // Conversion functions
        self.register_builtin_fn(
            "bool_to_string",
            vec![HirType::Boolean],
            Some(HirType::String),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "int_to_string",
            vec![HirType::Integer],
            Some(HirType::String),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "float_to_string",
            vec![HirType::Number],
            Some(HirType::String),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "number_to_string",
            vec![HirType::Number],
            Some(HirType::String),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "to_number",
            vec![HirType::String],
            Some(HirType::Number),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "to_integer",
            vec![HirType::Number],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // Testing functions
        self.register_builtin_fn(
            "mustBeTrue",
            vec![HirType::Boolean],
            Some(HirType::Void),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "mustBeFalse",
            vec![HirType::Boolean],
            Some(HirType::Void),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "assertEqual",
            vec![HirType::Integer, HirType::Integer],
            Some(HirType::Void),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "assertNotEqual",
            vec![HirType::Integer, HirType::Integer],
            Some(HirType::Void),
            builtin_location.clone(),
        );

        // List namespace functions (underscore versions for namespace resolution)
        // Using Integer as generic placeholder - type checker handles actual generic list typing
        self.register_builtin_fn(
            "list_add",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::Integer],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_get",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_set",
            vec![
                HirType::List(Box::new(HirType::Integer)),
                HirType::Integer,
                HirType::Integer,
            ],
            Some(HirType::Void),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_clear",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Void),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_sort",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_reverse",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_contains",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::Integer],
            Some(HirType::Boolean),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_remove",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_size",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_length",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_isEmpty",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Boolean),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_isNotEmpty",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Boolean),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_peek",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_indexOf",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_index_of",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_lastIndexOf",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_insert",
            vec![
                HirType::List(Box::new(HirType::Integer)),
                HirType::Integer,
                HirType::Integer,
            ],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_slice",
            vec![
                HirType::List(Box::new(HirType::Integer)),
                HirType::Integer,
                HirType::Integer,
            ],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_concat",
            vec![
                HirType::List(Box::new(HirType::Integer)),
                HirType::List(Box::new(HirType::Integer)),
            ],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_first",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_last",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_join",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::String],
            Some(HirType::String),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_fill",
            vec![HirType::Integer, HirType::Integer],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_range",
            vec![HirType::Integer, HirType::Integer],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_push",
            vec![HirType::List(Box::new(HirType::Integer)), HirType::Integer],
            Some(HirType::List(Box::new(HirType::Integer))),
            builtin_location.clone(),
        );
        self.register_builtin_fn(
            "list_pop",
            vec![HirType::List(Box::new(HirType::Integer))],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // Validator namespace functions
        // validator.create() -> Integer (pointer to validation rules)
        self.register_builtin_fn(
            "validator.create",
            vec![],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.ok(value: Integer) -> Integer (ValidationResult pointer)
        self.register_builtin_fn(
            "validator.ok",
            vec![HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.error(errors: Integer) -> Integer (ValidationResult pointer)
        self.register_builtin_fn(
            "validator.error",
            vec![HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.isOk(result: Integer) -> Boolean
        self.register_builtin_fn(
            "validator.isOk",
            vec![HirType::Integer],
            Some(HirType::Boolean),
            builtin_location.clone(),
        );

        // validator.isError(result: Integer) -> Boolean
        self.register_builtin_fn(
            "validator.isError",
            vec![HirType::Integer],
            Some(HirType::Boolean),
            builtin_location.clone(),
        );

        // validator.getValue(result: Integer) -> Any (the validated input data)
        self.register_builtin_fn(
            "validator.getValue",
            vec![HirType::Integer],
            Some(HirType::Any),
            builtin_location.clone(),
        );

        // validator.getErrors(result: Integer) -> List<String> (error messages)
        self.register_builtin_fn(
            "validator.getErrors",
            vec![HirType::Integer],
            Some(HirType::List(Box::new(HirType::String))),
            builtin_location.clone(),
        );

        // --- Schema builder functions (used by validate block desugaring) ---

        // validator.createWithName(name: String) -> Integer (rules pointer)
        self.register_builtin_fn(
            "validator.createWithName",
            vec![HirType::String],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.run(rules: Integer, input: Any) -> Integer (result pointer)
        // The input can be any data structure (pairs, list, etc.)
        self.register_builtin_fn(
            "validator.run",
            vec![HirType::Integer, HirType::Any],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.field(rules: Integer, fieldName: String) -> Integer
        self.register_builtin_fn(
            "validator.field",
            vec![HirType::Integer, HirType::String],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.type(rules: Integer, typeName: String) -> Integer
        self.register_builtin_fn(
            "validator.type",
            vec![HirType::Integer, HirType::String],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.required(rules: Integer, flag: Integer) -> Integer
        self.register_builtin_fn(
            "validator.required",
            vec![HirType::Integer, HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.trim(rules: Integer) -> Integer
        self.register_builtin_fn(
            "validator.trim",
            vec![HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.minLength(rules: Integer, min: Integer) -> Integer
        self.register_builtin_fn(
            "validator.minLength",
            vec![HirType::Integer, HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.maxLength(rules: Integer, max: Integer) -> Integer
        self.register_builtin_fn(
            "validator.maxLength",
            vec![HirType::Integer, HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.range(rules: Integer, min: Integer, max: Integer) -> Integer
        self.register_builtin_fn(
            "validator.range",
            vec![HirType::Integer, HirType::Integer, HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.match(rules: Integer, pattern: String) -> Integer
        self.register_builtin_fn(
            "validator.match",
            vec![HirType::Integer, HirType::String],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.allowedValues(rules: Integer, values: Integer) -> Integer
        self.register_builtin_fn(
            "validator.allowedValues",
            vec![HirType::Integer, HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.custom(rules: Integer, fn: Any) -> Integer
        // The custom function reference can be any callable (function reference)
        self.register_builtin_fn(
            "validator.custom",
            vec![HirType::Integer, HirType::Any],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.message(rules: Integer, text: String) -> Integer
        self.register_builtin_fn(
            "validator.message",
            vec![HirType::Integer, HirType::String],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // State reset bridge functions — generated by HIR builder for `reset` statements
        self.register_builtin_fn(
            "_state_reset_all",
            vec![],
            Some(HirType::Void),
            builtin_location.clone(),
        );
        // _state_reset_named takes the variable name as a string argument (emitted as base ptr by codegen)
        self.register_builtin_fn(
            "_state_reset_named",
            vec![HirType::String],
            Some(HirType::Void),
            builtin_location.clone(),
        );
    }

    /// Helper to register a single builtin function
    fn register_builtin_fn(
        &mut self,
        name: &str,
        parameters: Vec<HirType>,
        return_type: Option<HirType>,
        location: SourceLocation,
    ) {
        let symbol_id = self.symbol_table.create_symbol(
            name.to_string(),
            SymbolKind::Function {
                parameters,
                return_type,
            },
            self.symbol_table.current_scope_id(),
            location,
        );
        // Mark the symbol as builtin so it gets registered in the type environment
        self.symbol_table.mark_as_builtin(symbol_id);
    }

    /// Resolve names in a HIR program with plugin bridge functions
    ///
    /// This allows plugins to declare bridge functions in their plugin.toml
    /// that will be recognized by the compiler during name resolution.
    /// Bridge functions are applied AFTER builtin registration (inside
    /// `resolve_program`) so their signatures are never overwritten.
    pub fn resolve_with_bridge_functions(
        hir: HirProgram,
        bridge_functions: &[crate::plugins::BridgeFunction],
    ) -> Result<ResolutionResult, Vec<CompilerError>> {
        let mut resolver = Self::new();

        // Defer bridge registration — applied inside resolve_program after builtins
        resolver.pending_bridge_functions = bridge_functions.to_vec();

        match resolver.resolve_program(hir) {
            Ok(resolved_hir) => Ok(ResolutionResult {
                resolved_hir,
                warnings: resolver.warnings,
            }),
            Err(_) => Err(resolver.errors),
        }
    }

    /// Resolve with both bridge functions and language-name aliases.
    ///
    /// Extends `resolve_with_bridge_functions` by also registering dot-notation
    /// language API names (e.g. `"db.query"`) as builtin functions with the same
    /// signature as the underlying bridge function they map to.
    /// Both bridge functions and aliases are applied AFTER builtin registration
    /// (inside `resolve_program`) so plugin signatures are never overwritten.
    pub fn resolve_with_bridge_and_language_aliases(
        hir: crate::hir::HirProgram,
        bridge_functions: &[crate::plugins::BridgeFunction],
        language_to_bridge: &std::collections::HashMap<String, String>,
    ) -> Result<ResolutionResult, Vec<CompilerError>> {
        let mut resolver = Self::new();

        // Defer both bridge and alias registration — applied inside resolve_program after builtins
        resolver.pending_bridge_functions = bridge_functions.to_vec();
        resolver.pending_language_aliases = language_to_bridge.clone();

        match resolver.resolve_program(hir) {
            Ok(resolved_hir) => Ok(ResolutionResult {
                resolved_hir,
                warnings: resolver.warnings,
            }),
            Err(_) => Err(resolver.errors),
        }
    }

    /// Register language-name aliases (dot-notation API names) as builtins.
    ///
    /// For each `(lang_name, bridge_name)` pair in `language_to_bridge`, looks
    /// up the bridge function's parameter/return types and registers `lang_name`
    /// as a builtin function with identical types.  This lets the resolver accept
    /// calls like `db.query(...)` without emitting "unknown function" errors.
    fn register_language_function_aliases(
        &mut self,
        language_to_bridge: &std::collections::HashMap<String, String>,
        bridge_by_name: &std::collections::HashMap<&str, &crate::plugins::BridgeFunction>,
    ) {
        use crate::builtins::registry::BuiltinType;

        let builtin_location = SourceLocation {
            line: 0,
            column: 0,
            file: "<plugin-language>".to_string(),
            byte_start: None,
            byte_end: None,
        };

        for (lang_name, bridge_name) in language_to_bridge {
            if let Some(bf) = bridge_by_name.get(bridge_name.as_str()) {
                let lang_def = self.pending_language_fn_defs.get(lang_name.as_str());

                // Use the language def's param types if declared, else bridge params
                let parameters: Vec<HirType> = if let Some(def) = lang_def {
                    if let Some(ref param_types) = def.params {
                        param_types
                            .iter()
                            .map(|s| Self::parse_bridge_hir_type(s))
                            .collect()
                    } else {
                        bf.get_param_types()
                            .iter()
                            .map(Self::builtin_type_to_hir_type)
                            .collect()
                    }
                } else {
                    bf.get_param_types()
                        .iter()
                        .map(Self::builtin_type_to_hir_type)
                        .collect()
                };

                // Use the language def's return type if declared, else bridge return
                let return_type = if let Some(def) = lang_def {
                    if let Some(ref ret_str) = def.returns {
                        let ht = Self::parse_bridge_hir_type(ret_str);
                        if matches!(ht, HirType::Void) {
                            None
                        } else {
                            Some(ht)
                        }
                    } else {
                        let ret = bf.get_return_type();
                        match ret {
                            BuiltinType::Void => None,
                            _ => Some(Self::builtin_type_to_hir_type(&ret)),
                        }
                    }
                } else {
                    let ret = bf.get_return_type();
                    match ret {
                        BuiltinType::Void => None,
                        _ => Some(Self::builtin_type_to_hir_type(&ret)),
                    }
                };

                // Extract param_defaults now while lang_def borrow is still live,
                // before the mutable borrow from register_builtin_fn.
                let param_defaults_opt: Option<Vec<String>> = lang_def
                    .filter(|def| !def.param_defaults.is_empty())
                    .map(|def| def.param_defaults.clone());

                // Skip if this name already refers to a builtin namespace.
                // A plugin language alias (e.g. frame.server's `json -> _res_json`) must
                // not overwrite the `json` built-in namespace — doing so would cause
                // `json.get(...)` calls in shared modules to be misresolved as method
                // calls on a variable instead of namespace calls (SEM001).
                // Direct `json(data)` calls still work through the call-site rewrite at
                // the `function == "json"` branch in the Call handler above.
                if let Some(existing_id) = self
                    .symbol_table
                    .lookup_symbol_in_scope(lang_name, ScopeId(0))
                {
                    if let Some(existing_sym) = self.symbol_table.get_symbol(existing_id) {
                        if matches!(existing_sym.kind, SymbolKind::Namespace { .. }) {
                            tracing::debug!(
                                lang_name = %lang_name,
                                bridge_name = %bridge_name,
                                "Skipping language alias — would shadow builtin namespace"
                            );
                            continue;
                        }
                    }
                }

                tracing::debug!(
                    lang_name = %lang_name,
                    bridge_name = %bridge_name,
                    params = ?parameters,
                    returns = ?return_type,
                    "Registering language-name alias in resolver"
                );

                self.register_builtin_fn(
                    lang_name,
                    parameters,
                    return_type,
                    builtin_location.clone(),
                );

                // Store param_defaults so call-site injection can fill in missing optional args.
                if let Some(defaults) = param_defaults_opt {
                    self.language_fn_defaults
                        .insert(lang_name.clone(), defaults);
                }
            }
        }

        // Register namespace identifiers for plugin dot-notation prefixes
        // e.g., if we have "req.query", "req.body", register "req" as a Namespace
        let mut namespace_functions: std::collections::HashMap<String, Vec<SymbolId>> =
            std::collections::HashMap::new();
        for lang_name in language_to_bridge.keys() {
            if let Some(dot_pos) = lang_name.find('.') {
                let ns_name = &lang_name[..dot_pos];
                let func_symbol_id = self
                    .symbol_table
                    .lookup_symbol(lang_name)
                    .unwrap_or(SymbolId(0));
                namespace_functions
                    .entry(ns_name.to_string())
                    .or_default()
                    .push(func_symbol_id);
            }
        }

        for (ns_name, functions) in namespace_functions {
            // Skip if already registered as a builtin namespace
            if self
                .symbol_table
                .lookup_symbol_in_scope(&ns_name, ScopeId(0))
                .is_some()
            {
                continue;
            }

            let ns_id = self.symbol_table.create_symbol(
                ns_name.clone(),
                SymbolKind::Namespace { functions },
                ScopeId(0),
                builtin_location.clone(),
            );
            self.symbol_table.builtins.insert(ns_id);
            tracing::debug!(
                namespace = %ns_name,
                "Registered plugin namespace in resolver"
            );
        }
    }

    /// Register plugin bridge functions as builtins
    ///
    /// Bridge functions are declared in plugin.toml and provide runtime
    /// functionality that the compiler needs to recognize during name resolution.
    fn register_plugin_bridge_functions(
        &mut self,
        bridge_functions: &[crate::plugins::BridgeFunction],
    ) {
        use crate::builtins::registry::BuiltinType;

        let builtin_location = SourceLocation {
            line: 0,
            column: 0,
            file: "<plugin-bridge>".to_string(),
            byte_start: None,
            byte_end: None,
        };

        for func in bridge_functions {
            // Convert BuiltinType to HirType for parameters
            let parameters: Vec<HirType> = func
                .get_param_types()
                .iter()
                .map(Self::builtin_type_to_hir_type)
                .collect();

            // Convert return type
            let return_type = {
                let ret = func.get_return_type();
                match ret {
                    BuiltinType::Void => None,
                    _ => Some(Self::builtin_type_to_hir_type(&ret)),
                }
            };

            // PLUGIN002: detect signature mismatch with an already-registered function.
            // When two plugins declare the same bridge name with different parameter
            // counts, the second registration would silently shadow the first. Emit an
            // error instead so the developer can reconcile the conflict.
            if let Some(existing_id) = self.symbol_table.lookup_symbol(&func.name) {
                if let Some(existing_sym) = self.symbol_table.get_symbol(existing_id) {
                    if let SymbolKind::Function {
                        parameters: ref existing_params,
                        ..
                    } = existing_sym.kind
                    {
                        if existing_params.len() != parameters.len() {
                            self.errors.push(CompilerError::Validation {
                                context: Box::new(
                                    crate::error::ErrorContext::new(
                                        format!(
                                            "PLUGIN002: Bridge function '{}' is declared with {} parameter(s) \
                                             in plugin.toml but a conflicting declaration with {} parameter(s) \
                                             is already registered. Reconcile the signatures.",
                                            func.name,
                                            parameters.len(),
                                            existing_params.len()
                                        ),
                                        Some(format!(
                                            "Check all plugin.toml [bridge] sections that declare '{}'",
                                            func.name
                                        )),
                                        crate::error::ErrorType::Validation,
                                        None,
                                    )
                                    .with_error_code("PLUGIN002"),
                                ),
                            });
                            continue;
                        }
                    }
                }
            }

            tracing::debug!(
                name = %func.name,
                params = ?parameters,
                returns = ?return_type,
                "Registering plugin bridge function in resolver"
            );

            self.register_builtin_fn(
                &func.name,
                parameters,
                return_type,
                builtin_location.clone(),
            );
        }
    }

    /// Convert BuiltinType to HirType
    fn builtin_type_to_hir_type(bt: &crate::builtins::registry::BuiltinType) -> HirType {
        use crate::builtins::registry::BuiltinType;
        match bt {
            BuiltinType::Integer => HirType::Integer,
            BuiltinType::Number => HirType::Number,
            BuiltinType::String => HirType::String,
            BuiltinType::Boolean => HirType::Boolean,
            BuiltinType::Void => HirType::Void,
            BuiltinType::List(inner) => {
                HirType::List(Box::new(Self::builtin_type_to_hir_type(inner)))
            }
            BuiltinType::Matrix(inner) => {
                HirType::Matrix(Box::new(Self::builtin_type_to_hir_type(inner)))
            }
            BuiltinType::Pairs(k, v) => HirType::Pairs(
                Box::new(Self::builtin_type_to_hir_type(k)),
                Box::new(Self::builtin_type_to_hir_type(v)),
            ),
            BuiltinType::Namespace => HirType::Integer, // Namespace is internal, use Integer as placeholder
            BuiltinType::Any => HirType::Any,           // Any type for dynamic/JSON values
            BuiltinType::Handler => HirType::Integer,   // Handler is an i32 index at WASM level
        }
    }

    /// Parse a type string from plugin.toml into a `HirType`.
    fn parse_bridge_hir_type(s: &str) -> HirType {
        match s.to_lowercase().as_str() {
            "string" => HirType::String,
            "integer" | "int" | "i32" | "i64" => HirType::Integer,
            "number" | "float" | "f64" | "f32" => HirType::Number,
            "boolean" | "bool" => HirType::Boolean,
            "void" | "()" => HirType::Void,
            "any" => HirType::Any,
            _ => HirType::Any,
        }
    }

    /// Like `resolve_with_bridge_and_language_aliases` but also accepts the
    /// language function definitions so that return-type and param-type overrides
    /// declared in `[language].functions` of `plugin.toml` are honoured.
    pub fn resolve_with_bridge_aliases_and_fn_defs(
        hir: crate::hir::HirProgram,
        bridge_functions: &[crate::plugins::BridgeFunction],
        language_to_bridge: &std::collections::HashMap<String, String>,
        language_fn_defs: std::collections::HashMap<
            String,
            crate::plugins::plugin_abi::PluginFunctionDef,
        >,
    ) -> Result<ResolutionResult, Vec<CompilerError>> {
        let mut resolver = Self::new();
        resolver.pending_bridge_functions = bridge_functions.to_vec();
        resolver.pending_language_aliases = language_to_bridge.clone();
        resolver.pending_language_fn_defs = language_fn_defs;

        match resolver.resolve_program(hir) {
            Ok(resolved_hir) => Ok(ResolutionResult {
                resolved_hir,
                warnings: resolver.warnings,
            }),
            Err(_) => Err(resolver.errors),
        }
    }
}

impl Default for NameResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    fn loc() -> SourceLocation {
        SourceLocation {
            line: 1,
            column: 1,
            file: "<test>".to_string(),
            byte_start: None,
            byte_end: None,
        }
    }

    fn empty_block() -> HirBlock {
        HirBlock {
            statements: vec![],
            location: loc(),
        }
    }

    /// Build a minimal HirProgram with one screen block containing a state variable,
    /// and a start function that tries to reference that state variable.
    /// Resolution should fail with SCOPE005.
    fn make_scope005_program(access_in_start: bool) -> HirProgram {
        let screen_state = HirStateBlock {
            declarations: vec![HirStateDeclaration {
                name: "homeCount".to_string(),
                state_type: HirType::Integer,
                initializer: HirExpression::Literal {
                    value: crate::ast::Value::Integer(0),
                    location: loc(),
                },
                guard: None,
                is_private: false,
                location: loc(),
            }],
            computed: vec![],
            rules: vec![],
            scope: HirStateScope::Screen,
            location: loc(),
        };

        let screen = HirScreenBlock {
            name: "Home".to_string(),
            state: Some(screen_state),
            watch_blocks: vec![],
            functions: vec![],
            location: loc(),
        };

        // The start function body: if access_in_start is true, it references homeCount.
        let start_body = if access_in_start {
            HirBlock {
                statements: vec![HirStatement::VariableDeclaration {
                    name: "x".to_string(),
                    var_type: HirType::Integer,
                    initializer: Some(HirExpression::Variable {
                        name: "homeCount".to_string(),
                        location: loc(),
                    }),
                    is_mutable: true,
                    location: loc(),
                }],
                location: loc(),
            }
        } else {
            empty_block()
        };

        HirProgram {
            functions: vec![],
            classes: vec![],
            start_function: Some(HirFunction {
                name: "start".to_string(),
                parameters: vec![],
                return_type: None,
                body: start_body,
                is_start: true,
                is_private: false,
                owner_screen: None,
                location: loc(),
            }),
            imports: vec![],
            tests: vec![],
            state: None,
            watch_blocks: vec![],
            externals: vec![],
            screen_blocks: vec![screen],
            location: loc(),
        }
    }

    #[test]
    fn scope005_rejects_screen_state_access_from_start() {
        let hir = make_scope005_program(true);
        let result = NameResolver::resolve(hir);
        assert!(
            result.is_err(),
            "Expected SCOPE005 error but resolution succeeded"
        );
        let errors = result.unwrap_err();
        let has_scope005 = errors.iter().any(|e| {
            e.message().contains("homeCount")
                && (e.message().contains("local to screen") || e.message().contains("Home"))
        });
        assert!(has_scope005, "Expected SCOPE005 message, got: {:?}", errors);
    }

    #[test]
    fn scope005_allows_empty_start_with_screen_state() {
        let hir = make_scope005_program(false);
        let result = NameResolver::resolve(hir);
        assert!(
            result.is_ok(),
            "Unexpected errors: {:?}",
            result.unwrap_err()
        );
    }
}
