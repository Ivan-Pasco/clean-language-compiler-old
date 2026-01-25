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
    errors: Vec<CompilerError>,
    warnings: Vec<CompilerError>,
    expression_recursion_depth: usize,
}

#[allow(dead_code)]
impl NameResolver {
    /// Create a new name resolver
    pub fn new() -> Self {
        Self {
            symbol_table: GlobalSymbolTable::new(),
            current_class: None,
            current_function: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            expression_recursion_depth: 0,
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
        // First pass: Register all top-level symbols
        self.register_top_level_symbols(&hir)?;

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

        // Resolve external functions (WASM imports)
        let resolved_externals = self.resolve_externals(&hir.externals)?;

        Ok(ResolvedHirProgram {
            functions: resolved_functions,
            classes: resolved_classes,
            start_function: resolved_start_function,
            imports: resolved_imports,
            tests: resolved_tests,
            state: resolved_state,
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
                let _symbol_id = self.symbol_table.create_symbol(
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

                // CRITICAL FIX: Register constructor symbol in first pass
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
                    let _symbol_id = self.symbol_table.create_symbol(
                        state_decl.name.clone(),
                        SymbolKind::StateVariable {
                            var_type: state_decl.state_type.clone(),
                            scope: state_block.scope,
                            has_guard: state_decl.guard.is_some(),
                        },
                        self.symbol_table.current_scope_id(),
                        state_decl.location.clone(),
                    );
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
                            has_guard: false, // Computed state doesn't have guards
                        },
                        self.symbol_table.current_scope_id(),
                        computed_decl.location.clone(),
                    );
                }
            }
        }

