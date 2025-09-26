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
        
        Ok(ResolvedHirProgram {
            functions: resolved_functions,
            classes: resolved_classes,
            start_function: resolved_start_function,
            imports: resolved_imports,
            tests: resolved_tests,
            symbol_table: self.symbol_table.clone(),
            location: hir.location,
        })
    }
    
    /// First pass: Register all top-level symbols (functions, classes)
    fn register_top_level_symbols(&mut self, hir: &HirProgram) -> Result<(), ()> {
        // Register functions
        for function in &hir.functions {
            // Check for duplicates BEFORE creating the symbol
            if self.symbol_table.has_symbol_in_current_scope(&function.name) {
                self.error(&format!("Function '{}' is already defined", function.name), function.location.clone());
            } else {
                let symbol_id = self.symbol_table.create_symbol(
                    function.name.clone(),
                    SymbolKind::Function {
                        parameters: function.parameters.iter()
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
            if self.symbol_table.has_symbol_in_current_scope(&start_fn.name) {
                self.error(&format!("Function '{}' conflicts with start function", start_fn.name), start_fn.location.clone());
            } else {
                let symbol_id = self.symbol_table.create_symbol(
                    start_fn.name.clone(),
                    SymbolKind::Function {
                        parameters: start_fn.parameters.iter()
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
                self.error(&format!("Class '{}' is already defined", class.name), class.location.clone());
            } else {
                let symbol_id = self.symbol_table.create_symbol(
                    class.name.clone(),
                    SymbolKind::Class {
                        fields: Vec::new(), // Will be filled in second pass
                        methods: Vec::new(), // Will be filled in second pass
                        parent: None, // Will be resolved in second pass
                    },
                    self.symbol_table.current_scope_id(),
                    class.location.clone(),
                );
            }
        }
        
        if self.errors.is_empty() { Ok(()) } else { Err(()) }
    }
    
    /// Resolve all functions
    fn resolve_functions(&mut self, functions: &[HirFunction]) -> Result<Vec<ResolvedHirFunction>, ()> {
        let mut resolved_functions = Vec::new();
        
        for function in functions {
            resolved_functions.push(self.resolve_function(function.clone())?);
        }
        
        Ok(resolved_functions)
    }
    
    /// Resolve a single function
    fn resolve_function(&mut self, function: HirFunction) -> Result<ResolvedHirFunction, ()> {
        eprintln!("DEBUG RESOLVER: Resolving function '{}'", function.name);
        // Find function symbol
        let function_symbol_id = self.symbol_table.lookup_symbol(&function.name)
            .ok_or_else(|| {
                self.error(&format!("Function '{}' not found in symbol table", function.name), function.location.clone());
            })?;
        
        // Create function scope
        let function_scope = self.symbol_table.create_scope(
            None, 
            ScopeType::Function { function_id: function_symbol_id }
        );
        self.symbol_table.enter_scope(function_scope);
        self.current_function = Some(function_symbol_id);
        
        // Resolve parameters
        let mut resolved_parameters = Vec::new();
        for param in &function.parameters {
            let param_symbol_id = self.symbol_table.create_symbol(
                param.name.clone(),
                SymbolKind::Parameter { param_type: param.param_type.clone() },
                function_scope,
                param.location.clone(),
            );
            
            resolved_parameters.push(ResolvedHirParameter {
                name: param.name.clone(),
                symbol_id: param_symbol_id,
                param_type: param.param_type.clone(),
                default_value: None, // TODO: Handle default values
                is_variadic: false,  // TODO: Handle variadic parameters
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
            is_async: false, // TODO: Handle async functions
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
        let class_symbol_id = self.symbol_table.lookup_symbol(&class.name)
            .ok_or_else(|| {
                self.error(&format!("Class '{}' not found in symbol table", class.name), class.location.clone());
            })?;
        
        // Resolve parent class if exists
        eprintln!("DEBUG RESOLVER: Class '{}' parent in HIR: {:?}", class.name, class.parent);
        let parent_symbol_id = if let Some(parent_name) = &class.parent {
            eprintln!("DEBUG RESOLVER: Looking up parent class '{}'", parent_name);
            let parent_id = self.symbol_table.lookup_symbol(parent_name)
                .ok_or_else(|| {
                    self.error(&format!("Parent class '{}' not found", parent_name), class.location.clone());
                })?;
            eprintln!("DEBUG RESOLVER: Found parent class '{}' with ID {:?}", parent_name, parent_id);
            Some(parent_id)
        } else {
            eprintln!("DEBUG RESOLVER: Class '{}' has no parent", class.name);
            None
        };
        
        // Create class scope
        let class_scope = self.symbol_table.create_scope(
            None,
            ScopeType::Class { class_id: class_symbol_id }
        );
        self.symbol_table.enter_scope(class_scope);
        self.current_class = Some(class_symbol_id);
        
        // Resolve fields
        let mut resolved_fields = Vec::new();
        let mut field_symbol_ids = Vec::new();

        eprintln!("DEBUG RESOLVER: Resolving {} fields for class '{}'", class.fields.len(), class.name);

        for field in &class.fields {
            eprintln!("DEBUG RESOLVER: Creating field symbol for '{}' in class '{}'", field.name, class.name);
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
            eprintln!("DEBUG RESOLVER: Added field '{}' with symbol ID {:?}", field.name, field_symbol_id);

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
        eprintln!("DEBUG RESOLVER: Updating class '{}' symbol with {} fields and parent {:?} (immediate update)", class.name, field_symbol_ids.len(), parent_symbol_id);
        if let Some(class_symbol) = self.symbol_table.get_symbol_mut(class_symbol_id) {
            if let SymbolKind::Class { fields, parent, .. } = &mut class_symbol.kind {
                eprintln!("DEBUG RESOLVER: Before immediate update - fields: {:?}, parent: {:?}", fields, parent);
                *fields = field_symbol_ids.clone();
                *parent = parent_symbol_id;
                eprintln!("DEBUG RESOLVER: After immediate update - fields: {:?}, parent: {:?}", fields, parent);
            }
        }
        
        // Resolve constructor
        let resolved_constructor = if let Some(constructor) = &class.constructor {
            Some(self.resolve_constructor(constructor, class_symbol_id)?)
        } else {
            None
        };
        
        // Resolve methods
        let mut resolved_methods = Vec::new();
        let mut method_symbol_ids = Vec::new();
        
        for method in &class.methods {
            let method_symbol_id = self.symbol_table.create_symbol(
                method.name.clone(),
                SymbolKind::Method {
                    class_id: class_symbol_id,
                    parameters: method.parameters.iter()
                        .map(|p| p.param_type.clone())
                        .collect(),
                    return_type: method.return_type.clone(),
                },
                class_scope,
                method.location.clone(),
            );
            
            method_symbol_ids.push(method_symbol_id);
            
            let resolved_method = self.resolve_method(method.clone(), class_symbol_id, method_symbol_id)?;
            resolved_methods.push(resolved_method);
        }
        
        // Update class symbol with fields and methods
        eprintln!("DEBUG RESOLVER: Updating class '{}' symbol with {} fields and {} methods", class.name, field_symbol_ids.len(), method_symbol_ids.len());
        if let Some(class_symbol) = self.symbol_table.get_symbol_mut(class_symbol_id) {
            if let SymbolKind::Class { fields, methods, parent } = &mut class_symbol.kind {
                eprintln!("DEBUG RESOLVER: Before update - fields: {:?}", fields);
                *fields = field_symbol_ids.clone();
                *methods = method_symbol_ids;
                *parent = parent_symbol_id;
                eprintln!("DEBUG RESOLVER: After update - fields: {:?}", fields);
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
    fn resolve_constructor(&mut self, constructor: &HirConstructor, class_id: SymbolId) -> Result<ResolvedHirConstructor, ()> {
        // Create constructor scope
        let constructor_scope = self.symbol_table.create_scope(
            None,
            ScopeType::Constructor { class_id }
        );
        self.symbol_table.enter_scope(constructor_scope);

        // Set current class for implicit field access
        let previous_class = self.current_class;
        self.current_class = Some(class_id);

        // Resolve parameters
        let mut resolved_parameters = Vec::new();
        for param in &constructor.parameters {
            let param_symbol_id = self.symbol_table.create_symbol(
                param.name.clone(),
                SymbolKind::Parameter { param_type: param.param_type.clone() },
                constructor_scope,
                param.location.clone(),
            );

            resolved_parameters.push(ResolvedHirParameter {
                name: param.name.clone(),
                symbol_id: param_symbol_id,
                param_type: param.param_type.clone(),
                default_value: None, // TODO: Handle default values
                is_variadic: false,  // TODO: Handle variadic parameters
                location: param.location.clone(),
            });
        }

        // Resolve body
        let resolved_body = self.resolve_block(&constructor.body)?;

        // Exit constructor scope and restore previous class context
        self.symbol_table.exit_scope();
        self.current_class = previous_class;

        Ok(ResolvedHirConstructor {
            parameters: resolved_parameters,
            body: resolved_body,
            location: constructor.location.clone(),
        })
    }
    
    /// Resolve a method
    fn resolve_method(&mut self, method: HirMethod, class_id: SymbolId, method_id: SymbolId) -> Result<ResolvedHirMethod, ()> {
        // Create method scope
        let method_scope = self.symbol_table.create_scope(
            None,
            ScopeType::Method { method_id, class_id }
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
                SymbolKind::Parameter { param_type: param.param_type.clone() },
                method_scope,
                param.location.clone(),
            );

            resolved_parameters.push(ResolvedHirParameter {
                name: param.name.clone(),
                symbol_id: param_symbol_id,
                param_type: param.param_type.clone(),
                default_value: None, // TODO: Handle default values
                is_variadic: false,  // TODO: Handle variadic parameters
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
                        SymbolKind::Function { parameters: vec![], return_type: None },
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
        eprintln!("DEBUG RESOLVER: Resolving statement: {:?}", statement);
        match statement {
            HirStatement::VariableDeclaration { name, var_type, initializer, location } => {
                let initializer_resolved = if let Some(init) = initializer {
                    Some(self.resolve_expression(init)?)
                } else {
                    None
                };
                
                let symbol_id = self.symbol_table.create_symbol(
                    name.clone(),
                    SymbolKind::Variable { var_type: var_type.clone() },
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
            
            HirStatement::Assignment { target, value, location } => {
                let resolved_target = self.resolve_lvalue(target)?;
                let resolved_value = self.resolve_expression(value)?;
                
                Ok(ResolvedHirStatement::Assignment {
                    target: resolved_target,
                    value: resolved_value,
                    location: location.clone(),
                })
            }
            
            HirStatement::Expression { expression, location } => {
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
            
            HirStatement::If { condition, then_branch, else_branch, location } => {
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
            
            HirStatement::While { condition, body, location } => {
                let resolved_condition = self.resolve_expression(condition)?;
                let resolved_body = self.resolve_block(body)?;
                
                Ok(ResolvedHirStatement::While {
                    condition: resolved_condition,
                    body: resolved_body,
                    location: location.clone(),
                })
            }
            
            HirStatement::For { variable, iterable, body, location } => {
                let resolved_iterable = self.resolve_expression(iterable)?;
                
                // Create new scope for loop variable
                let loop_scope = self.symbol_table.create_scope(None, ScopeType::Block);
                self.symbol_table.enter_scope(loop_scope);
                
                let var_symbol_id = self.symbol_table.create_symbol(
                    variable.clone(),
                    SymbolKind::Variable { var_type: HirType::Inferred { id: 0, location: location.clone() } },
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
            
            HirStatement::Print { expression, newline, location } => {
                let resolved_expression = self.resolve_expression(expression)?;
                
                Ok(ResolvedHirStatement::Print {
                    expression: resolved_expression,
                    newline: *newline,
                    location: location.clone(),
                })
            }
        }
    }
    
    /// Resolve an expression
    fn resolve_expression(&mut self, expression: &HirExpression) -> Result<ResolvedHirExpression, ()> {
        // Check recursion depth to prevent stack overflow
        const MAX_EXPRESSION_RECURSION: usize = 50; // Increased from 5 to allow more complex expressions
        if self.expression_recursion_depth >= MAX_EXPRESSION_RECURSION {
            self.error(&format!("Maximum expression recursion depth exceeded ({})", MAX_EXPRESSION_RECURSION), 
                      expression.location().clone());
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
    fn resolve_expression_internal(&mut self, expression: &HirExpression) -> Result<ResolvedHirExpression, ()> {
        match expression {
            HirExpression::Literal { value, location } => {
                Ok(ResolvedHirExpression::Literal {
                    value: value.clone(),
                    location: location.clone(),
                })
            }
            
            HirExpression::Variable { name, location } => {
                eprintln!("DEBUG RESOLVER: Looking up variable '{}', current_class: {:?}", name, self.current_class);

                // If we're in a class method, check for class fields first (implicit field access)
                if let Some(current_class_id) = self.current_class {
                    if let Some(class_symbol) = self.symbol_table.get_symbol(current_class_id) {
                        if let SymbolKind::Class { fields, parent, .. } = &class_symbol.kind {
                            eprintln!("DEBUG RESOLVER: Checking fields in class '{}', fields: {:?}", class_symbol.name, fields);

                            // Check current class fields
                            for &field_id in fields {
                                if let Some(field_symbol) = self.symbol_table.get_symbol(field_id) {
                                    eprintln!("DEBUG RESOLVER: Checking field '{}' against '{}'", field_symbol.name, name);
                                    if field_symbol.name == *name {
                                        eprintln!("DEBUG RESOLVER: Found field '{}' - converting to field access", name);
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
                            eprintln!("DEBUG RESOLVER: Parent class for '{}': {:?}", class_symbol.name, parent);
                            if let Some(parent_class_id) = parent {
                                eprintln!("DEBUG RESOLVER: Checking parent class fields for '{}'", name);
                                if let Some(parent_symbol) = self.symbol_table.get_symbol(*parent_class_id) {
                                    if let SymbolKind::Class { fields: parent_fields, .. } = &parent_symbol.kind {
                                        eprintln!("DEBUG RESOLVER: Parent class '{}' has fields: {:?}", parent_symbol.name, parent_fields);
                                        for &parent_field_id in parent_fields {
                                            if let Some(parent_field_symbol) = self.symbol_table.get_symbol(parent_field_id) {
                                                eprintln!("DEBUG RESOLVER: Checking parent field '{}' against '{}'", parent_field_symbol.name, name);
                                                if parent_field_symbol.name == *name {
                                                    eprintln!("DEBUG RESOLVER: Found inherited field '{}' - converting to field access", name);
                                                    // Convert variable access to inherited field access
                                                    return Ok(ResolvedHirExpression::FieldAccess {
                                                        object: Box::new(ResolvedHirExpression::This {
                                                            class_symbol_id: current_class_id,
                                                            location: location.clone(),
                                                        }),
                                                        field: name.clone(),
                                                        field_symbol_id: parent_field_id,
                                                        location: location.clone(),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    eprintln!("DEBUG RESOLVER: Parent symbol not found for parent_class_id: {:?}", parent_class_id);
                                }
                            } else {
                                eprintln!("DEBUG RESOLVER: No parent class for '{}'", class_symbol.name);
                            }
                        }
                    }
                }

                eprintln!("DEBUG RESOLVER: Not found as field, trying normal symbol lookup for '{}'", name);

                // If not a field, try to find the variable in normal scope
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
            
            HirExpression::BinaryOp { left, op, right, location } => {
                let resolved_left = self.resolve_expression(left)?;
                let resolved_right = self.resolve_expression(right)?;
                
                Ok(ResolvedHirExpression::BinaryOp {
                    left: Box::new(resolved_left),
                    op: op.clone(),
                    right: Box::new(resolved_right),
                    location: location.clone(),
                })
            }
            
            HirExpression::UnaryOp { op, operand, location } => {
                let resolved_operand = self.resolve_expression(operand)?;
                
                Ok(ResolvedHirExpression::UnaryOp {
                    op: op.clone(),
                    operand: Box::new(resolved_operand),
                    location: location.clone(),
                })
            }
            
            HirExpression::Call { function, arguments, location } => {
                let function_symbol_id = self.symbol_table.lookup_symbol(function)
                    .ok_or_else(|| {
                        self.error(&format!("Function '{}' not found", function), location.clone());
                    })?;
                
                let mut resolved_arguments = Vec::new();
                for arg in arguments {
                    resolved_arguments.push(self.resolve_expression(arg)?);
                }
                
                Ok(ResolvedHirExpression::Call {
                    function: function.clone(),
                    function_symbol_id,
                    arguments: resolved_arguments,
                    location: location.clone(),
                })
            }
            
            HirExpression::MethodCall { receiver, method, arguments, location } => {
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
            
            HirExpression::FieldAccess { object, field, location } => {
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
            
            HirExpression::Index { array, index, location } => {
                let resolved_array = self.resolve_expression(array)?;
                let resolved_index = self.resolve_expression(index)?;
                
                Ok(ResolvedHirExpression::Index {
                    array: Box::new(resolved_array),
                    index: Box::new(resolved_index),
                    location: location.clone(),
                })
            }
            
            HirExpression::Array { elements, element_type, location } => {
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
            
            HirExpression::Constructor { class_name, arguments, location } => {
                let class_symbol_id = self.symbol_table.lookup_symbol(class_name)
                    .ok_or_else(|| {
                        self.error(&format!("Class '{}' not found", class_name), location.clone());
                    })?;
                
                let mut resolved_arguments = Vec::new();
                for arg in arguments {
                    resolved_arguments.push(self.resolve_expression(arg)?);
                }
                
                Ok(ResolvedHirExpression::Constructor {
                    class_name: class_name.clone(),
                    class_symbol_id,
                    arguments: resolved_arguments,
                    location: location.clone(),
                })
            }
            
            HirExpression::This { location } => {
                let class_symbol_id = self.current_class
                    .ok_or_else(|| {
                        self.error("'this' can only be used inside a class", location.clone());
                    })?;
                
                Ok(ResolvedHirExpression::This {
                    class_symbol_id,
                    location: location.clone(),
                })
            }
            
            HirExpression::Cast { expression, target_type, location } => {
                let resolved_expression = self.resolve_expression(expression)?;
                
                Ok(ResolvedHirExpression::Cast {
                    expression: Box::new(resolved_expression),
                    target_type: target_type.clone(),
                    location: location.clone(),
                })
            }
            
            HirExpression::Assignment { target, value, location } => {
                let resolved_target = self.resolve_lvalue(target)?;
                let resolved_value = self.resolve_expression(value)?;
                
                Ok(ResolvedHirExpression::Assignment {
                    target: resolved_target,
                    value: Box::new(resolved_value),
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
                            // Check current class fields
                            for &field_id in fields {
                                if let Some(field_symbol) = self.symbol_table.get_symbol(field_id) {
                                    if field_symbol.name == *name {
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
                                if let Some(parent_symbol) = self.symbol_table.get_symbol(*parent_class_id) {
                                    if let SymbolKind::Class { fields: parent_fields, .. } = &parent_symbol.kind {
                                        for &parent_field_id in parent_fields {
                                            if let Some(parent_field_symbol) = self.symbol_table.get_symbol(parent_field_id) {
                                                if parent_field_symbol.name == *name {
                                                    // Convert variable assignment to inherited field assignment
                                                    return Ok(ResolvedHirLValue::FieldAccess {
                                                        object: Box::new(ResolvedHirExpression::This {
                                                            class_symbol_id: current_class_id,
                                                            location: location.clone(),
                                                        }),
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
            
            HirLValue::FieldAccess { object, field, location } => {
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
            
            HirLValue::Index { array, index, location } => {
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
        self.errors.push(CompilerError::validation_error(message, location));
    }
    
    /// Report a warning
    fn warning(&mut self, message: &str, location: SourceLocation) {
        self.warnings.push(CompilerError::validation_warning(message, location));
    }
}

impl Default for NameResolver {
    fn default() -> Self {
        Self::new()
    }
}