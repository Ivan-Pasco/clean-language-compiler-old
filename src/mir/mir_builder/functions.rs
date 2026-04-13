//! Function building — lowers TAST functions and classes to MIR functions.

use super::*;

impl MirBuilder {
    /// Build MIR function from TAST function
    pub(super) fn build_function(
        &mut self,
        tast_function: TastFunction,
    ) -> Result<MirFunction, Vec<CompilerError>> {
        self.build_function_with_class_context(tast_function, None)
    }

    /// Build MIR function from TAST function with optional class context for field access
    pub(super) fn build_function_with_class_context(
        &mut self,
        tast_function: TastFunction,
        class_context: Option<&TastClass>,
    ) -> Result<MirFunction, Vec<CompilerError>> {
        debug!(
            function = %tast_function.name,
            param_count = tast_function.parameters.len(),
            "Building MIR for function"
        );
        for (i, param) in tast_function.parameters.iter().enumerate() {
            trace!(
                index = i,
                name = %param.name,
                param_type = ?param.param_type,
                has_default = param.default_value.is_some(),
                "TAST parameter"
            );
        }

        // Create function structure
        let mir_function = MirFunction {
            symbol_id: tast_function.symbol_id,
            name: tast_function.name.clone(),
            parameters: Vec::new(),
            return_type: self.convert_concrete_type(&tast_function.return_type),
            blocks: HashMap::new(),
            entry_block: BasicBlockId(0),
            locals: HashMap::new(),
            next_value_id: 0,
            next_block_id: 1, // Block 0 is entry block
            attributes: MirFunctionAttributes {
                inline: false,
                pure: false,
                entry_point: tast_function.name == "start",
                exported: tast_function.name == "start",
            },
            location: tast_function.location.clone(),
        };

        // Create function build context
        let mut context = FunctionBuildContext {
            function: mir_function,
            scope_stack: vec![HashMap::new()],
            pending_phis: Vec::new(),
            loop_stack: Vec::new(),
            class_context: class_context.cloned(),
            all_classes: self.all_classes.clone(),
            all_functions: self.all_functions.clone(),
        };

        // For class methods and constructors, add implicit 'this' parameter as the first parameter
        // EXCEPT for static methods which don't need 'this'
        trace!(
            function = %tast_function.name,
            has_class_context = class_context.is_some(),
            is_static = tast_function.is_static,
            "Checking this parameter requirement"
        );
        if let Some(_class_ctx) = class_context {
            if !tast_function.is_static {
                let this_value_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;

                // Create 'this' parameter with class type
                let this_param = MirParameter {
                    value_id: this_value_id,
                    name: "this".to_string(),
                    param_type: MirType::I32, // Instance pointer is i32 in WASM
                    location: tast_function.location.clone(),
                };

                context.function.parameters.push(this_param);

                // Add 'this' to scope
                if let Some(current_scope) = context.scope_stack.last_mut() {
                    current_scope.insert("this".to_string(), this_value_id);
                }
            }
        }

        // Process parameters
        for (_i, param) in tast_function.parameters.iter().enumerate() {
            let value_id = ValueId(context.function.next_value_id);
            context.function.next_value_id += 1;

            let mir_param = MirParameter {
                value_id,
                name: param.name.clone(),
                param_type: self.convert_concrete_type(&param.param_type),
                location: param.location.clone(),
            };

            context.function.parameters.push(mir_param);

            // Add parameter to current scope
            if let Some(current_scope) = context.scope_stack.last_mut() {
                current_scope.insert(param.name.clone(), value_id);
            }
        }

        trace!(
            function_name = %context.function.name,
            param_count = context.function.parameters.len(),
            "MIR parameters created for function"
        );
        for (i, mir_param) in context.function.parameters.iter().enumerate() {
            trace!(
                index = i,
                param_name = %mir_param.name,
                param_type = ?mir_param.param_type,
                "MIR parameter"
            );
        }

        // Create entry basic block
        let entry_block = MirBasicBlock {
            id: BasicBlockId(0),
            label: Some("entry".to_string()),
            instructions: Vec::new(),
            terminator: MirTerminator::Unreachable, // Will be replaced
            predecessors: HashSet::new(),
            successors: HashSet::new(),
            location: tast_function.location.clone(),
        };

        context.function.blocks.insert(BasicBlockId(0), entry_block);
        self.current_block = Some(BasicBlockId(0));

        // Lower function body
        debug!(
            function_name = %tast_function.name,
            statement_count = tast_function.body.statements.len(),
            has_class_context = context.class_context.is_some(),
            "Building function body"
        );
        if let Some(ref class) = context.class_context {
            trace!(field_count = class.fields.len(), "Class context fields");
        }

        // Handle automatic return for last expression in non-void functions
        let has_non_void_return = !matches!(tast_function.return_type, ConcreteType::Undefined);
        self.build_function_body(&mut context, &tast_function.body, has_non_void_return)?;

        // Inject state rules checking at operation boundaries (start:, frame:)
        if (tast_function.name == "start" || tast_function.name == "frame")
            && !self.state_rules.is_empty()
        {
            debug!(
                function_name = %tast_function.name,
                rule_count = self.state_rules.len(),
                "Injecting state rules checking at operation boundary"
            );
            self.inject_rules_checking(&mut context)?;
        }

        // Ensure function has proper termination
        self.ensure_function_termination(&mut context, &tast_function.return_type)?;

        // Resolve pending phi nodes
        self.resolve_phi_nodes(&mut context)?;

        self.stats.basic_blocks_created += context.function.blocks.len();
        self.stats.ssa_values_created += context.function.next_value_id;
        self.stats.phi_nodes_inserted += context.pending_phis.len();

        if context.function.name == "test" {
            trace!(
                function_name = %context.function.name,
                block_count = context.function.blocks.len(),
                "Final MIR function blocks"
            );
            for (block_id, block) in &context.function.blocks {
                trace!(block_id = ?block_id, terminator = ?block.terminator, "Block terminator");
            }
        }

        Ok(context.function)
    }