        // Register external functions (WASM imports)
        // External functions are treated like builtins - they have no body in Clean code
        for external in &hir.externals {
            if self
                .symbol_table
                .has_symbol_in_current_scope(&external.name)
            {
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

        // Resolve function body
        let resolved_body = self.resolve_block(&function.body)?;

        // Exit function scope
        self.symbol_table.exit_scope();
        self.current_function = None;

        Ok(ResolvedHirFunction {
            name: function.name,
            symbol_id: function_symbol_id,
            parameters: resolved_parameters,
            return_type: function.return_type,
            body: resolved_body,
            is_start: function.is_start,
            is_async: false, // Async detection handled by runtime analysis
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
            // CRITICAL FIX: Generate default constructor with empty body
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

        // Resolve methods
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

            method_symbol_ids.push(method_symbol_id);

            let resolved_method =
                self.resolve_method(method.clone(), class_symbol_id, method_symbol_id)?;
            resolved_methods.push(resolved_method);
        }

        // Update class symbol with fields and methods
        if let Some(class_symbol) = self.symbol_table.get_symbol_mut(class_symbol_id) {
            if let SymbolKind::Class {
                fields,
                methods,
                parent,
            } = &mut class_symbol.kind
            {
                *fields = field_symbol_ids.clone();
                *methods = method_symbol_ids;
                *parent = parent_symbol_id;
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
            // For now, create a placeholder module
            let module_id = self.symbol_table.create_module(
                import.module_name.clone(),
                format!("{}.cln", import.module_name),
            );

            let resolved_items = if let Some(items) = &import.items {
                let mut resolved_items = Vec::new();
                for item in items {
                    // For now, create placeholder symbols for imported items
                    // In a full implementation, these would be resolved from the actual module
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

        Ok(ResolvedHirStateBlock {
            declarations: resolved_declarations,
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

                let symbol_id = self.symbol_table.create_symbol(
                    name.clone(),
                    SymbolKind::Variable {
                        var_type: var_type.clone(),
                        is_mutable: *is_mutable,
                    },
                    self.symbol_table.current_scope_id(),
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

            HirStatement::Break { location } => Ok(ResolvedHirStatement::Break {
                location: location.clone(),
            }),

            HirStatement::Continue { location } => Ok(ResolvedHirStatement::Continue {
                location: location.clone(),
            }),
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

        // Debug output disabled for performance
        // eprintln!("DEBUG RESOLVER: Resolving expression at depth {}: {:?}", self.expression_recursion_depth, expression);

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
                    return Ok(ResolvedHirExpression::Variable {
                        name: name.clone(),
                        symbol_id,
                        location: location.clone(),
                    });
                }

                // If still not found, report error
                self.error(&format!("Variable '{}' not found", name), location.clone());
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

                // If not found as a global function and we're inside a class,
                // check if it's a method in the current class or parent class
                if function_symbol_opt.is_none() {
                    if let Some(current_class_id) = self.current_class {
                        // Try to find it as a method in the current class or parent
                        if let Some(method_symbol_id) = self
                            .symbol_table
                            .lookup_class_member(current_class_id, function)
                        {
                            // Found as a method! Convert to implicit this.method() call
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
                }

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
                                    .unwrap_or_else(|| {
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
                                // CRITICAL FIX: Use dot notation to match stdlib function registration
                                let qualified_name = format!("{}.{}", class_name, method);

                                // Try to look up the qualified function name in the symbol table
                                // CRITICAL: Stdlib functions (string.length, math.max, etc.) are NOT in the symbol table
                                // They're registered directly in CodeGenerator, so we use a placeholder SymbolId
                                let function_symbol_id = self
                                    .symbol_table
                                    .lookup_symbol(&qualified_name)
                                    .unwrap_or_else(|| {
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

                // CRITICAL FIX: Check if receiver is a FieldAccess that represents a namespace path
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
                // CRITICAL FIX: Check if object is a Variable that refers to a namespace
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
                // CRITICAL FIX: Handle field access chains like test.flag.toString()
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

                // CRITICAL FIX: Check if the "namespace" is actually a field (method call on field)
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

                // CRITICAL FIX: Check if the "namespace" is actually a variable (method call) or a static class method
                // This handles cases like value.toString() where 'value' is a variable, not a namespace
                // However, do NOT convert if it's a known namespace or a class (static method call)
                if let Some(symbol_id) = self.symbol_table.lookup_symbol(namespace) {
                    // Check the symbol kind to determine if it's a true namespace or class
                    let symbol_kind = if let Some(symbol) = self.symbol_table.get_symbol(symbol_id)
                    {
                        Some(symbol.kind.clone())
                    } else {
                        None
                    };

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
                                return Err(self.error(
                                    &format!(
                                        "Static method '{}' not found in class '{}'",
                                        function, namespace
                                    ),
                                    location.clone(),
                                ));
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
                // CRITICAL FIX: Use dot notation to match stdlib function registration
                // stdlib registers as "string.length", not "string_length"
                let qualified_name = format!("{}.{}", namespace, function);

                // Try to look up the qualified function name in the symbol table
                // CRITICAL: Stdlib functions (string.length, math.max, etc.) are NOT in the symbol table
                // They're registered directly in CodeGenerator, so we use a placeholder SymbolId
                let function_symbol_id = self
                    .symbol_table
                    .lookup_symbol(&qualified_name)
                    .unwrap_or_else(|| {
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

                // If still not found, report error
                self.error(&format!("Variable '{}' not found", name), location.clone());
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

    /// Report a warning
    fn warning(&mut self, message: &str, location: SourceLocation) {
        self.warnings
            .push(CompilerError::validation_warning(message, location));
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

        // HTTP server functions (internal bridge functions for Frame runtime)
        // _http_route(method: string, path: string, handler_idx: integer) -> integer
        self.register_builtin_fn(
            "_http_route",
            vec![HirType::String, HirType::String, HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        // _http_listen(port: integer) -> integer
        self.register_builtin_fn(
            "_http_listen",
            vec![HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // Request context access functions (for reading request data in handlers)
        // _req_param(name: string) -> string
        self.register_builtin_fn(
            "_req_param",
            vec![HirType::String],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _req_query(name: string) -> string
        self.register_builtin_fn(
            "_req_query",
            vec![HirType::String],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _req_body() -> string
        self.register_builtin_fn(
            "_req_body",
            vec![],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _req_header(name: string) -> string
        self.register_builtin_fn(
            "_req_header",
            vec![HirType::String],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _req_method() -> string
        self.register_builtin_fn(
            "_req_method",
            vec![],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _req_path() -> string
        self.register_builtin_fn(
            "_req_path",
            vec![],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _req_cookie(name: string) -> string
        self.register_builtin_fn(
            "_req_cookie",
            vec![HirType::String],
            Some(HirType::String),
            builtin_location.clone(),
        );

        // Protected route registration
        // _http_route_protected(method: string, path: string, handler_idx: integer, required_role: string) -> integer
        self.register_builtin_fn(
            "_http_route_protected",
            vec![
                HirType::String,
                HirType::String,
                HirType::Integer,
                HirType::String,
            ],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // Session management functions
        // _session_create(user_id: integer, role: string, claims: string) -> string
        self.register_builtin_fn(
            "_session_create",
            vec![HirType::Integer, HirType::String, HirType::String],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _session_get() -> string
        self.register_builtin_fn(
            "_session_get",
            vec![],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _session_destroy() -> integer
        self.register_builtin_fn(
            "_session_destroy",
            vec![],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        // _session_set_cookie(cookie: string) -> integer
        self.register_builtin_fn(
            "_session_set_cookie",
            vec![HirType::String],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // Authentication context functions
        // _auth_get_session() -> string
        self.register_builtin_fn(
            "_auth_get_session",
            vec![],
            Some(HirType::String),
            builtin_location.clone(),
        );
        // _auth_require_auth() -> integer
        self.register_builtin_fn(
            "_auth_require_auth",
            vec![],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        // _auth_require_role(role: string) -> integer
        self.register_builtin_fn(
            "_auth_require_role",
            vec![HirType::String],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        // _auth_can(permission: string) -> integer
        self.register_builtin_fn(
            "_auth_can",
            vec![HirType::String],
            Some(HirType::Integer),
            builtin_location.clone(),
        );
        // _auth_has_any_role(roles_json: string) -> integer
        self.register_builtin_fn(
            "_auth_has_any_role",
            vec![HirType::String],
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

        // validator.getValue(result: Integer) -> Integer
        self.register_builtin_fn(
            "validator.getValue",
            vec![HirType::Integer],
            Some(HirType::Integer),
            builtin_location.clone(),
        );

        // validator.getErrors(result: Integer) -> Integer (errors list pointer)
        self.register_builtin_fn(
            "validator.getErrors",
            vec![HirType::Integer],
            Some(HirType::Integer),
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
    pub fn resolve_with_bridge_functions(
        hir: HirProgram,
        bridge_functions: &[crate::plugins::BridgeFunction],
    ) -> Result<ResolutionResult, Vec<CompilerError>> {
        let mut resolver = Self::new();

        // Register plugin bridge functions before resolving
        resolver.register_plugin_bridge_functions(bridge_functions);

        match resolver.resolve_program(hir) {
            Ok(resolved_hir) => Ok(ResolutionResult {
                resolved_hir,
                warnings: resolver.warnings,
            }),
            Err(_) => Err(resolver.errors),
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
        };

        for func in bridge_functions {
            // Convert BuiltinType to HirType for parameters
            let parameters: Vec<HirType> = func
                .get_param_types()
                .iter()
                .map(|bt| Self::builtin_type_to_hir_type(bt))
                .collect();

            // Convert return type
            let return_type = {
                let ret = func.get_return_type();
                match ret {
                    BuiltinType::Void => None,
                    _ => Some(Self::builtin_type_to_hir_type(&ret)),
                }
            };

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
        }
    }
}

impl Default for NameResolver {
    fn default() -> Self {
        Self::new()
    }
}
