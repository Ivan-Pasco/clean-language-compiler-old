/*!
 * Plugin Expander - AST transformation pass for framework block expansion
 *
 * This module provides the expansion pass that runs between parsing and HIR
 * transformation. It walks the AST, finds FrameworkBlock nodes, and replaces
 * them with expanded Clean Language statements using registered plugins.
 */

use super::{FrameworkBlock, PluginError, PluginRegistry};
use crate::ast::{Class, Function, Program, Statement};

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
}

impl<'a> PluginExpander<'a> {
    /// Create a new expander with the given plugin registry
    pub fn new(registry: &'a PluginRegistry) -> Self {
        Self {
            registry,
            blocks_expanded: 0,
            statements_generated: 0,
            pending_start: None,
            pending_functions: Vec::new(),
        }
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
        program.functions.extend(self.pending_functions.drain(..));

        tracing::debug!(
            blocks_expanded = self.blocks_expanded,
            statements_generated = self.statements_generated,
            "Plugin expansion complete"
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

                        // Add expanded statements
                        let expanded = self.expand_statements_full(expansion.statements)?;
                        result.extend(expanded);
                    } else {
                        result.push(Statement::FrameworkBlock {
                            name,
                            content: block.content,
                            attributes: block.attributes,
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
                        // No handler - keep the block for later error reporting
                        // (will fail in semantic analysis with helpful error)
                        result.push(Statement::FrameworkBlock {
                            name,
                            content: block.content,
                            attributes: block.attributes,
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
                    body,
                    location,
                } => {
                    let expanded_body = self.expand_statements(body)?;
                    result.push(Statement::RangeIterate {
                        iterator,
                        start,
                        end,
                        step,
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

                Statement::PrivateBlock { items, location } => {
                    let expanded_items = self.expand_statements(items)?;
                    result.push(Statement::PrivateBlock {
                        items: expanded_items,
                        location,
                    });
                }

                Statement::Match {
                    value,
                    cases,
                    location,
                } => {
                    // Match cases may contain statements in future extensions
                    result.push(Statement::Match {
                        value,
                        cases,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expression, Value};
    use crate::plugins::{FrameworkPlugin, PluginResult};
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
            start_function: None,
            tests: vec![],
            screens: vec![],
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
    fn test_unknown_block_passes_through() {
        let registry = PluginRegistry::new();

        let program = make_test_program(vec![Statement::FrameworkBlock {
            name: "unknown".to_string(),
            content: "content".to_string(),
            attributes: vec![],
            location: None,
        }]);

        // Unknown blocks should pass through (will fail later in semantic analysis)
        let mut expander = PluginExpander::new(&registry);
        let result = expander.expand_program(program).unwrap();
        assert_eq!(result.statements.len(), 1);
        assert!(matches!(
            result.statements[0],
            Statement::FrameworkBlock { .. }
        ));
    }
}