    /// Build MIR class functions (constructor and methods)
    pub(super) fn build_class(
        &mut self,
        tast_class: TastClass,
    ) -> Result<Vec<MirFunction>, Vec<CompilerError>> {
        let mut functions = Vec::new();
        let mut errors = Vec::new();

        // Clone class for passing to methods (needed to avoid borrow checker issues)
        let class_for_methods = tast_class.clone();

        // Build all constructors with class context (constructors can use 'this')
        for constructor in tast_class.constructors {
            match self.build_function_with_class_context(constructor, Some(&class_for_methods)) {
                Ok(ctor_function) => functions.push(ctor_function),
                Err(ctor_errors) => errors.extend(ctor_errors),
            }
        }

        // Build all methods with class context
        for method in tast_class.methods {
            match self.build_function_with_class_context(method, Some(&class_for_methods)) {
                Ok(method_function) => functions.push(method_function),
                Err(method_errors) => errors.extend(method_errors),
            }
        }

        if errors.is_empty() {
            Ok(functions)
        } else {
            Err(errors)
        }
    }

    /// Build function body with automatic return handling
    pub(super) fn build_function_body(
        &mut self,
        context: &mut FunctionBuildContext,
        block: &TastBlock,
        has_non_void_return: bool,
    ) -> Result<(), Vec<CompilerError>> {
        // Enter new scope
        context.scope_stack.push(HashMap::new());

        let num_statements = block.statements.len();

        // Check if the last statement is an expression that should be auto-returned
        let last_is_auto_return = if has_non_void_return && num_statements > 0 {
            matches!(
                block.statements.last(),
                Some(TastStatement::Expression { .. })
            )
        } else {
            false
        };

        // Process all statements except possibly the last
        let statements_to_process = if last_is_auto_return {
            num_statements - 1
        } else {
            num_statements
        };

        for (i, statement) in block
            .statements
            .iter()
            .take(statements_to_process)
            .enumerate()
        {
            trace!(
                statement_index = i,
                statement_type = ?std::mem::discriminant(statement),
                current_block = ?self.current_block,
                "Processing statement"
            );
            self.build_statement(context, statement)?;
            trace!(
                statement_index = i,
                current_block = ?self.current_block,
                "After statement"
            );
        }

        // Handle last statement if it should be auto-returned
        if last_is_auto_return {
            if let Some(TastStatement::Expression {
                expression,
                location: _,
            }) = block.statements.last()
            {
                trace!("Auto-returning last expression");
                // Build expression and convert to return
                let value_id = self.build_expression(context, expression)?;

                // Add return instruction
                if let Some(block_id) = self.current_block {
                    if let Some(current_block) = context.function.blocks.get_mut(&block_id) {
                        current_block.terminator = MirTerminator::Return {
                            value: Some(MirOperand::Value(value_id)),
                        };
                    }
                }
            }
        }

        // Exit scope
        context.scope_stack.pop();

        Ok(())
    }

    /// Build basic block from TAST block
    pub(super) fn build_block(
        &mut self,
        context: &mut FunctionBuildContext,
        block: &TastBlock,
    ) -> Result<(), Vec<CompilerError>> {
        // Enter new scope
        context.scope_stack.push(HashMap::new());

        trace!(statement_count = block.statements.len(), "Processing block");

        // Lower all statements
        for (i, statement) in block.statements.iter().enumerate() {
            trace!(
                statement_index = i,
                statement_type = ?std::mem::discriminant(statement),
                current_block = ?self.current_block,
                "Processing block statement"
            );
            self.build_statement(context, statement)?;
            trace!(
                statement_index = i,
                current_block = ?self.current_block,
                "After block statement"
            );
        }

        // Exit scope
        context.scope_stack.pop();

        Ok(())
    }

    /// Helper function to check if a block effectively returns (has Return OR Branch where both branches return)
    pub(super) fn block_effectively_returns(
        &self,
        context: &FunctionBuildContext,
        block_id: BasicBlockId,
    ) -> bool {
        let mut visited = std::collections::HashSet::new();
        self.block_effectively_returns_recursive(context, block_id, &mut visited)
    }

    pub(super) fn block_effectively_returns_recursive(
        &self,
        context: &FunctionBuildContext,
        block_id: BasicBlockId,
        visited: &mut std::collections::HashSet<BasicBlockId>,
    ) -> bool {
        // Prevent infinite loops
        if visited.contains(&block_id) {
            return false;
        }
        visited.insert(block_id);

        let block = match context.function.blocks.get(&block_id) {
            Some(b) => b,
            None => return false,
        };

        match &block.terminator {
            MirTerminator::Return { .. } => true,
            MirTerminator::Branch {
                true_block,
                false_block,
                ..
            } => {
                // Both branches must effectively return
                self.block_effectively_returns_recursive(context, *true_block, visited)
                    && self.block_effectively_returns_recursive(context, *false_block, visited)
            }
            MirTerminator::Unreachable | MirTerminator::Jump { .. } => false,
            MirTerminator::Trap => true, // Trap terminates execution
        }
    }
}
