//! Code generation for Clean Language functions, statements, and expressions.
//! This module contains the core code generation logic for the `CodeGenerator`.

use super::instruction_generator::LocalVarInfo;
use crate::ast::{
    self, AssignmentTarget, BinaryOperator, Expression, Function as AstFunction, PostfixOperator,
    SourceLocation, Statement, Type, UnaryOperator, Value,
};
use crate::error::CompilerError;
use crate::types::WasmType;
use tracing::{debug, trace};
use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

impl super::CodeGenerator {
    pub fn generate_function(&mut self, function: &AstFunction) -> Result<(), CompilerError> {
        // NOTE: Legacy workaround — infer class context for functions that should be class methods
        // This handles cases where the parser incorrectly reconstructs class methods as standalone functions
        let inferred_class = self.infer_class_context_for_function(&function.name);
        if let Some(class_name) = inferred_class {
            self.current_class_context = Some(class_name);
        }

        // Reset function state
        self.current_function_params.clear();
        self.current_function_locals.clear();
        self.variable_map.clear();
        self.variable_types.clear();
        self.current_function_param_count = function.parameters.len() as u32;

        // Add parameters with WebAssembly-compliant indexing (0, 1, 2...)
        for (param_index, param) in function.parameters.iter().enumerate() {
            let param_info = LocalVarInfo {
                index: param_index as u32, // Parameters use indices 0, 1, 2...
                type_: WasmType::from(&param.type_).into(),
            };
            self.current_function_params.push(param_info.clone());
            self.variable_map.insert(param.name.clone(), param_info);

            // Track parameter types for automatic toString() conversion
            self.variable_types
                .insert(param.name.clone(), param.type_.clone());
        }

        // If we're in a class context, add class fields as locals (indices param_count+N)
        // Include fields from the entire inheritance hierarchy
        if let Some(class_name) = &self.current_class_context {
            if let Some(_class) = self.class_table.get(class_name).cloned() {
                // Build the inheritance hierarchy (current class + all parents)
                let mut hierarchy = Vec::new();
                let mut current_class_name = class_name.clone();

                while let Some(class_def) = self.class_table.get(&current_class_name) {
                    hierarchy.push(current_class_name.clone());
                    if let Some(ref base_class) = class_def.base_class {
                        current_class_name = base_class.clone();
                    } else {
                        break;
                    }
                }

                // Add fields from all classes in the hierarchy (parents first)
                for class_name_in_hierarchy in hierarchy.iter().rev() {
                    if let Some(class_def) = self.class_table.get(class_name_in_hierarchy) {
                        for field in &class_def.fields {
                            // Only add if not already defined (avoid duplicates)
                            if !self.variable_map.contains_key(&field.name) {
                                let local_info = LocalVarInfo {
                                    index: self.current_function_param_count
                                        + self.current_function_locals.len() as u32,
                                    type_: WasmType::from(&field.type_).into(),
                                };
                                self.current_function_locals.push(local_info.clone());
                                self.variable_map.insert(field.name.clone(), local_info);

                                // Track field types
                                self.variable_types
                                    .insert(field.name.clone(), field.type_.clone());
                            }
                        }
                    }
                }
            }
        }

        // Generate function body
        let mut instructions = Vec::new();

        // Check if the function has a non-void return type
        let needs_return_value = function.return_type != Type::Void;

        // Handle function body with implicit return logic
        if !function.body.is_empty() {
            // Generate all statements except the last one normally
            for stmt in &function.body[..function.body.len().saturating_sub(1)] {
                self.generate_statement(stmt, &mut instructions)?;
            }

            // Handle the last statement specially for implicit returns
            if let Some(last_stmt) = function.body.last() {
                match last_stmt {
                    Statement::Expression { expr, .. } => {
                        // For expression statements as the last statement, treat as implicit return
                        // unless the function return type is Void
                        if function.return_type == Type::Void {
                            // If function returns void, generate the expression but drop the value
                            // EXCEPT for expressions that don't leave values on the stack
                            let result_type = self.generate_expression(expr, &mut instructions)?;

                            // Only drop if the expression actually produced a value
                            // Void function calls return WasmType::Unit and don't leave values on the stack
                            if result_type != WasmType::Unit {
                                instructions.push(Instruction::Drop);
                            }
                        } else {
                            // If function has a return type, use the expression as return value
                            self.generate_expression(expr, &mut instructions)?;
                            // Don't add explicit return instruction - WASM functions implicitly return the top stack value
                        }
                    }
                    Statement::Print { .. } => {
                        // Print statements are void and don't leave values on the stack
                        self.generate_statement(last_stmt, &mut instructions)?;
                        // No need to drop anything since print statements return void
                    }
                    Statement::Return { .. } => {
                        // For explicit return statements, generate normally
                        self.generate_statement(last_stmt, &mut instructions)?;
                    }
                    Statement::VariableDecl { .. } => {
                        // Variable declarations don't leave values on the stack
                        self.generate_statement(last_stmt, &mut instructions)?;

                        // If the function has a non-void return type and this is the last statement,
                        // we need to add a default return value since variable declarations don't produce one
                        if needs_return_value {
                            match function.return_type {
                                Type::Integer => instructions.push(Instruction::I32Const(0)),
                                Type::Number => instructions.push(Instruction::F64Const(0.0)),
                                Type::Boolean => instructions.push(Instruction::I32Const(0)),
                                _ => instructions.push(Instruction::I32Const(0)), // Default for other types
                            }
                        }
                        // For void functions, nothing more needs to be done since variable declarations
                        // don't leave values on the stack that need to be dropped
                    }
                    _ => {
                        // For non-expression, non-return statements, generate normally
                        self.generate_statement(last_stmt, &mut instructions)?;

                        // If the function has a non-void return type and the last statement isn't a return,
                        // we need to add a default return value
                        if needs_return_value {
                            match function.return_type {
                                Type::Integer => instructions.push(Instruction::I32Const(0)),
                                Type::Number => instructions.push(Instruction::F64Const(0.0)),
                                Type::Boolean => instructions.push(Instruction::I32Const(0)),
                                _ => instructions.push(Instruction::I32Const(0)), // Default for other types
                            }
                        }
                    }
                }
            }
        } else {
            // Empty function body - add default return if needed
            if needs_return_value {
                match function.return_type {
                    Type::Integer => instructions.push(Instruction::I32Const(0)),
                    Type::Number => instructions.push(Instruction::F64Const(0.0)),
                    Type::Boolean => instructions.push(Instruction::I32Const(0)),
                    Type::IntegerSized { bits: 8..=32, .. } => {
                        instructions.push(Instruction::I32Const(0))
                    }
                    Type::IntegerSized { bits: 64, .. } => {
                        instructions.push(Instruction::I64Const(0))
                    }
                    Type::NumberSized { bits: 32 } => instructions.push(Instruction::F32Const(0.0)),
                    Type::NumberSized { bits: 64 } => instructions.push(Instruction::F64Const(0.0)),
                    Type::Object(_) => instructions.push(Instruction::I32Const(0)), // Object as pointer (0 = null for now)
                    Type::String => instructions.push(Instruction::I32Const(0)), // String as pointer
                    Type::List(_) => instructions.push(Instruction::I32Const(0)), // List as pointer
                    Type::Pairs(_, _) => instructions.push(Instruction::I32Const(0)), // Pairs as pointer
                    Type::Matrix(_) => instructions.push(Instruction::I32Const(0)), // Matrix as pointer
                    Type::Any => instructions.push(Instruction::I32Const(0)), // Any as pointer
                    Type::Void => {} // No return value needed for void
                    _ => {
                        return Err(CompilerError::codegen_error(
                            format!(
                                "Cannot generate default return value for type {:?}",
                                function.return_type
                            ),
                            None,
                            None,
                        ));
                    }
                }
            }
        }

        // Create function with only actual local variables (WebAssembly spec compliant)
        // Note: current_function_locals contains LocalVarInfo with absolute indices,
        // but Function::new() only needs the types since WASM handles indexing automatically
        let locals = self
            .current_function_locals
            .iter()
            .map(|local| (1u32, local.type_))
            .collect::<Vec<_>>();

        let mut func = Function::new(locals);

        // Add all instructions - they should already be properly structured
        for instruction in &instructions {
            func.instruction(instruction);
        }

        // Always add END instruction to close the function body
        // Control flow structures (Block, Loop, If) have their own END instructions
        // but the function body itself always needs a final END
        func.instruction(&Instruction::End);

        // CRITICAL DEBUG: Show all instructions for the start function
        if function.name == "start" {
            // Debug instruction sequence printing disabled
        }

        // Add to code section
        self.code_section.function(&func);

        // Just store function type in instruction generator for proper return type detection
        // (The function section registration is already handled by prepare_function_type)
        let param_types: Vec<WasmType> = function
            .parameters
            .iter()
            .map(|param| WasmType::from(&param.type_))
            .collect();

        let return_type = if function.return_type == Type::Void {
            None
        } else {
            Some(WasmType::from(&function.return_type))
        };

        // Find the function index from the function map (set by prepare_function_type)
        let function_index = self.function_map.get(&function.name).ok_or_else(|| {
            CompilerError::codegen_error(
                format!(
                    "Function '{function_name}' not found in function map",
                    function_name = function.name
                ),
                None,
                None,
            )
        })?;

        // Store function type in instruction generator for proper return type detection
        self.instruction_generator.add_function_type(
            *function_index,
            param_types
                .iter()
                .map(|wasm_type| match wasm_type {
                    WasmType::I32 => ValType::I32,
                    WasmType::I64 => ValType::I64,
                    WasmType::F32 => ValType::F32,
                    WasmType::F64 => ValType::F64,
                    WasmType::V128 => ValType::V128,
                    _ => ValType::I32,
                })
                .collect(),
            if let Some(ret_type) = return_type {
                vec![match ret_type {
                    WasmType::I32 => ValType::I32,
                    WasmType::I64 => ValType::I64,
                    WasmType::F32 => ValType::F32,
                    WasmType::F64 => ValType::F64,
                    WasmType::V128 => ValType::V128,
                    _ => ValType::I32,
                }]
            } else {
                vec![]
            },
        );

        Ok(())
    }

    pub fn generate_statement(
        &mut self,
        stmt: &Statement,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        match stmt {
            Statement::VariableDecl {
                name,
                type_,
                initializer,
                location,
            } => {
                self.generate_variable_decl_statement(
                    name,
                    type_,
                    initializer,
                    location,
                    instructions,
                )?;
            }
            Statement::Assignment {
                target,
                value,
                location,
            } => {
                self.generate_assignment_target(target, value, location, instructions)?;
            }
            Statement::Print {
                expression,
                newline,
                ..
            } => {
                self.generate_print_statement(expression, *newline, instructions)?;
            }
            Statement::PrintBlock {
                expressions,
                newline,
                ..
            } => {
                for expression in expressions {
                    self.generate_print_statement(expression, *newline, instructions)?;
                }
            }
            Statement::Return { value, .. } => {
                self.generate_return_statement(value, instructions)?;
            }
            Statement::Break { .. } => {
                // Break exits the innermost loop by branching to the outer block
                if self.loop_break_labels.is_empty() {
                    return Err(CompilerError::codegen_error(
                        "break statement outside of loop",
                        Some("break statements can only be used inside loops".to_string()),
                        None,
                    ));
                }
                // Get the label depth of the loop's outer block
                let break_target_depth = *self.loop_break_labels.last().unwrap();
                // Calculate relative label from current depth
                let label = self.current_block_depth - break_target_depth;
                instructions.push(Instruction::Br(label));
            }
            Statement::Continue { .. } => {
                // Continue jumps to the loop header for next iteration
                if self.loop_continue_labels.is_empty() {
                    return Err(CompilerError::codegen_error(
                        "continue statement outside of loop",
                        Some("continue statements can only be used inside loops".to_string()),
                        None,
                    ));
                }
                // Get the label depth of the loop block
                let continue_target_depth = *self.loop_continue_labels.last().unwrap();
                // Calculate relative label from current depth
                let label = self.current_block_depth - continue_target_depth;
                instructions.push(Instruction::Br(label));
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.generate_if_statement(condition, then_branch, else_branch, instructions)?;
            }
            Statement::Iterate {
                iterator,
                collection,
                body,
                ..
            } => {
                self.generate_iterate_statement(iterator, collection, body, instructions)?;
            }
            Statement::While {
                condition, body, ..
            } => {
                self.generate_while_statement(condition, body, instructions)?;
            }
            Statement::Test { name: _, body, .. } => {
                self.generate_test_statement(body, instructions)?;
            }
            Statement::TestsBlock { tests, .. } => {
                // Generate test runner function for the test block
                self.generate_tests_block_runner(tests, instructions)?;
            }
            Statement::Expression { expr, .. } => {
                self.generate_expression_statement(expr, instructions)?;
            }
            Statement::TypeApplyBlock {
                type_, assignments, ..
            } => {
                self.generate_type_apply_block_statement(type_, assignments, instructions)?;
            }
            Statement::FunctionApplyBlock {
                function_name,
                expressions,
                ..
            } => {
                debug!("DEBUG: Statement::FunctionApplyBlock matched");
                trace!("  function_name: {}", function_name);
                trace!("  expressions: {:?}", expressions);
                self.generate_function_apply_block_statement(
                    function_name,
                    expressions,
                    instructions,
                )?;
            }
            Statement::MethodApplyBlock {
                object_name,
                method_chain,
                expressions,
                ..
            } => {
                self.generate_method_apply_block_statement(
                    object_name,
                    method_chain,
                    expressions,
                    instructions,
                )?;
            }
            Statement::ConstantApplyBlock { constants, .. } => {
                self.generate_constant_apply_block_statement(constants, instructions)?;
            }
            Statement::RangeIterate {
                iterator,
                start,
                end,
                step,
                body,
                ..
            } => {
                self.generate_range_iterate_statement(
                    iterator,
                    start,
                    end,
                    step.as_ref(),
                    body,
                    instructions,
                )?;
            }
            Statement::Error { message, .. } => {
                self.generate_error_statement(message, instructions)?;
            }
            Statement::Import { .. } => {
                // For now, imports are no-ops in code generation
            }
            Statement::LaterAssignment {
                variable,
                expression,
                ..
            } => {
                self.generate_later_assignment_statement(variable, expression, instructions)?;
            }
            Statement::Background { expression, .. } => {
                self.generate_background_statement(expression, instructions)?;
            }

            Statement::OnErrorBlock {
                expression,
                error_block,
                ..
            } => {
                // For now, generate the expression normally and ignore the error block
                // Full error handling would require try-catch WASM instructions or custom runtime
                self.generate_expression_statement(expression, instructions)?;

                // Error handling requires WASM exception handling proposal
                // Expression executes normally; error_block pending WASM support
                let _ = error_block; // Suppress unused warning
            }

            Statement::FunctionsBlock { functions, .. } => {
                // Functions block - generate code for all functions
                for function in functions {
                    self.generate_function(function)?;
                }
            }

            Statement::PrivateBlock { items, .. } => {
                // Private block - generate code for all items
                for item in items {
                    self.generate_statement(item, instructions)?;
                }
            }

            Statement::Description { .. } => {
                // Description statements are metadata only - no code generation needed
                // They are used for documentation and should be skipped during execution
            }

            Statement::StandaloneErrorHandler { body, .. } => {
                // Standalone error handler - generate error handling statements
                // WASM exception handling proposal not yet stabilized
                let mut error_instructions = Vec::new();
                for stmt in body {
                    self.generate_statement(stmt, &mut error_instructions)?;
                }
                // For now, we append the error handling instructions
                // In the future, this should be wrapped in proper exception handling
                instructions.extend(error_instructions);
            }

            Statement::ClassDefinition { class, .. } => {
                // Class definition - generate class code
                self.generate_class(class)?;
            }

            Statement::FrameworkBlock { name, location, .. } => {
                // Framework blocks should be expanded by plugins before codegen
                // If we reach here, it means the plugin expansion pass didn't run
                return Err(CompilerError::codegen_error(
                    format!(
                        "Unexpanded framework block '{}:'. Framework blocks must be expanded by plugins before code generation.",
                        name
                    ),
                    Some("Ensure framework plugins are loaded and the expansion pass runs before codegen".to_string()),
                    location.clone(),
                ));
            }

            Statement::ScreenBlock { location, .. } => {
                // Screen blocks should be handled as framework blocks by plugins
                return Err(CompilerError::codegen_error(
                    "Screen blocks are not supported in direct compilation. Use framework plugins."
                        .to_string(),
                    Some(
                        "Screen blocks should be parsed as framework blocks by the plugin system"
                            .to_string(),
                    ),
                    location.clone(),
                ));
            }

            Statement::UiBlock { location, .. } => {
                // UI blocks should be handled as framework blocks by plugins
                return Err(CompilerError::codegen_error(
                    "UI blocks are not supported in direct compilation. Use framework plugins."
                        .to_string(),
                    Some(
                        "UI blocks should be parsed as framework blocks by the plugin system"
                            .to_string(),
                    ),
                    location.clone(),
                ));
            }
            Statement::StateBlockStmt { .. } => {
                // State blocks are handled at HIR level; this is exhaustive match only
            }
            Statement::WatchBlockStmt { .. } => {
                // Watch blocks are handled at HIR level; this is exhaustive match only
            }
            Statement::ResetStmt { .. } => {
                // Reset statements are handled at HIR level; this is exhaustive match only
            }
            Statement::ScreenBlockStmt { .. } => {
                // Screen blocks are handled at HIR level; this is exhaustive match only
            }
            Statement::Require {
                condition,
                location,
            } => {
                // Generate code for require statement (precondition check)
                // Evaluate condition, trap if false
                self.generate_require_statement(condition, location, instructions)?;
            }
            // AI metadata statements — compile-time only, no code generation
            Statement::Spec { .. }
            | Statement::Intent { .. }
            | Statement::SourceBlock { .. }
            | Statement::BuildBlock { .. } => {}
        }
        Ok(())
    }

    pub(crate) fn generate_variable_decl_statement(
        &mut self,
        name: &str,
        type_: &Type,
        initializer: &Option<Expression>,
        location: &Option<SourceLocation>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        let specified_type = WasmType::from(type_);

        let (var_type, init_instructions) = if let Some(init_expr) = initializer {
            let mut init_instr = Vec::new();
            let init_type =
                self.generate_expression_with_type_hint(init_expr, Some(type_), &mut init_instr)?;

            let target_type = specified_type;

            if !self.types_compatible(&init_type, &target_type) {
                return Err(CompilerError::type_error(
                    format!("Initializer type {init_type:?} does not match specified type {target_type:?} for variable '{name}'"),
                    None, location.clone()
                ));
            }

            if init_type != target_type {
                self.generate_conversion(init_type, target_type, &mut init_instr)?;
            }

            (target_type, Some(init_instr))
        } else {
            (specified_type, None)
        };

        let local_index = self.add_local_variable(var_type);
        let local_info = LocalVarInfo {
            index: local_index,
            type_: var_type.into(),
        };
        self.variable_map
            .insert(name.to_string(), local_info.clone());

        // Track the original Clean Language type for automatic toString() conversion
        self.variable_types.insert(name.to_string(), type_.clone());

        if let Some(init_instr) = init_instructions {
            instructions.extend(init_instr);
            instructions.push(Instruction::LocalSet(local_info.index));
        } else {
            // Check if this is an object type that needs constructor call
            if let Type::Object(class_name) = type_ {
                // Call the default constructor for this class
                let constructor_name = format!("{}_constructor", class_name);
                if let Some(constructor_index) = self.function_map.get(&constructor_name) {
                    instructions.push(Instruction::Call(*constructor_index));
                    instructions.push(Instruction::LocalSet(local_info.index));
                } else {
                    return Err(CompilerError::codegen_error(
                        format!(
                            "Constructor '{}' not found for class '{}'",
                            constructor_name, class_name
                        ),
                        Some("Ensure the class has a constructor defined".to_string()),
                        location.clone(),
                    ));
                }
            } else {
                // Handle primitive types with default values
                match var_type {
                    WasmType::I32 => instructions.push(Instruction::I32Const(0)),
                    WasmType::I64 => instructions.push(Instruction::I64Const(0)),
                    WasmType::F32 => instructions.push(Instruction::F32Const(0.0)),
                    WasmType::F64 => instructions.push(Instruction::F64Const(0.0)),
                    _ => {
                        return Err(CompilerError::codegen_error(
                            format!("Cannot determine default value for type {var_type:?}"),
                            None,
                            location.clone(),
                        ))
                    }
                }
                instructions.push(Instruction::LocalSet(local_info.index));
            }
        }
        Ok(())
    }

    /// Dispatcher for `Statement::Assignment` — routes to the correct code-generation
    /// path based on the `AssignmentTarget` variant.
    pub(crate) fn generate_assignment_target(
        &mut self,
        target: &AssignmentTarget,
        value: &Expression,
        location: &Option<SourceLocation>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        match target {
            AssignmentTarget::Variable(name) => {
                self.generate_assignment_statement(name, value, location, instructions)
            }
            AssignmentTarget::Index { collection, index } => {
                // Reuse the existing ListAssignment expression codegen path:
                // generate an Expression::ListAssignment and evaluate it.
                let expr = Expression::ListAssignment {
                    list: Box::new(Expression::Variable(collection.clone())),
                    index: index.clone(),
                    value: Box::new(value.clone()),
                    location: location.clone().unwrap_or_default(),
                };
                self.generate_expression(&expr, instructions)?;
                Ok(())
            }
            AssignmentTarget::Property { object, path } => {
                // Property field assignment. We look up the object in the local
                // variable map then generate a field-write using the same logic
                // as generate_assignment_statement on the nested field name.
                // For `obj.a.b = val`, the canonical path is obj → a → b where
                // "b" is the final field and the prefix determines the object.
                // At the codegen level we treat `object.path[0..n-1]` as the
                // receiver and `path[n-1]` as the field.  For single-element
                // paths (`obj.field`) this becomes `object` + `field`.
                let (field, prefix) = path
                    .split_last()
                    .expect("AssignmentTarget::Property requires at least one path element");
                // Build the variable name used to look up the receiver.
                // For simple `obj.field`, just look up `object`.
                let receiver_name = if prefix.is_empty() {
                    object.clone()
                } else {
                    // For deeper chains we join with '.' — the variable_map
                    // typically only contains the root object, so we use the
                    // root object name as the receiver and the entire path chain
                    // as field access.  Nested field writes beyond two levels
                    // require runtime support; for now we report a codegen error.
                    return Err(CompilerError::codegen_error(
                        format!(
                            "Nested property assignment '{}.{}.{field}' is not yet supported in codegen",
                            object,
                            prefix.join(".")
                        ),
                        Some("Use a temporary variable for deeply nested field assignments".to_string()),
                        location.clone(),
                    ));
                };
                self.generate_assignment_statement(
                    &format!("{receiver_name}.{field}"),
                    value,
                    location,
                    instructions,
                )
            }
        }
    }

    pub(crate) fn generate_assignment_statement(
        &mut self,
        target: &str,
        value: &Expression,
        location: &Option<SourceLocation>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        if let Some(local_info) = self.find_local(target) {
            let value_type = self.generate_expression(value, instructions)?;
            let target_type = local_info.type_.into();

            // Add type conversion if needed
            if value_type != target_type {
                self.generate_conversion(value_type, target_type, instructions)?;
            }

            instructions.push(Instruction::LocalSet(local_info.index));
        } else if let Some(class_context) = &self.current_class_context {
            let field_info = self
                .class_field_map
                .get(class_context)
                .and_then(|field_map| field_map.get(target).cloned());

            if let Some((field_type, _field_offset)) = field_info {
                let value_type = self.generate_expression(value, instructions)?;

                let wasm_type = self.ast_type_to_wasm_type(&field_type)?;
                let local_index = self.add_local_variable(wasm_type);

                // Add type conversion if needed
                if value_type != wasm_type {
                    self.generate_conversion(value_type, wasm_type, instructions)?;
                }

                self.variable_map.insert(
                    target.to_string(),
                    LocalVarInfo {
                        index: local_index,
                        type_: wasm_type.into(),
                    },
                );

                instructions.push(Instruction::LocalSet(local_index));
            } else if self.class_field_map.contains_key(class_context) {
                return Err(CompilerError::codegen_error(
                    format!("Field '{target}' not found in class '{class_context}'"),
                    None,
                    location.clone(),
                ));
            } else {
                return Err(CompilerError::codegen_error(
                    format!("Class '{class_context}' not found"),
                    None,
                    location.clone(),
                ));
            }
        } else {
            return Err(CompilerError::codegen_error(
                format!("Undefined variable: {target}"),
                None,
                location.clone(),
            ));
        }
        Ok(())
    }

    pub(crate) fn generate_print_statement(
        &mut self,
        expression: &Expression,
        newline: bool,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        let func_name = if newline { "printl" } else { "print" };
        self.generate_print_call(func_name, expression, instructions)
    }

    pub(crate) fn generate_expression(
        &mut self,
        expr: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Extract location if available, or use None
        let loc = match expr {
            Expression::Binary(_, _, _) => None, // Binary has no location field
            // Add other expression variants with locations as needed
            _ => None,
        };

        match expr {
            Expression::Literal(value) => self.generate_value(value, instructions),
            Expression::Variable(name) => {
                // Check if variable exists to provide better error messages
                if let Some(local) = self.find_local(name) {
                    instructions.push(Instruction::LocalGet(local.index));
                    Ok(WasmType::from(local.type_))
                } else {
                    // Collect all visible variables for better suggestions
                    let variables: Vec<&str> =
                        self.variable_map.keys().map(|s| s.as_str()).collect();

                    Err(CompilerError::variable_not_found_error(
                        name,
                        &variables,
                        loc.unwrap_or_default(),
                    ))
                }
            }
            Expression::Call(func_name, args) => {
                // Handle built-in type constructors first
                if func_name == "List" {
                    if !args.is_empty() {
                        return Err(CompilerError::codegen_error(
                            "List() constructor takes no arguments",
                            Some("List type is inferred from variable declaration: List<T> myList = List()".to_string()),
                            None
                        ));
                    }
                    // Create a new empty list using list allocator
                    instructions.push(Instruction::I32Const(0)); // size = 0 for empty list
                    if let Some(func_index) = self.get_function_index("list.allocate") {
                        instructions.push(Instruction::Call(func_index));
                    } else {
                        return Err(CompilerError::type_error(
                            "list.allocate function not found".to_string(),
                            None,
                            None,
                        ));
                    }
                    return Ok(WasmType::I32); // Lists are represented as I32 pointers
                }

                // Special handling for basic input function - convert string to ptr+len
                if func_name == "input" {
                    if args.len() != 1 {
                        return Err(CompilerError::detailed_type_error(
                            "input() function called with wrong number of arguments",
                            1,
                            args.len(),
                            None,
                            Some(format!(
                                "input() expects exactly 1 argument, but {} were provided",
                                args.len()
                            )),
                        ));
                    }

                    // Generate the string argument and convert to ptr+len
                    self.generate_string_for_import(&args[0], instructions)?;

                    // Call the imported function
                    if let Some(&function_index) = self.function_map.get("input") {
                        instructions.push(Instruction::Call(function_index));
                        return Ok(WasmType::I32); // Returns string pointer
                    } else {
                        return Err(CompilerError::codegen_error(
                            "input function not found",
                            None,
                            None,
                        ));
                    }
                }

                // Special handling for error function
                if func_name == "error" {
                    if args.len() != 1 {
                        return Err(CompilerError::detailed_type_error(
                            "error() function called with wrong number of arguments",
                            1,
                            args.len(),
                            None,
                            Some(format!(
                                "error() expects exactly 1 argument, but {} were provided",
                                args.len()
                            )),
                        ));
                    }

                    // Generate the error value - can be any type (string, number, integer, boolean)
                    let error_type = self.generate_expression(&args[0], instructions)?;

                    // Create error handling logic based on the type
                    // Drop the error value and create a simple error indicator
                    match error_type {
                        WasmType::I32 => {
                            // Integer or string or boolean - drop it
                            instructions.push(Instruction::Drop);
                        }
                        WasmType::F64 => {
                            // Float - drop it
                            instructions.push(Instruction::Drop);
                        }
                        WasmType::F32 => {
                            // Float32 - drop it
                            instructions.push(Instruction::Drop);
                        }
                        WasmType::I64 => {
                            // I64 - drop it
                            instructions.push(Instruction::Drop);
                        }
                        WasmType::V128 => {
                            // V128 - drop it
                            instructions.push(Instruction::Drop);
                        }
                        WasmType::Unit => {
                            // Unit type - nothing to drop
                        }
                    }

                    // For now, use Unreachable to halt execution immediately
                    // This ensures stack balance: no values on stack when reaching unreachable
                    instructions.push(Instruction::Unreachable);

                    return Ok(WasmType::I32); // Error function never actually returns, but we need a type
                }

                // Special handling for print functions - they use type-safe dispatch
                if func_name == "print" || func_name == "printl" {
                    if args.len() != 1 {
                        return Err(CompilerError::detailed_type_error(
                            format!("Print function '{func_name}' called with wrong number of arguments"),
                            1,
                            args.len(),
                            None,
                            Some(format!("Print functions expect exactly 1 argument, but {count} were provided", count = args.len()))
                        ));
                    }
                    // Generate print call - this handles the stack properly
                    self.generate_print_call(func_name, &args[0], instructions)?;
                    // Print functions are void - they don't leave anything on the stack
                    return Ok(WasmType::Unit); // Print functions are truly void
                }

                // Special handling for HTTP functions - call import functions directly
                if func_name == "http_get" || func_name == "http_delete" {
                    if args.len() != 1 {
                        return Err(CompilerError::detailed_type_error(
                            format!("HTTP function '{func_name}' called with wrong number of arguments"),
                            1,
                            args.len(),
                            None,
                            Some(format!("HTTP function '{func_name}' expects exactly 1 argument (URL), but {count} were provided", count = args.len()))
                        ));
                    }
                    // Generate HTTP call with URL parameter
                    self.generate_http_call(func_name, args, instructions)?;
                    return Ok(WasmType::I32); // String represented as I32 pointer
                }

                if func_name == "http_post" || func_name == "http_put" || func_name == "http_patch"
                {
                    if args.len() != 2 {
                        return Err(CompilerError::detailed_type_error(
                            format!("HTTP function '{func_name}' called with wrong number of arguments"),
                            2,
                            args.len(),
                            None,
                            Some(format!("HTTP function '{func_name}' expects exactly 2 arguments (URL, data), but {count} were provided", count = args.len()))
                        ));
                    }
                    // Generate HTTP call with URL and data parameters
                    self.generate_http_call(func_name, args, instructions)?;
                    return Ok(WasmType::I32); // String represented as I32 pointer
                }

                // Special handling for HTTP server functions
                if func_name == "_http_route" {
                    if args.len() != 3 {
                        return Err(CompilerError::detailed_type_error(
                            format!("_http_route called with wrong number of arguments"),
                            3,
                            args.len(),
                            None,
                            Some("_http_route expects 3 arguments: method (string), path (string), handler_idx (integer)".to_string())
                        ));
                    }
                    self.generate_http_call(func_name, args, instructions)?;
                    return Ok(WasmType::I32);
                }

                if func_name == "_http_listen" {
                    if args.len() != 1 {
                        return Err(CompilerError::detailed_type_error(
                            format!("_http_listen called with wrong number of arguments"),
                            1,
                            args.len(),
                            None,
                            Some("_http_listen expects 1 argument: port (integer)".to_string()),
                        ));
                    }
                    self.generate_http_call(func_name, args, instructions)?;
                    return Ok(WasmType::I32);
                }

                // Special handling for file I/O functions - call import functions directly
                if func_name == "file_read" {
                    if args.len() != 1 {
                        return Err(CompilerError::detailed_type_error(
                            format!(
                                "File function '{func_name}' called with wrong number of arguments"
                            ),
                            1,
                            args.len(),
                            None,
                            Some(format!(
                                "file_read expects exactly 1 argument (path), but {} were provided",
                                args.len()
                            )),
                        ));
                    }
                    self.generate_file_call(func_name, args, instructions)?;
                    return Ok(WasmType::I32); // File content represented as I32 pointer
                }

                if func_name == "file_write" || func_name == "file_append" {
                    if args.len() != 2 {
                        return Err(CompilerError::detailed_type_error(
                            format!("File function '{func_name}' called with wrong number of arguments"),
                            2,
                            args.len(),
                            None,
                            Some(format!("{func_name} expects exactly 2 arguments (path, content), but {count} were provided", count = args.len()))
                        ));
                    }
                    self.generate_file_call(func_name, args, instructions)?;
                    return Ok(WasmType::I32); // Success/error code as I32
                }

                if func_name == "file_exists" || func_name == "file_delete" {
                    if args.len() != 1 {
                        return Err(CompilerError::detailed_type_error(
                            format!(
                                "File function '{func_name}' called with wrong number of arguments"
                            ),
                            1,
                            args.len(),
                            None,
                            Some(format!(
                                "{} expects exactly 1 argument (path), but {} were provided",
                                func_name,
                                args.len()
                            )),
                        ));
                    }
                    self.generate_file_call(func_name, args, instructions)?;
                    return Ok(WasmType::I32); // Boolean/status code as I32
                }

                // Check if this is a constructor call (function name matches a class name)
                if self.class_table.contains_key(func_name) {
                    // This is a constructor call - redirect to constructor function
                    let constructor_name = format!("{func_name}_constructor");
                    if let Some(constructor_index) = self.get_function_index(&constructor_name) {
                        // Generate arguments
                        for arg in args {
                            self.generate_expression(arg, instructions)?;
                        }

                        instructions.push(Instruction::Call(constructor_index));
                        // Constructor returns an object (represented as I32 pointer)
                        return Ok(WasmType::I32);
                    } else {
                        return Err(CompilerError::codegen_error(
                            format!("Constructor for class '{func_name}' not found"),
                            Some("Make sure the class has a constructor defined".to_string()),
                            None,
                        ));
                    }
                }

                // First, determine argument types for signature-based function resolution
                let mut arg_types = Vec::new();
                let mut arg_instructions = Vec::new();
                for arg in args {
                    let mut temp_instructions = Vec::new();
                    let arg_type = self.generate_expression(arg, &mut temp_instructions)?;
                    arg_types.push(arg_type);
                    arg_instructions.push(temp_instructions);
                }

                // Try name-based function resolution first (gives precedence to user-defined functions)
                let func_index = self.get_function_index(func_name).or_else(|| {
                    // Check if this is an implicit method call in a class context
                    if let Some(ref current_class) = self.current_class_context {
                        // Try to find the method in the current class hierarchy
                        if let Some(method_index) =
                            self.find_method_in_hierarchy(current_class, func_name)
                        {
                            return Some(method_index);
                        }
                    }

                    self.instruction_generator
                        .get_function_index_by_signature(func_name, &arg_types)
                });

                // Check if function exists to provide better error messages
                if let Some(func_index) = func_index {
                    // Check argument count with support for default parameters
                    if let Some(func_type) =
                        self.instruction_generator.get_function_type(func_index)
                    {
                        let total_param_count = func_type.params().len();

                        // Check if we have the function definition for default parameter support
                        if let Some(func_def) = self.function_definitions.get(func_name).cloned() {
                            let required_param_count = func_def
                                .parameters
                                .iter()
                                .filter(|p| p.default_value.is_none())
                                .count();

                            // Validate argument count is within valid range
                            if args.len() < required_param_count || args.len() > total_param_count {
                                return Err(CompilerError::detailed_type_error(
                                    format!(
                                        "Function '{func_name}' called with wrong number of arguments"
                                    ),
                                    format!("{}-{}", required_param_count, total_param_count),
                                    args.len(),
                                    None,
                                    Some(format!(
                                        "Function '{}' requires {}-{} arguments, but {} were provided",
                                        func_name,
                                        required_param_count,
                                        total_param_count,
                                        args.len()
                                    )),
                                ));
                            }
                        } else {
                            // Fallback for functions without definitions (built-ins, imports)
                            if args.len() != total_param_count {
                                return Err(CompilerError::detailed_type_error(
                                    format!(
                                        "Function '{func_name}' called with wrong number of arguments"
                                    ),
                                    total_param_count,
                                    args.len(),
                                    None,
                                    Some(format!(
                                        "Function '{}' expects {} arguments, but {} were provided",
                                        func_name,
                                        total_param_count,
                                        args.len()
                                    )),
                                ));
                            }
                        }
                    }

                    // Add default values for missing arguments if needed
                    let mut complete_args = args.to_vec();
                    let mut complete_arg_types = arg_types.clone();
                    let mut complete_arg_instructions = arg_instructions.clone();

                    if let Some(func_def) = self.function_definitions.get(func_name).cloned() {
                        // Fill in missing arguments with default values
                        while complete_args.len() < func_def.parameters.len() {
                            let param_index = complete_args.len();
                            let param = &func_def.parameters[param_index];

                            if let Some(default_expr) = &param.default_value {
                                // Generate instructions for default value
                                let mut default_instructions = Vec::new();
                                let default_type = self
                                    .generate_expression(default_expr, &mut default_instructions)?;

                                complete_args.push(default_expr.clone());
                                complete_arg_types.push(default_type);
                                complete_arg_instructions.push(default_instructions);
                            } else {
                                // This should not happen if validation passed
                                return Err(CompilerError::codegen_error(
                                    format!(
                                        "Missing default value for parameter '{}' in function '{}'",
                                        param.name, func_name
                                    ),
                                    Some("This should not happen if validation passed".to_string()),
                                    None,
                                ));
                            }
                        }
                    }

                    // Generate code for arguments with type conversion using pre-generated instructions
                    if let Some(func_type) =
                        self.instruction_generator.get_function_type(func_index)
                    {
                        let expected_params = func_type.params();
                        for (i, (arg_type, arg_instr)) in complete_arg_types
                            .iter()
                            .zip(complete_arg_instructions.iter())
                            .enumerate()
                        {
                            // Add the argument instructions to the main instruction stream
                            instructions.extend_from_slice(arg_instr);

                            // Convert argument type if needed
                            if i < expected_params.len() {
                                let expected_type = match expected_params[i] {
                                    wasm_encoder::ValType::I32 => WasmType::I32,
                                    wasm_encoder::ValType::I64 => WasmType::I64,
                                    wasm_encoder::ValType::F32 => WasmType::F32,
                                    wasm_encoder::ValType::F64 => WasmType::F64,
                                    wasm_encoder::ValType::V128 => WasmType::V128,
                                    _ => *arg_type,
                                };

                                // Add conversion instruction if types don't match
                                match (*arg_type, expected_type) {
                                    (WasmType::I32, WasmType::F64) => {
                                        instructions.push(Instruction::F64ConvertI32S);
                                    }
                                    (WasmType::F64, WasmType::I32) => {
                                        instructions.push(Instruction::I32TruncF64S);
                                    }
                                    (WasmType::I32, WasmType::F32) => {
                                        instructions.push(Instruction::F32ConvertI32S);
                                    }
                                    (WasmType::F32, WasmType::I32) => {
                                        instructions.push(Instruction::I32TruncF32S);
                                    }
                                    // Add more conversions as needed
                                    _ => {
                                        // No conversion needed or supported
                                    }
                                }
                            }
                        }
                    } else {
                        // Fallback: use pre-generated argument instructions without type checking
                        for arg_instr in arg_instructions {
                            instructions.extend_from_slice(&arg_instr);
                        }
                    }

                    instructions.push(Instruction::Call(func_index));
                    self.get_function_return_type(func_index)
                } else {
                    // Collect all function names for better suggestions
                    let functions: Vec<&str> =
                        self.function_names.iter().map(|s| s.as_str()).collect();

                    Err(CompilerError::function_not_found_error(
                        func_name,
                        &functions,
                        loc.unwrap_or_default(),
                    ))
                }
            }
            Expression::Binary(left, op, right) => {
                self.generate_binary_operation(left, op, right, instructions)
            }
            Expression::ListAccess(array, index) => {
                // Generate list access with type-safe value loading
                // First, generate the list expression (should be a pointer)
                let list_type = self.generate_expression(array, instructions)?;
                if list_type != WasmType::I32 {
                    return Err(CompilerError::codegen_error(
                        "List access requires list pointer (I32)",
                        Some("The list must be a valid list pointer".to_string()),
                        None,
                    ));
                }

                // Then, generate the index expression
                let index_type = self.generate_expression(index, instructions)?;
                if index_type != WasmType::I32 {
                    return Err(CompilerError::codegen_error(
                        "List index must be I32",
                        Some("The list index must be an integer".to_string()),
                        None,
                    ));
                }

                // Call the appropriate list access function
                if let Some(list_get_index) = self.function_map.get("list.get") {
                    instructions.push(Instruction::Call(*list_get_index));
                } else {
                    return Err(CompilerError::codegen_error(
                        "No list access function found (list.get)",
                        Some("Register list operations to enable list access".to_string()),
                        None,
                    ));
                }

                // The list access function returns a pointer to the element (i32)
                // We need to ensure this is properly consumed by subsequent operations

                // The list access function returns a pointer to the element
                // Now load the actual value based on the expected type
                let element_type = self.infer_list_element_type(array)?;

                match element_type {
                    WasmType::I32 => {
                        // Load 32-bit integer
                        instructions.push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                        Ok(WasmType::I32)
                    }
                    WasmType::F64 => {
                        // Load 64-bit float
                        instructions.push(Instruction::F64Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        Ok(WasmType::F64)
                    }
                    _ => {
                        // For other types, default to i32
                        instructions.push(Instruction::I32Load(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                        Ok(WasmType::I32)
                    }
                }
            }
            Expression::PropertyAssignment {
                object,
                property,
                value,
                location: _,
            } => {
                // Handle property assignments like list.type = "line"
                match property.as_str() {
                    "type" => {
                        // List behavior assignment: list.type = "line"
                        self.generate_expression(object, instructions)?; // List pointer
                        self.generate_expression(value, instructions)?; // Behavior string

                        // Call List.setBehavior function
                        if let Some(function_index) = self.function_map.get("List.setBehavior") {
                            instructions.push(Instruction::Call(*function_index));
                        } else {
                            // Fallback: just drop the values
                            instructions.push(Instruction::Drop);
                            instructions.push(Instruction::Drop);
                        }
                        Ok(WasmType::I32) // Void
                    }
                    _ => {
                        // Generic property assignment - for now, no-op
                        self.generate_expression(object, instructions)?;
                        self.generate_expression(value, instructions)?;
                        instructions.push(Instruction::Drop);
                        instructions.push(Instruction::Drop);
                        Ok(WasmType::I32) // Void
                    }
                }
            }
            Expression::MethodCall {
                object,
                method,
                arguments,
                location: _,
            } => {
                // First check if this is a method call on a user-defined class
                if let Expression::Variable(var_name) = object.as_ref() {
                    // Get the actual type from our variable types map
                    if let Some(var_type) = self.variable_types.get(var_name) {
                        // Handle both Type::Class and Type::Object for class instances
                        let class_name = match var_type {
                            Type::Class { name, type_args: _ } => Some(name.as_str()),
                            Type::Object(name) => Some(name.as_str()),
                            _ => None,
                        };

                        if let Some(class_name) = class_name {
                            // Search for method in class hierarchy (current class and all parent classes)
                            if let Some(method_index) =
                                self.find_method_in_hierarchy(class_name, method)
                            {
                                // Generate arguments
                                for arg in arguments {
                                    self.generate_expression(arg, instructions)?;
                                }
                                instructions.push(Instruction::Call(method_index));
                                // Get the actual return type from the method signature
                                return self.get_function_return_type(method_index);
                            }
                        }
                    }
                }

                // Check if this is a type conversion method only if not a class method
                if self.is_type_conversion_method(method) {
                    return self.generate_type_conversion_method(object, method, instructions);
                }

                // OLD CODE PATH DELETED - Input method calls now handled by MIR codegen
                // with single-parameter (ptr only) signature per system specification

                // Check for built-in module calls first
                if let Expression::Variable(module_name) = object.as_ref() {
                    match module_name.as_str() {
                        "http" => {
                            // HTTP functions need special handling for string expansion
                            let function_name = format!("{module_name}.{method}");

                            // HTTP functions that take a URL (ptr, len)
                            if matches!(method.as_str(), "get" | "delete" | "head" | "options") {
                                if arguments.len() != 1 {
                                    return Err(CompilerError::codegen_error(
                                        format!("HTTP method '{method}' expects 1 argument"),
                                        None,
                                        None,
                                    ));
                                }
                                // Generate URL string with proper expansion to (ptr, len)
                                self.generate_string_for_import(&arguments[0], instructions)?;
                            }
                            // HTTP functions that take URL and data (ptr, len, ptr, len)
                            else if matches!(method.as_str(), "post" | "put" | "patch") {
                                if arguments.len() != 2 {
                                    return Err(CompilerError::codegen_error(
                                        format!("HTTP method '{method}' expects 2 arguments"),
                                        None,
                                        None,
                                    ));
                                }
                                // Generate URL string with proper expansion to (ptr, len)
                                self.generate_string_for_import(&arguments[0], instructions)?;
                                // Generate data string with proper expansion to (ptr, len)
                                self.generate_string_for_import(&arguments[1], instructions)?;
                            } else {
                                return Err(CompilerError::codegen_error(
                                    format!("Unknown HTTP method: {method}"),
                                    None,
                                    None,
                                ));
                            }

                            // Find and call the function
                            if let Some(&function_index) = self.function_map.get(&function_name) {
                                instructions.push(Instruction::Call(function_index));
                                return Ok(self.get_function_return_type_by_name(&function_name));
                            } else {
                                return Err(CompilerError::codegen_error(
                                    format!("Function '{function_name}' not found"),
                                    None,
                                    None,
                                ));
                            }
                        }
                        "math" | "array" | "string" | "file" | "list" => {
                            let mut function_name = format!("{module_name}.{method}");

                            // Special handling for polymorphic math.abs - determine the correct function variant
                            if function_name == "math.abs" && !arguments.is_empty() {
                                // Determine the argument type to select correct math.abs variant
                                let arg_type = match &arguments[0] {
                                    Expression::Variable(name) => {
                                        // Look up variable type in variable_types
                                        if let Some(var_type) = self.variable_types.get(name) {
                                            match var_type {
                                                Type::Integer => WasmType::I32,
                                                Type::Number => WasmType::F64,
                                                Type::IntegerSized { bits: 64, .. } => {
                                                    WasmType::I64
                                                }
                                                Type::IntegerSized { bits: 32, .. } => {
                                                    WasmType::I32
                                                }
                                                Type::NumberSized { bits: 64 } => WasmType::F64,
                                                Type::NumberSized { bits: 32 } => WasmType::F32,
                                                _ => WasmType::I32, // Default to I32 for other types
                                            }
                                        } else {
                                            WasmType::I32 // Default fallback
                                        }
                                    }
                                    Expression::Literal(Value::Integer(_)) => WasmType::I32,
                                    Expression::Literal(Value::Number(_)) => WasmType::F64,
                                    Expression::Literal(Value::Integer64(_)) => WasmType::I64,
                                    Expression::Unary(UnaryOperator::Negate, inner_expr) => {
                                        // Handle unary negation - determine type of inner expression
                                        match inner_expr.as_ref() {
                                            Expression::Literal(Value::Integer(_)) => WasmType::I32,
                                            Expression::Literal(Value::Number(_)) => WasmType::F64,
                                            _ => WasmType::I32, // Default to I32
                                        }
                                    }
                                    _ => {
                                        // For complex expressions, try to infer the type
                                        match self
                                            .generate_expression(&arguments[0], &mut Vec::new())
                                        {
                                            Ok(wasm_type) => wasm_type,
                                            Err(_) => WasmType::I32, // Default fallback
                                        }
                                    }
                                };

                                // Select the appropriate math.abs function based on argument type
                                function_name = match arg_type {
                                    WasmType::I32 => "math.abs.i32".to_string(),
                                    WasmType::F64 => "math.abs".to_string(),
                                    WasmType::I64 => "math.abs".to_string(), // Use F64 version for I64
                                    WasmType::F32 => "math.abs".to_string(), // Use F64 version for F32
                                    WasmType::V128 | WasmType::Unit => "math.abs".to_string(), // Default to F64 version
                                };
                            }

                            // Generate arguments
                            for arg in arguments {
                                self.generate_expression(arg, instructions)?;
                            }

                            // Find and call the function
                            if let Some(&function_index) = self.function_map.get(&function_name) {
                                instructions.push(Instruction::Call(function_index));

                                // Return the appropriate type based on the function
                                return Ok(self.get_function_return_type_by_name(&function_name));
                            } else {
                                return Err(CompilerError::codegen_error(
                                    format!("Function '{function_name}' not found"),
                                    None,
                                    None,
                                ));
                            }
                        }
                        _ => {}
                    }
                }

                // Check for nested property access method calls (like compare.integer.greaterThan)
                if let Expression::PropertyAccess {
                    object: nested_object,
                    property,
                    ..
                } = object.as_ref()
                {
                    if let Expression::Variable(base_name) = nested_object.as_ref() {
                        // This handles cases like compare.integer.greaterThan(a, b)
                        // where base_name="compare", property="integer", method="greaterThan"
                        let qualified_function_name = format!("{base_name}.{property}.{method}");

                        // Generate arguments
                        for arg in arguments {
                            self.generate_expression(arg, instructions)?;
                        }

                        // Find and call the qualified function
                        if let Some(&function_index) =
                            self.function_map.get(&qualified_function_name)
                        {
                            instructions.push(Instruction::Call(function_index));

                            // Return the appropriate type based on the function
                            return Ok(
                                self.get_function_return_type_by_name(&qualified_function_name)
                            );
                        } else {
                            return Err(CompilerError::codegen_error(
                                format!("Function '{qualified_function_name}' not found"),
                                None,
                                None,
                            ));
                        }
                    }
                }

                // Check if this is a static method call on a built-in class first
                if let Expression::Variable(class_name) = object.as_ref() {
                    // Try to handle as built-in static method call
                    if let Some(result_type) = self.generate_builtin_static_method_call(
                        class_name,
                        method,
                        arguments,
                        instructions,
                    )? {
                        return Ok(result_type);
                    }
                }

                // Handle method calls on different types (instance methods)
                // First, check if this is a method call on a typed variable that should map to MethodStyleManager functions
                if let Expression::Variable(var_name) = object.as_ref() {
                    if let Some(var_type) = self.variable_types.get(var_name) {
                        // Map the Clean Language type to a type name for method resolution
                        let type_name = match var_type {
                            crate::ast::Type::Integer | crate::ast::Type::IntegerSized { .. } => {
                                "integer"
                            }
                            crate::ast::Type::Number | crate::ast::Type::NumberSized { .. } => {
                                "number"
                            }
                            crate::ast::Type::String => "string",
                            crate::ast::Type::Boolean => "boolean",
                            crate::ast::Type::List(_) => "list",
                            _ => "value", // fallback for unknown types
                        };

                        // Try to find the type-based method function
                        let type_method_name = format!("{type_name}.{method}");

                        if let Some(&function_index) = self.function_map.get(&type_method_name) {
                            // Generate the object expression (variable value)
                            self.generate_expression(object, instructions)?;

                            // Generate arguments
                            for arg in arguments {
                                self.generate_expression(arg, instructions)?;
                            }

                            // Call the method function
                            instructions.push(Instruction::Call(function_index));

                            // Return appropriate type based on method
                            let return_type = match method.as_str() {
                                "toString" => WasmType::I32, // String pointer
                                "toInteger" => WasmType::I32,
                                "toNumber" => WasmType::F64,
                                "toBoolean" => WasmType::I32,
                                "isDefined" | "isNotDefined" | "isEmpty" | "isNotEmpty" => {
                                    WasmType::I32
                                } // Boolean
                                "keepBetween" => {
                                    if type_name == "number" {
                                        WasmType::F64
                                    } else {
                                        WasmType::I32
                                    }
                                }
                                "mustBeTrue" | "mustBeFalse" | "mustBeEqual" | "mustNotBeEqual" => {
                                    WasmType::I32
                                } // Void (represented as I32)
                                "length" => WasmType::I32,
                                _ => WasmType::I32, // Default
                            };

                            return Ok(return_type);
                        }
                    }
                }

                // If not a type-based method call, proceed with normal handling
                self.generate_expression(object, instructions)?;

                // Generate arguments
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Check if this is a List method call
                if let Expression::Variable(_) = object.as_ref() {
                    // Route List methods to proper import functions
                    match method.as_str() {
                        "add" | "push" => {
                            // List.add(item) / List.push(item) - calls list.add import (in-place)
                            // push is an alias for add per spec
                            // Stack: [list_ptr, item] -> [list_ptr]
                            if let Some(add_index) = self.get_function_index("list.add") {
                                instructions.push(Instruction::Call(add_index));
                                return Ok(WasmType::I32);
                            }
                            // Fallback to list.push for compatibility
                            if let Some(push_index) = self.get_function_index("list.push") {
                                instructions.push(Instruction::Call(push_index));
                                return Ok(WasmType::I32);
                            }
                            return Err(CompilerError::codegen_error(
                                "List.add requires list.add import - ensure bridge provides this function",
                                None,
                                None,
                            ));
                        }
                        "remove" | "pop" | "removeLast" => {
                            // List.remove() / List.pop() / List.removeLast() - calls list.pop import
                            // removeLast is the canonical spec name for pop
                            // Stack: [list_ptr] -> [removed_item]
                            if let Some(pop_index) = self.get_function_index("list.pop") {
                                instructions.push(Instruction::Call(pop_index));
                                return Ok(WasmType::I32);
                            }
                            return Err(CompilerError::codegen_error(
                                "List.removeLast requires list.pop import - ensure bridge provides this function",
                                None,
                                None,
                            ));
                        }
                        "size" | "length" => {
                            // List.size() / List.length() - calls list.length function
                            if let Some(length_index) = self.get_function_index("list.length") {
                                instructions.push(Instruction::Call(length_index));
                                return Ok(WasmType::I32);
                            }
                            return Err(CompilerError::codegen_error(
                                "List.size requires list.length function",
                                None,
                                None,
                            ));
                        }
                        "peek" => {
                            // List.peek() - get last element without removing
                            // Implementation: get element at index (size - 1)
                            // Stack: [list_ptr] -> duplicate -> get size -> subtract 1 -> get element
                            if let Some(length_index) = self.get_function_index("list.length") {
                                if let Some(get_index) = self.get_function_index("list.get") {
                                    // Duplicate list pointer (need it twice: once for size, once for get)
                                    instructions.push(Instruction::LocalGet(0)); // Re-get list_ptr
                                    instructions.push(Instruction::Call(length_index)); // Get size
                                    instructions.push(Instruction::I32Const(1));
                                    instructions.push(Instruction::I32Sub); // size - 1
                                                                            // Stack now: [list_ptr, last_index]
                                    instructions.push(Instruction::Call(get_index));
                                    return Ok(WasmType::I32);
                                }
                            }
                            return Err(CompilerError::codegen_error(
                                "List.peek requires array_length and array_get functions",
                                None,
                                None,
                            ));
                        }
                        "contains" => {
                            // List.contains(item) - calls list.contains import
                            // Stack: [list_ptr, item] -> [0 or 1]
                            if let Some(contains_index) = self.get_function_index("list.contains") {
                                instructions.push(Instruction::Call(contains_index));
                                return Ok(WasmType::I32);
                            }
                            return Err(CompilerError::codegen_error(
                                "List.contains requires list.contains import - ensure bridge provides this function",
                                None,
                                None,
                            ));
                        }
                        "get" => {
                            // List.get(index) - calls list.get import
                            if let Some(get_index) = self.get_function_index("list.get") {
                                instructions.push(Instruction::Call(get_index));
                                return Ok(WasmType::I32);
                            }
                            return Err(CompilerError::codegen_error(
                                "List.get requires list.get import - ensure bridge provides this function",
                                None,
                                None,
                            ));
                        }
                        "set" => {
                            // List.set(index, value) - calls list.set import
                            if let Some(set_index) = self.get_function_index("list.set") {
                                instructions.push(Instruction::Call(set_index));
                                return Ok(WasmType::I32);
                            }
                            return Err(CompilerError::codegen_error(
                                "List.set requires list.set import - ensure bridge provides this function",
                                None,
                                None,
                            ));
                        }
                        _ => {
                            // Fall through to regular method handling
                        }
                    }
                }

                // Handle built-in method-style functions first
                match method.as_str() {
                    "keepBetween" => {
                        // value.keepBetween(min, max) - keep value between bounds
                        // Arguments are already on stack: object, arg1, arg2
                        // We need to call the appropriate keepBetween function
                        if let Some(keep_between_index) = self.get_function_index("keepBetween") {
                            instructions.push(Instruction::Call(keep_between_index));
                            return Ok(WasmType::I32); // Returns the bounded value
                        } else if let Some(keep_between_float_index) =
                            self.get_function_index("keepBetweenFloat")
                        {
                            instructions.push(Instruction::Call(keep_between_float_index));
                            return Ok(WasmType::F64); // Returns the bounded float value
                        } else {
                            return Err(CompilerError::codegen_error(
                                "keepBetween function not found",
                                None,
                                None,
                            ));
                        }
                    }
                    "length" => {
                        // value.length() - get length of string or array
                        if let Some(length_index) = self.get_function_index("string.length") {
                            instructions.push(Instruction::Call(length_index));
                            return Ok(WasmType::I32); // Returns length
                        } else if let Some(length_index) = self.get_function_index("array.length") {
                            instructions.push(Instruction::Call(length_index));
                            return Ok(WasmType::I32); // Returns length
                        } else if let Some(length_index) = self.get_function_index("length") {
                            instructions.push(Instruction::Call(length_index));
                            return Ok(WasmType::I32); // Returns length
                        } else {
                            return Err(CompilerError::codegen_error(
                                "length function not found",
                                None,
                                None,
                            ));
                        }
                    }
                    "isEmpty" => {
                        // value.isEmpty() - check if empty
                        if let Some(is_empty_index) = self.get_function_index("value.isEmpty") {
                            instructions.push(Instruction::Call(is_empty_index));
                            return Ok(WasmType::I32); // Returns boolean
                        } else {
                            return Err(CompilerError::codegen_error(
                                "isEmpty function not found",
                                None,
                                None,
                            ));
                        }
                    }
                    "isNotEmpty" => {
                        // value.isNotEmpty() - check if not empty
                        if let Some(is_not_empty_index) =
                            self.get_function_index("value.isNotEmpty")
                        {
                            instructions.push(Instruction::Call(is_not_empty_index));
                            return Ok(WasmType::I32); // Returns boolean
                        } else {
                            return Err(CompilerError::codegen_error(
                                "isNotEmpty function not found",
                                None,
                                None,
                            ));
                        }
                    }
                    "isDefined" => {
                        // value.isDefined() - check if defined
                        if let Some(is_defined_index) = self.get_function_index("value.isDefined") {
                            instructions.push(Instruction::Call(is_defined_index));
                            return Ok(WasmType::I32); // Returns boolean
                        } else {
                            return Err(CompilerError::codegen_error(
                                "isDefined function not found",
                                None,
                                None,
                            ));
                        }
                    }
                    "isNotDefined" => {
                        // value.isNotDefined() - check if not defined
                        if let Some(is_not_defined_index) =
                            self.get_function_index("value.isNotDefined")
                        {
                            instructions.push(Instruction::Call(is_not_defined_index));
                            return Ok(WasmType::I32); // Returns boolean
                        } else {
                            return Err(CompilerError::codegen_error(
                                "isNotDefined function not found",
                                None,
                                None,
                            ));
                        }
                    }
                    "toInteger" | "toFloat" | "toString" | "toBoolean" => {
                        // Type conversion methods - delegate to existing implementation
                        return self.generate_type_conversion_method(object, method, instructions);
                    }
                    _ => {} // Fall through to existing method handling
                }

                // Handle specific array/collection methods
                match method.as_str() {
                    "at" => {
                        // List.at(index) - 1-indexed access
                        // Convert 1-indexed to 0-indexed by subtracting 1
                        instructions.push(Instruction::I32Const(1));
                        instructions.push(Instruction::I32Sub);
                        instructions.push(Instruction::Call(self.get_array_get()));
                        Ok(WasmType::I32)
                    }
                    "length" => {
                        // List.length() - get list length
                        instructions.push(Instruction::Call(self.get_array_length()));
                        Ok(WasmType::I32)
                    }
                    "get" => {
                        // array.get(index) - 0-indexed access
                        instructions.push(Instruction::Call(self.get_array_get()));
                        Ok(WasmType::I32)
                    }
                    "set" => {
                        // array.set(index, value) - 0-indexed assignment
                        if let Some(set_index) = self.get_function_index("list.set") {
                            instructions.push(Instruction::Call(set_index));
                            Ok(WasmType::I32) // Void represented as I32
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.set function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "push" => {
                        // array.push(item) - add element to end
                        if let Some(push_index) = self.get_function_index("list.push") {
                            instructions.push(Instruction::Call(push_index));
                            Ok(WasmType::I32) // Returns new array pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.push function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "pop" => {
                        // array.pop() - remove and return last element
                        if let Some(pop_index) = self.get_function_index("list.pop") {
                            instructions.push(Instruction::Call(pop_index));
                            Ok(WasmType::I32) // Returns popped element
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.pop function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "contains" => {
                        // array.contains(item) - check if item exists
                        if let Some(contains_index) = self.get_function_index("list.contains") {
                            instructions.push(Instruction::Call(contains_index));
                            Ok(WasmType::I32) // Returns boolean (0/1)
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.contains function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "indexOf" => {
                        // array.indexOf(item) - find index of item
                        if let Some(index_of_index) = self.get_function_index("list.indexOf") {
                            instructions.push(Instruction::Call(index_of_index));
                            Ok(WasmType::I32) // Returns index (-1 if not found)
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.indexOf function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "slice" => {
                        // array.slice(start, end) - extract portion of array
                        if let Some(slice_index) = self.get_function_index("list.slice") {
                            instructions.push(Instruction::Call(slice_index));
                            Ok(WasmType::I32) // Returns new array pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.slice function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "concat" => {
                        // array.concat(other) - combine with another array
                        if let Some(concat_index) = self.get_function_index("list.concat") {
                            instructions.push(Instruction::Call(concat_index));
                            Ok(WasmType::I32) // Returns new array pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.concat function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "reverse" => {
                        // array.reverse() - reverse array elements
                        if let Some(reverse_index) = self.get_function_index("list.reverse") {
                            instructions.push(Instruction::Call(reverse_index));
                            Ok(WasmType::I32) // Returns new array pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.reverse function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "join" => {
                        // array.join(separator) - join elements into string
                        if let Some(join_index) = self.get_function_index("list.join") {
                            instructions.push(Instruction::Call(join_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "list.join function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "isEmpty" => {
                        // array.isEmpty() - check if array is empty
                        // Get array length and compare to 0
                        instructions.push(Instruction::Call(self.get_array_length()));
                        instructions.push(Instruction::I32Const(0));
                        instructions.push(Instruction::I32Eq);
                        Ok(WasmType::I32) // Returns boolean (0/1)
                    }
                    "isNotEmpty" => {
                        // array.isNotEmpty() - check if array has elements
                        // Get array length and compare to 0
                        instructions.push(Instruction::Call(self.get_array_length()));
                        instructions.push(Instruction::I32Const(0));
                        instructions.push(Instruction::I32Ne);
                        Ok(WasmType::I32) // Returns boolean (0/1)
                    }
                    "first" => {
                        // array.first() - get first element
                        instructions.push(Instruction::I32Const(0)); // Index 0
                        instructions.push(Instruction::Call(self.get_array_get()));
                        Ok(WasmType::I32)
                    }
                    "last" => {
                        // array.last() - get last element
                        // Get length - 1 as index
                        instructions.push(Instruction::LocalTee(0)); // Store array pointer in local 0
                        instructions.push(Instruction::Call(self.get_array_length()));
                        instructions.push(Instruction::I32Const(1));
                        instructions.push(Instruction::I32Sub); // length - 1
                        instructions.push(Instruction::LocalGet(0)); // Get array pointer back
                        instructions.push(Instruction::LocalGet(1)); // Get calculated index
                        instructions.push(Instruction::Call(self.get_array_get()));
                        Ok(WasmType::I32)
                    }
                    "map" => {
                        // array.map(callback) - transform each element
                        if let Some(map_index) = self.get_function_index("array.map") {
                            instructions.push(Instruction::Call(map_index));
                            Ok(WasmType::I32) // Returns new array pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "array.map function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "iterate" => {
                        // array.iterate(callback) - iterate over elements
                        if let Some(iterate_index) = self.get_function_index("array.iterate") {
                            instructions.push(Instruction::Call(iterate_index));
                            Ok(WasmType::I32) // Void represented as I32
                        } else {
                            Err(CompilerError::codegen_error(
                                "array.iterate function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    // String methods
                    "trimStart" => {
                        if let Some(trim_start_index) = self.get_function_index("string.trimStart")
                        {
                            instructions.push(Instruction::Call(trim_start_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.trimStart function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "trimEnd" => {
                        if let Some(trim_end_index) = self.get_function_index("string.trimEnd") {
                            instructions.push(Instruction::Call(trim_end_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.trimEnd function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "lastIndexOf" => {
                        if let Some(last_index_of_index) =
                            self.get_function_index("string.lastIndexOf")
                        {
                            instructions.push(Instruction::Call(last_index_of_index));
                            Ok(WasmType::I32) // Returns index
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.lastIndexOf function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "substring" => {
                        if let Some(substring_index) = self.get_function_index("string.substring") {
                            instructions.push(Instruction::Call(substring_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.substring function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "replace" => {
                        if let Some(replace_index) = self.get_function_index("string.replace") {
                            instructions.push(Instruction::Call(replace_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.replace function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "padStart" => {
                        if let Some(pad_start_index) = self.get_function_index("string.padStart") {
                            instructions.push(Instruction::Call(pad_start_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.padStart function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "trim" => {
                        if let Some(trim_index) = self.get_function_index("string.trim") {
                            instructions.push(Instruction::Call(trim_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.trim function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "toLowerCase" => {
                        if let Some(to_lower_index) = self.get_function_index("string.toLowerCase")
                        {
                            instructions.push(Instruction::Call(to_lower_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.toLowerCase function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "toUpperCase" => {
                        if let Some(to_upper_index) = self.get_function_index("string.toUpperCase")
                        {
                            instructions.push(Instruction::Call(to_upper_index));
                            Ok(WasmType::I32) // Returns string pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "string.toUpperCase function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    "startsWith" => {
                        let starts_with_function = ast::Function {
                            name: "string_starts_with".to_string(),
                            type_parameters: vec![],
                            type_constraints: vec![],
                            parameters: vec![
                                ast::Parameter {
                                    name: "s".to_string(),
                                    type_: ast::Type::String,
                                    default_value: None,
                                },
                                ast::Parameter {
                                    name: "prefix".to_string(),
                                    type_: ast::Type::String,
                                    default_value: None,
                                },
                            ],
                            return_type: ast::Type::Boolean,
                            body: vec![ast::Statement::Return {
                                value: Some(ast::Expression::Call(
                                    "string_starts_with_impl".to_string(),
                                    vec![
                                        ast::Expression::Variable("s".to_string()),
                                        ast::Expression::Variable("prefix".to_string()),
                                    ],
                                )),
                                location: None,
                            }],
                            description: Some(
                                "Checks if a string starts with a given prefix.".to_string(),
                            ),
                            syntax: ast::FunctionSyntax::Simple,
                            visibility: ast::Visibility::Public,
                            modifier: ast::FunctionModifier::None,
                            location: None,
                        };
                        self.prepare_function_type(&starts_with_function)?;
                        self.generate_function(&starts_with_function)?;
                        Ok(WasmType::I32) // Returns boolean as I32
                    }
                    "endsWith" => {
                        let ends_with_function = ast::Function {
                            name: "string_ends_with".to_string(),
                            type_parameters: vec![],
                            type_constraints: vec![],
                            parameters: vec![
                                ast::Parameter {
                                    name: "s".to_string(),
                                    type_: ast::Type::String,
                                    default_value: None,
                                },
                                ast::Parameter {
                                    name: "suffix".to_string(),
                                    type_: ast::Type::String,
                                    default_value: None,
                                },
                            ],
                            return_type: ast::Type::Boolean,
                            body: vec![ast::Statement::Return {
                                value: Some(ast::Expression::Call(
                                    "string_ends_with_impl".to_string(),
                                    vec![
                                        ast::Expression::Variable("s".to_string()),
                                        ast::Expression::Variable("suffix".to_string()),
                                    ],
                                )),
                                location: None,
                            }],
                            description: Some(
                                "Checks if a string ends with a given suffix.".to_string(),
                            ),
                            syntax: ast::FunctionSyntax::Simple,
                            visibility: ast::Visibility::Public,
                            modifier: ast::FunctionModifier::None,
                            location: None,
                        };
                        self.prepare_function_type(&ends_with_function)?;
                        self.generate_function(&ends_with_function)?;
                        Ok(WasmType::I32) // Returns boolean as I32
                    }
                    // Matrix methods
                    "transpose" => {
                        if let Some(transpose_index) = self.get_function_index("matrix.transpose") {
                            instructions.push(Instruction::Call(transpose_index));
                            Ok(WasmType::I32) // Returns matrix pointer
                        } else {
                            Err(CompilerError::codegen_error(
                                "matrix.transpose function not found",
                                None,
                                None,
                            ))
                        }
                    }
                    _ => {
                        // Fallback: try common class names if type information is not available
                        if let Expression::Variable(_var_name) = object.as_ref() {
                            let possible_class_names =
                                vec!["Person", "Rectangle", "Circle", "Point"];
                            for class_name in &possible_class_names {
                                let class_method_name = format!("{class_name}_{method}");
                                if let Some(method_index) =
                                    self.get_function_index(&class_method_name)
                                {
                                    instructions.push(Instruction::Call(method_index));
                                    // Get actual return type from function signature
                                    return Ok(
                                        self.get_function_return_type_by_name(&class_method_name)
                                    );
                                }
                            }
                        }

                        // Try to find a global function with the method name (method dispatch)
                        if let Some(method_index) = self.get_function_index(method) {
                            instructions.push(Instruction::Call(method_index));
                            // Get actual return type from function signature
                            return Ok(self.get_function_return_type_by_name(method));
                        }

                        // Try to find a function with the method name (fallback for arrays)
                        if let Some(method_index) =
                            self.get_function_index(&format!("array_{method}"))
                        {
                            instructions.push(Instruction::Call(method_index));
                            Ok(WasmType::I32) // Default return type
                        } else {
                            Err(CompilerError::codegen_error(
                                format!("Method '{method}' not found"),
                                None,
                                None,
                            ))
                        }
                    }
                }
            }
            Expression::MatrixAccess(matrix, row, col) => {
                self.generate_expression(matrix, instructions)?;
                self.generate_expression(row, instructions)?;
                self.generate_expression(col, instructions)?;
                instructions.push(Instruction::Call(self.get_matrix_get()));
                Ok(WasmType::F64)
            }
            Expression::StringInterpolation(parts) => {
                // Handle string interpolation by concatenating parts
                if parts.is_empty() {
                    // Empty interpolation, return empty string
                    let string_ptr = self.allocate_string("")?;
                    instructions.push(Instruction::I32Const(string_ptr as i32));
                    return Ok(WasmType::I32);
                }

                // Allocate temp locals for string concatenation
                // FIXED: Only need temp_local_2 for storing current part, result_local for accumulating result
                // No longer expanding strings to (ptr, len) pairs - using length-prefixed pointers directly
                let temp_local_2 = self.add_local(WasmType::I32);
                let result_local = self.add_local(WasmType::I32);

                let mut has_result = false;

                for (i, part) in parts.iter().enumerate() {
                    // Generate the string pointer for this part
                    match part {
                        ast::StringPart::Text(text) => {
                            // Allocate string literal
                            let string_ptr = self.allocate_string(text)?;
                            instructions.push(Instruction::I32Const(string_ptr as i32));
                        }
                        ast::StringPart::Interpolation(expr) => {
                            // Generate the expression and convert to string if needed
                            let expr_type = self.generate_expression(expr, instructions)?;

                            // Convert to string based on the expression type
                            match expr_type {
                                WasmType::I32 => {
                                    if !self.is_string_type(expr) {
                                        // Integer value, convert to string
                                        if let Some(int_to_string_index) =
                                            self.get_function_index("int_to_string")
                                        {
                                            instructions
                                                .push(Instruction::Call(int_to_string_index));
                                        } else {
                                            instructions.push(Instruction::Drop);
                                            let fallback_str = self.allocate_string("0")?;
                                            instructions
                                                .push(Instruction::I32Const(fallback_str as i32));
                                        }
                                    }
                                }
                                WasmType::F64 => {
                                    if let Some(float_to_string_index) =
                                        self.get_function_index("float_to_string")
                                    {
                                        instructions.push(Instruction::Call(float_to_string_index));
                                    } else {
                                        instructions.push(Instruction::Drop);
                                        let fallback_str = self.allocate_string("0.0")?;
                                        instructions
                                            .push(Instruction::I32Const(fallback_str as i32));
                                    }
                                }
                                _ => {
                                    instructions.push(Instruction::Drop);
                                    let fallback_str = self.allocate_string("[object]")?;
                                    instructions.push(Instruction::I32Const(fallback_str as i32));
                                }
                            }
                        }
                    }

                    // Now we have a string pointer on the stack for this part
                    if i == 0 {
                        // First part - store as initial result
                        instructions.push(Instruction::LocalSet(result_local));
                        has_result = true;
                    } else {
                        // Subsequent parts - concatenate with result
                        // Stack: [current_part_ptr]
                        // Store current part to temp
                        instructions.push(Instruction::LocalSet(temp_local_2));

                        // FIXED: native string.concat expects 2 args: (str_ptr1, str_ptr2)
                        // Each pointer points to a length-prefixed string: [4-byte len][content]
                        // DO NOT expand to (content_ptr, len) pairs

                        // Push first string pointer (result so far)
                        instructions.push(Instruction::LocalGet(result_local));

                        // Push second string pointer (current part)
                        instructions.push(Instruction::LocalGet(temp_local_2));

                        // Call string.concat(ptr1, ptr2) -> result_ptr
                        instructions.push(Instruction::Call(self.get_string_concat_index()?));

                        // Store result for next iteration
                        instructions.push(Instruction::LocalSet(result_local));
                    }
                }

                // Push final result to stack
                if has_result {
                    instructions.push(Instruction::LocalGet(result_local));
                } else {
                    let empty_str = self.allocate_string("")?;
                    instructions.push(Instruction::I32Const(empty_str as i32));
                }

                Ok(WasmType::I32) // String type is represented as I32 pointer
            }
            Expression::ObjectCreation {
                class_name,
                arguments,
                location: _,
            } => {
                // Handle object creation (constructor calls)

                // Generate arguments
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Create constructor function name
                let constructor_name = format!("{class_name}_constructor");

                // Find the constructor function index
                if let Some(constructor_index) = self.get_function_index(&constructor_name) {
                    instructions.push(Instruction::Call(constructor_index));
                    // Constructor returns an object (represented as I32 pointer)
                    Ok(WasmType::I32)
                } else {
                    Err(CompilerError::codegen_error(
                        format!("Constructor for class '{class_name}' not found"),
                        Some("Make sure the class has a constructor defined".to_string()),
                        None,
                    ))
                }
            }
            Expression::StaticMethodCall {
                namespace,
                class_name,
                method,
                arguments,
                location: _,
            } => {
                // Handle namespace.class.method() calls (e.g., compare.integer.greaterThan)
                // For now, treat namespace calls as built-in methods
                let full_class_name = if !namespace.is_empty() {
                    format!("{}.{}", namespace.join("."), class_name)
                } else {
                    class_name.clone()
                };

                // Check if this is a built-in system class first
                if let Some(return_type) = self.generate_builtin_static_method_call(
                    &full_class_name,
                    method,
                    arguments,
                    instructions,
                )? {
                    return Ok(return_type);
                }

                // Generate arguments for user-defined static methods
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Create function name from class and method (use dot notation for stdlib functions)
                let function_name = format!("{class_name}.{method}");

                // Find the function index
                if let Some(method_index) = self.get_function_index(&function_name) {
                    instructions.push(Instruction::Call(method_index));
                    // Get the return type from the function name mapping
                    // This is more reliable than the function signature lookup
                    Ok(self.get_function_return_type_by_name(&function_name))
                } else {
                    Err(CompilerError::codegen_error(
                        format!("Static method '{method}' in class '{class_name}' not found"),
                        Some("Make sure the method is defined in the class".to_string()),
                        None,
                    ))
                }
            }
            Expression::OnError {
                expression,
                fallback,
                ..
            } => {
                // Handle onError expression: expression onError fallback
                self.generate_on_error(expression, fallback, instructions)
            }
            Expression::OnErrorBlock {
                expression,
                error_handler,
                ..
            } => {
                // Handle onError block: expression onError: block
                self.generate_error_handler(expression, error_handler, instructions)
            }
            Expression::ErrorVariable { .. } => {
                // Access the error variable in an error context
                if let Some(error_local) = self.variable_map.get("error") {
                    instructions.push(Instruction::LocalGet(error_local.index));
                    Ok(WasmType::I32) // Error object is represented as a pointer
                } else {
                    Err(CompilerError::codegen_error(
                        "Error variable accessed outside of error context",
                        Some("Error variable can only be used within onError blocks".to_string()),
                        None,
                    ))
                }
            }
            Expression::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                // Generate conditional expression: if condition then value else value
                // This generates a WebAssembly if-else block that returns a value

                // Generate the condition
                self.generate_expression(condition, instructions)?;

                // Start the if block
                let then_type = {
                    let mut then_instructions = Vec::new();
                    let result_type =
                        self.generate_expression(then_expr, &mut then_instructions)?;

                    // Convert to block type
                    let block_type = match &result_type {
                        WasmType::I32 => BlockType::Result(ValType::I32),
                        WasmType::I64 => BlockType::Result(ValType::I64),
                        WasmType::F32 => BlockType::Result(ValType::F32),
                        WasmType::F64 => BlockType::Result(ValType::F64),
                        _ => BlockType::Empty,
                    };

                    instructions.push(Instruction::If(block_type));
                    instructions.extend(then_instructions);

                    result_type
                };

                // Generate the else branch
                instructions.push(Instruction::Else);
                let else_type = self.generate_expression(else_expr, instructions)?;

                // End the if block
                instructions.push(Instruction::End);

                // Return the common type (should be compatible from semantic analysis)
                if then_type == else_type {
                    Ok(then_type)
                } else {
                    // Handle type promotion if needed
                    match (then_type, else_type) {
                        (WasmType::I32, WasmType::I64) | (WasmType::I64, WasmType::I32) => {
                            Ok(WasmType::I64)
                        }
                        (WasmType::F32, WasmType::F64) | (WasmType::F64, WasmType::F32) => {
                            Ok(WasmType::F64)
                        }
                        (WasmType::I32, WasmType::F32) | (WasmType::F32, WasmType::I32) => {
                            Ok(WasmType::F32)
                        }
                        (WasmType::I32, WasmType::F64) | (WasmType::F64, WasmType::I32) => {
                            Ok(WasmType::F64)
                        }
                        (WasmType::I64, WasmType::F32) | (WasmType::F32, WasmType::I64) => {
                            Ok(WasmType::F32)
                        }
                        (WasmType::I64, WasmType::F64) | (WasmType::F64, WasmType::I64) => {
                            Ok(WasmType::F64)
                        }
                        _ => Ok(then_type), // Default to then type
                    }
                }
            }
            Expression::BaseCall {
                arguments,
                location,
            } => {
                // Generate base constructor call
                self.generate_base_call(arguments, location, instructions)
            }

            // Async expressions
            Expression::StartExpression {
                expression: _,
                location: _,
            } => {
                // Generate proper async execution with future creation

                // Step 1: Create a unique future ID
                let future_id = format!("future_{}", self.function_count);
                let future_id_ptr = self.add_string_to_pool(&future_id);
                let future_id_len = future_id.len() as i32;

                // Step 2: Create the future in the runtime
                instructions.push(Instruction::I32Const(future_id_ptr as i32));
                instructions.push(Instruction::I32Const(future_id_len));
                let create_future_index = self.get_or_create_function_index("create_future");
                instructions.push(Instruction::Call(create_future_index));

                // Step 3: Store the future handle for later resolution
                let future_handle_local = self.add_local(WasmType::I32);
                instructions.push(Instruction::LocalSet(future_handle_local));

                // Step 4: Start background task to execute the expression
                let task_name = format!("start_expr_{}", self.function_count);
                let task_name_ptr = self.add_string_to_pool(&task_name);
                let task_name_len = task_name.len() as i32;

                instructions.push(Instruction::I32Const(task_name_ptr as i32));
                instructions.push(Instruction::I32Const(task_name_len));
                let start_task_index = self.get_or_create_function_index("start_background_task");
                instructions.push(Instruction::Call(start_task_index));

                // Step 5: Queue the expression for async execution (FIXED - no immediate execution!)
                // Instead of executing immediately, we queue the task for the host-side async runtime
                let task_id = self.function_count;
                let future_task_name = format!("future_task_{task_id}");
                let _future_task_ptr = self.add_string_to_pool(&future_task_name);
                let _future_task_len = future_task_name.len() as i32;

                // Create future task metadata
                let future_metadata = format!("{{\"id\":{task_id},\"name\":\"{future_task_name}\",\"type\":\"future\",\"priority\":\"normal\"}}");
                let future_metadata_ptr = self.add_string_to_pool(&future_metadata);
                let future_metadata_len = future_metadata.len() as i32;

                // Queue the future task for execution (not execute immediately)
                instructions.push(Instruction::I32Const(task_id as i32));
                instructions.push(Instruction::I32Const(future_metadata_ptr as i32));
                instructions.push(Instruction::I32Const(future_metadata_len));
                let queue_future_index = self.get_or_create_function_index("queue_future_task");
                instructions.push(Instruction::Call(queue_future_index));
                instructions.push(Instruction::Drop); // Drop the queue result

                // Step 6: Associate the future handle with the queued task
                // This creates a pending future that will be resolved when the task completes
                instructions.push(Instruction::LocalGet(future_handle_local)); // Future ID
                instructions.push(Instruction::I32Const(task_id as i32)); // Task ID
                let associate_future_index =
                    self.get_or_create_function_index("associate_future_task");
                instructions.push(Instruction::Call(associate_future_index));

                // Step 8: Return the future handle
                instructions.push(Instruction::LocalGet(future_handle_local));

                // Increment function counter for unique IDs
                self.function_count += 1;

                // Return the future type (represented as i32 handle)
                Ok(WasmType::I32)
            }

            Expression::Unary(op, expr) => self.generate_unary_operation(op, expr, instructions),

            // Postfix `!` (Required assertion).
            // foundation/spec/grammar.ebnf: `postfix_primary = primary , [ required_op ]`
            // Asserts the operand is not null; traps at runtime if it is.
            Expression::Postfix {
                operand,
                operator: PostfixOperator::Required,
                ..
            } => {
                let operand_type = self.generate_expression(operand, instructions)?;
                self.generate_required_assertion(operand_type, instructions)
            }

            // ChainedMethodCall — foundation/spec/grammar.ebnf `chained_method_call`.
            // Left-to-right chain: evaluate receiver, then apply each segment.
            Expression::ChainedMethodCall {
                receiver,
                chain,
                location,
            } => self.generate_chained_calls(receiver, chain, location, instructions),

            // MultipleMethodCall — foundation/spec/grammar.ebnf `multiple_method_call`.
            // Structurally identical to ChainedMethodCall; uses the same codegen.
            Expression::MultipleMethodCall {
                receiver,
                chain,
                location,
            } => self.generate_chained_calls(receiver, chain, location, instructions),

            // ThreeLevelMethodCall — `a.b.method(args)`.
            // foundation/spec/grammar.ebnf: `three_level_method_call`.
            Expression::ThreeLevelMethodCall {
                first,
                second,
                method,
                arguments,
                location,
            } => {
                let property_access = Expression::PropertyAccess {
                    object: Box::new(Expression::Variable(first.clone())),
                    property: second.clone(),
                    location: location.clone(),
                };
                let method_call = Expression::MethodCall {
                    object: Box::new(property_access),
                    method: method.clone(),
                    arguments: arguments.clone(),
                    location: location.clone(),
                };
                self.generate_expression(&method_call, instructions)
            }

            // PropertyMethodCall — `obj.path...method(args)`.
            // foundation/spec/grammar.ebnf: `property_method_call`.
            Expression::PropertyMethodCall {
                object,
                path,
                method,
                arguments,
                location,
            } => {
                let base = Expression::Variable(object.clone());
                let receiver = path
                    .iter()
                    .fold(base, |acc, seg| Expression::PropertyAccess {
                        object: Box::new(acc),
                        property: seg.clone(),
                        location: location.clone(),
                    });
                let method_call = Expression::MethodCall {
                    object: Box::new(receiver),
                    method: method.clone(),
                    arguments: arguments.clone(),
                    location: location.clone(),
                };
                self.generate_expression(&method_call, instructions)
            }

            Expression::PropertyAccess {
                object, property, ..
            } => {
                // Handle property access to objects
                if let Expression::Variable(_namespace) = object.as_ref() {
                    {
                        // Handle regular property access on objects
                        let object_type = self.generate_expression(object, instructions)?;
                        match object_type {
                            WasmType::I32 => {
                                // This is an object pointer - implement property access
                                // We need to look up the field offset and generate a memory load

                                // First, try to determine the object's class type
                                // For now, we'll look for the field in all available classes
                                // In a full implementation, we'd track object types more precisely

                                let mut field_found = false;
                                let mut field_type = Type::Any;
                                let mut field_offset = 0u32;

                                // Look through all classes to find the field
                                for (_class_name, field_map) in &self.class_field_map {
                                    if let Some((found_field_type, found_offset)) =
                                        field_map.get(property)
                                    {
                                        field_found = true;
                                        field_type = found_field_type.clone();
                                        field_offset = *found_offset;
                                        break;
                                    }
                                }

                                if !field_found {
                                    return Err(CompilerError::codegen_error(
                                        format!("Property '{}' not found in any class", property),
                                        Some("Check if the property name is correct".to_string()),
                                        None,
                                    ));
                                }

                                // Generate WASM instructions to load the field value
                                // object pointer is already on the stack from generate_expression(object)

                                // Add the field offset to the object pointer
                                if field_offset > 0 {
                                    instructions.push(Instruction::I32Const(field_offset as i32));
                                    instructions.push(Instruction::I32Add);
                                }

                                // Load the value based on field type
                                match field_type {
                                    Type::Integer => {
                                        instructions.push(Instruction::I32Load(MemArg {
                                            offset: 0,
                                            align: 2, // 4-byte alignment
                                            memory_index: 0,
                                        }));
                                        Ok(WasmType::I32)
                                    }
                                    Type::Number => {
                                        instructions.push(Instruction::F64Load(MemArg {
                                            offset: 0,
                                            align: 3, // 8-byte alignment
                                            memory_index: 0,
                                        }));
                                        Ok(WasmType::F64)
                                    }
                                    Type::String => {
                                        // Strings are stored as pointers to string objects
                                        instructions.push(Instruction::I32Load(MemArg {
                                            offset: 0,
                                            align: 2, // 4-byte alignment
                                            memory_index: 0,
                                        }));
                                        Ok(WasmType::I32)
                                    }
                                    Type::Boolean => {
                                        instructions.push(Instruction::I32Load(MemArg {
                                            offset: 0,
                                            align: 2, // 4-byte alignment
                                            memory_index: 0,
                                        }));
                                        Ok(WasmType::I32)
                                    }
                                    _ => {
                                        // For other types, treat as pointer
                                        instructions.push(Instruction::I32Load(MemArg {
                                            offset: 0,
                                            align: 2, // 4-byte alignment
                                            memory_index: 0,
                                        }));
                                        Ok(WasmType::I32)
                                    }
                                }
                            }
                            _ => Err(CompilerError::codegen_error(
                                format!("Property access on type {object_type:?} not supported"),
                                Some(
                                    "Property access is only supported on objects and lists"
                                        .to_string(),
                                ),
                                None,
                            )),
                        }
                    }
                } else {
                    Err(CompilerError::codegen_error(
                        "Complex property access not supported",
                        Some("Property access is only supported on simple variables".to_string()),
                        None,
                    ))
                }
            }
            Expression::NamespaceCall {
                namespace,
                function,
                arguments,
                location: _,
            } => {
                // Handle namespace function calls like string.startsWith(), math.sqrt(), etc.
                let mut full_function_name = format!("{}.{}", namespace, function);

                // Special handling for polymorphic math.abs - determine the correct function variant
                if full_function_name == "math.abs" && !arguments.is_empty() {
                    // Determine the argument type to select correct math.abs variant
                    let arg_type = match &arguments[0] {
                        Expression::Variable(name) => {
                            // Look up variable type in variable_types
                            if let Some(var_type) = self.variable_types.get(name) {
                                match var_type {
                                    Type::Integer => WasmType::I32,
                                    Type::Number => WasmType::F64,
                                    Type::IntegerSized { bits: 64, .. } => WasmType::I64,
                                    Type::IntegerSized { bits: 32, .. } => WasmType::I32,
                                    Type::NumberSized { bits: 64 } => WasmType::F64,
                                    Type::NumberSized { bits: 32 } => WasmType::F32,
                                    _ => WasmType::I32, // Default to I32 for other types
                                }
                            } else {
                                WasmType::I32 // Default fallback
                            }
                        }
                        Expression::Literal(Value::Integer(_)) => WasmType::I32,
                        Expression::Literal(Value::Number(_)) => WasmType::F64,
                        Expression::Literal(Value::Integer64(_)) => WasmType::I64,
                        _ => {
                            // For complex expressions, try to infer the type
                            match self.generate_expression(&arguments[0], &mut Vec::new()) {
                                Ok(wasm_type) => wasm_type,
                                Err(_) => WasmType::I32, // Default fallback
                            }
                        }
                    };

                    // Select the appropriate math.abs function based on argument type
                    full_function_name = match arg_type {
                        WasmType::I32 => "math.abs.i32".to_string(),
                        WasmType::F64 => "math.abs".to_string(),
                        WasmType::I64 => "math.abs".to_string(), // Use F64 version for I64
                        WasmType::F32 => "math.abs".to_string(), // Use F64 version for F32
                        WasmType::V128 | WasmType::Unit => "math.abs".to_string(), // Default to F64 version
                    };
                }

                let return_type = self.get_function_return_type_by_name(&full_function_name);

                // Generate arguments
                for arg in arguments {
                    self.generate_expression(arg, instructions)?;
                }

                // Find the function index
                if let Some(function_index) = self.get_function_index(&full_function_name) {
                    instructions.push(Instruction::Call(function_index));
                    Ok(return_type)
                } else {
                    Err(CompilerError::codegen_error(
                        format!("Namespace function '{}' not found", full_function_name),
                        Some(format!(
                            "Function '{}' may not be registered in the standard library",
                            full_function_name
                        )),
                        None,
                    ))
                }
            }
            _ => Err(CompilerError::codegen_error(
                "Unsupported expression type in codegen",
                None,
                loc.clone(),
            )),
        }
    }

    pub(crate) fn generate_expression_with_type_hint(
        &mut self,
        expr: &Expression,
        type_hint: Option<&Type>,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        match expr {
            Expression::Literal(value) => {
                match value {
                    Value::List(elements) => {
                        // Use type hint to determine array element type
                        let target_element_type = if let Some(hint) = type_hint {
                            match hint {
                                Type::List(element_type) => Some(element_type.as_ref()),
                                _ => None,
                            }
                        } else {
                            None
                        };

                        let ptr =
                            self.allocate_array_with_target_type(elements, target_element_type)?;
                        instructions.push(Instruction::I32Const(ptr as i32));
                        Ok(WasmType::I32)
                    }
                    _ => {
                        // For non-array literals, use the standard method
                        self.generate_expression(expr, instructions)
                    }
                }
            }
            _ => {
                // For non-literal expressions, use the standard method
                self.generate_expression(expr, instructions)
            }
        }
    }

    pub(crate) fn generate_binary_operation(
        &mut self,
        left: &Expression,
        op: &BinaryOperator,
        right: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // BOOK: null-coalescing - Handle default operator specially
        if let BinaryOperator::Default = op {
            return self.generate_default_operation(left, right, instructions);
        }

        // Special handling for string concatenation
        if let BinaryOperator::Add = op {
            if self.is_string_type(left) && self.is_string_type(right) {
                // Generate string pointers for both operands
                // string_concat expects 2 args: (string_ptr1, string_ptr2)
                // Each pointer points to [4-byte length][data]
                self.generate_expression(left, instructions)?;
                self.generate_expression(right, instructions)?;

                // Call string concatenation function
                if let Ok(concat_index) = self.get_string_concat_index() {
                    instructions.push(Instruction::Call(concat_index));
                    return Ok(WasmType::I32); // String pointer
                } else {
                    return Err(CompilerError::codegen_error(
                        "String concatenation function not found",
                        None,
                        None,
                    ));
                }
            }
        }

        let left_type = self.generate_expression(left, instructions)?;
        let right_type = self.generate_expression(right, instructions)?;

        if matches!(op, BinaryOperator::Equal | BinaryOperator::NotEqual) {
            debug!(
                op = ?op,
                left_type = ?left_type,
                right_type = ?right_type,
                "Binary operation types"
            );
        }

        // Special handling for division by zero
        if let BinaryOperator::Divide = op {
            match right {
                Expression::Literal(Value::Integer(0)) => {
                    return Err(CompilerError::division_by_zero_error(None));
                }
                Expression::Literal(Value::Number(n)) if *n == 0.0 => {
                    return Err(CompilerError::division_by_zero_error(None));
                }
                _ => {
                    // For non-literal divisors, add a runtime check
                    let temp_local_idx = self.add_local(right_type);
                    instructions.push(Instruction::LocalSet(temp_local_idx));
                    instructions.push(Instruction::LocalGet(temp_local_idx));

                    match right_type {
                        WasmType::I32 => {
                            instructions.push(Instruction::I32Eqz); // Check if zero
                            instructions.push(Instruction::If(BlockType::Empty));
                            instructions.push(Instruction::Unreachable);
                            instructions.push(Instruction::End);
                        }
                        WasmType::F64 => {
                            instructions.push(Instruction::F64Const(0.0));
                            instructions.push(Instruction::F64Eq); // Check if zero
                            instructions.push(Instruction::If(BlockType::Empty));
                            instructions.push(Instruction::Unreachable);
                            instructions.push(Instruction::End);
                        }
                        _ => {} // No check for other types
                    }
                    instructions.push(Instruction::LocalGet(temp_local_idx));
                }
            }
        }

        match (left_type, right_type) {
            (WasmType::I32, WasmType::I32) => {
                match op {
                    // Use correct AST variant names
                    ast::BinaryOperator::Add => { instructions.push(Instruction::I32Add); Ok(WasmType::I32) },
                    ast::BinaryOperator::Subtract => { instructions.push(Instruction::I32Sub); Ok(WasmType::I32) },
                    ast::BinaryOperator::Multiply => { instructions.push(Instruction::I32Mul); Ok(WasmType::I32) },
                    ast::BinaryOperator::Divide => { instructions.push(Instruction::I32DivS); Ok(WasmType::I32) },
                    ast::BinaryOperator::Equal => { instructions.push(Instruction::I32Eq); Ok(WasmType::I32) },
                    ast::BinaryOperator::NotEqual => { instructions.push(Instruction::I32Ne); Ok(WasmType::I32) },
                    ast::BinaryOperator::Less => { instructions.push(Instruction::I32LtS); Ok(WasmType::I32) },
                    ast::BinaryOperator::Greater => { instructions.push(Instruction::I32GtS); Ok(WasmType::I32) },
                    ast::BinaryOperator::LessEqual => { instructions.push(Instruction::I32LeS); Ok(WasmType::I32) },
                    ast::BinaryOperator::GreaterEqual => { instructions.push(Instruction::I32GeS); Ok(WasmType::I32) },
                    ast::BinaryOperator::Modulo => { instructions.push(Instruction::I32RemS); Ok(WasmType::I32) },
                    ast::BinaryOperator::Power => {
                        // For I32 ^ I32, we need to convert both operands to F64
                        // Stack currently has: [left_i32, right_i32]

                        // Store right operand temporarily
                        let temp_local = self.add_local(WasmType::I32);
                        instructions.push(Instruction::LocalSet(temp_local));

                        // Convert left operand to F64
                        instructions.push(Instruction::F64ConvertI32S);

                        // Get right operand and convert to F64
                        instructions.push(Instruction::LocalGet(temp_local));
                        instructions.push(Instruction::F64ConvertI32S);

                        // Call power function
                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error("Power function not found".to_string(), None, None))
                        }
                    },
                    ast::BinaryOperator::And => { instructions.push(Instruction::I32And); Ok(WasmType::I32) },
                    ast::BinaryOperator::Or => { instructions.push(Instruction::I32Or); Ok(WasmType::I32) },
                    ast::BinaryOperator::Is => { instructions.push(Instruction::I32Eq); Ok(WasmType::I32) },
                    ast::BinaryOperator::Not => { instructions.push(Instruction::I32Ne); Ok(WasmType::I32) },
                    // BOOK: null-coalescing - Default is handled before this match
                    ast::BinaryOperator::Default => unreachable!("Default handled in generate_binary_expression"),
                }
            },

            (WasmType::I64, WasmType::I64) => {
                match op {
                    ast::BinaryOperator::Add => { instructions.push(Instruction::I64Add); Ok(WasmType::I64) },
                    ast::BinaryOperator::Subtract => { instructions.push(Instruction::I64Sub); Ok(WasmType::I64) },
                    ast::BinaryOperator::Multiply => { instructions.push(Instruction::I64Mul); Ok(WasmType::I64) },
                    ast::BinaryOperator::Divide => { instructions.push(Instruction::I64DivS); Ok(WasmType::I64) },
                    ast::BinaryOperator::Equal => { instructions.push(Instruction::I64Eq); Ok(WasmType::I32) },
                    ast::BinaryOperator::NotEqual => { instructions.push(Instruction::I64Ne); Ok(WasmType::I32) },
                    ast::BinaryOperator::Less => { instructions.push(Instruction::I64LtS); Ok(WasmType::I32) },
                    ast::BinaryOperator::Greater => { instructions.push(Instruction::I64GtS); Ok(WasmType::I32) },
                    ast::BinaryOperator::LessEqual => { instructions.push(Instruction::I64LeS); Ok(WasmType::I32) },
                    ast::BinaryOperator::GreaterEqual => { instructions.push(Instruction::I64GeS); Ok(WasmType::I32) },
                    ast::BinaryOperator::Modulo => { instructions.push(Instruction::I64RemS); Ok(WasmType::I64) },
                    ast::BinaryOperator::Power => {
                        // For I64 ^ I64, convert both operands to F64 and use F64 power
                        // Store right operand temporarily
                        let temp_local = self.add_local(WasmType::I64);
                        instructions.push(Instruction::LocalSet(temp_local));

                        // Convert left operand to F64
                        instructions.push(Instruction::F64ConvertI64S);

                        // Get right operand and convert to F64
                        instructions.push(Instruction::LocalGet(temp_local));
                        instructions.push(Instruction::F64ConvertI64S);

                        // Call power function
                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            instructions.push(Instruction::I64TruncF64S);
                            Ok(WasmType::I64)
                        } else {
                            Err(CompilerError::type_error("Power function not found".to_string(), None, None))
                        }
                    },
                    ast::BinaryOperator::And => { instructions.push(Instruction::I64And); Ok(WasmType::I64) },
                    ast::BinaryOperator::Or => { instructions.push(Instruction::I64Or); Ok(WasmType::I64) },
                    ast::BinaryOperator::Is => { instructions.push(Instruction::I64Eq); Ok(WasmType::I32) },
                    ast::BinaryOperator::Not => { instructions.push(Instruction::I64Ne); Ok(WasmType::I32) },
                    // BOOK: null-coalescing - Default is handled before this match
                    ast::BinaryOperator::Default => unreachable!("Default handled in generate_binary_expression"),
                }
            },

            (WasmType::F64, WasmType::F64) => {
                match op {
                    // Use correct AST variant names
                    ast::BinaryOperator::Add => {
                        instructions.push(Instruction::F64Add);
                        Ok(WasmType::F64)
                    },
                    ast::BinaryOperator::Subtract => {
                        instructions.push(Instruction::F64Sub);
                        Ok(WasmType::F64)
                    },
                    ast::BinaryOperator::Multiply => {
                        instructions.push(Instruction::F64Mul);
                        Ok(WasmType::F64)
                    },
                    ast::BinaryOperator::Divide => {
                        instructions.push(Instruction::F64Div);
                        Ok(WasmType::F64)
                    },
                    ast::BinaryOperator::Modulo => {
                        if let Some(mod_index) = self.get_function_index("mod") {
                            instructions.push(Instruction::Call(mod_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error("Modulo function not found".to_string(), None, None))
                        }
                    },
                    ast::BinaryOperator::Power => {
                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error("Power function not found".to_string(), None, None))
                        }
                    },
                    ast::BinaryOperator::Equal => {
                        instructions.push(Instruction::F64Eq);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::NotEqual => {
                        instructions.push(Instruction::F64Ne);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Less => {
                        instructions.push(Instruction::F64Lt);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Greater => {
                        instructions.push(Instruction::F64Gt);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::LessEqual => {
                        instructions.push(Instruction::F64Le);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::GreaterEqual => {
                        instructions.push(Instruction::F64Ge);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::And => {
                        instructions.push(Instruction::I32TruncF64S);
                        instructions.push(Instruction::I32And);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Or => {
                        instructions.push(Instruction::I32TruncF64S);
                        instructions.push(Instruction::I32Or);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Is => {
                        instructions.push(Instruction::F64Eq);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Not => {
                        instructions.push(Instruction::F64Ne);
                        Ok(WasmType::I32)
                    },
                    // BOOK: null-coalescing - Default is handled before this match
                    ast::BinaryOperator::Default => unreachable!("Default handled in generate_binary_expression"),
                }
            },

            (WasmType::F32, WasmType::F32) => {
                match op {
                    ast::BinaryOperator::Add => {
                        instructions.push(Instruction::F32Add);
                        Ok(WasmType::F32)
                    },
                    ast::BinaryOperator::Subtract => {
                        instructions.push(Instruction::F32Sub);
                        Ok(WasmType::F32)
                    },
                    ast::BinaryOperator::Multiply => {
                        instructions.push(Instruction::F32Mul);
                        Ok(WasmType::F32)
                    },
                    ast::BinaryOperator::Divide => {
                        instructions.push(Instruction::F32Div);
                        Ok(WasmType::F32)
                    },
                    ast::BinaryOperator::Modulo => {
                        // F32 modulo requires conversion to F64
                        // Stack currently has: [F32_left, F32_right]
                        // Store right operand temporarily
                        let temp_f32_local = self.add_local(WasmType::F32);
                        instructions.push(Instruction::LocalSet(temp_f32_local));

                        // Convert left operand to F64
                        instructions.push(Instruction::F64PromoteF32);

                        // Get right operand and convert to F64
                        instructions.push(Instruction::LocalGet(temp_f32_local));
                        instructions.push(Instruction::F64PromoteF32);

                        if let Some(mod_index) = self.get_function_index("mod") {
                            instructions.push(Instruction::Call(mod_index));
                            instructions.push(Instruction::F32DemoteF64);
                            Ok(WasmType::F32)
                        } else {
                            Err(CompilerError::type_error("Modulo function not found".to_string(), None, None))
                        }
                    },
                    ast::BinaryOperator::Power => {
                        // F32 power requires conversion to F64
                        // Stack currently has: [F32_left, F32_right]
                        // Store right operand temporarily
                        let temp_f32_local = self.add_local(WasmType::F32);
                        instructions.push(Instruction::LocalSet(temp_f32_local));

                        // Convert left operand to F64
                        instructions.push(Instruction::F64PromoteF32);

                        // Get right operand and convert to F64
                        instructions.push(Instruction::LocalGet(temp_f32_local));
                        instructions.push(Instruction::F64PromoteF32);

                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            instructions.push(Instruction::F32DemoteF64);
                            Ok(WasmType::F32)
                        } else {
                            Err(CompilerError::type_error("Power function not found".to_string(), None, None))
                        }
                    },
                    ast::BinaryOperator::Equal => {
                        instructions.push(Instruction::F32Eq);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::NotEqual => {
                        instructions.push(Instruction::F32Ne);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Less => {
                        instructions.push(Instruction::F32Lt);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Greater => {
                        instructions.push(Instruction::F32Gt);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::LessEqual => {
                        instructions.push(Instruction::F32Le);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::GreaterEqual => {
                        instructions.push(Instruction::F32Ge);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::And => {
                        // Stack has [F32_left, F32_right]
                        // Store right operand
                        let temp_f32_local = self.add_local(WasmType::F32);
                        instructions.push(Instruction::LocalSet(temp_f32_local));

                        // Convert left to I32
                        instructions.push(Instruction::I32TruncF32S);

                        // Get right and convert to I32
                        instructions.push(Instruction::LocalGet(temp_f32_local));
                        instructions.push(Instruction::I32TruncF32S);

                        instructions.push(Instruction::I32And);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Or => {
                        // Stack has [F32_left, F32_right]
                        // Store right operand
                        let temp_f32_local = self.add_local(WasmType::F32);
                        instructions.push(Instruction::LocalSet(temp_f32_local));

                        // Convert left to I32
                        instructions.push(Instruction::I32TruncF32S);

                        // Get right and convert to I32
                        instructions.push(Instruction::LocalGet(temp_f32_local));
                        instructions.push(Instruction::I32TruncF32S);

                        instructions.push(Instruction::I32Or);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Is => {
                        instructions.push(Instruction::F32Eq);
                        Ok(WasmType::I32)
                    },
                    ast::BinaryOperator::Not => {
                        instructions.push(Instruction::F32Ne);
                        Ok(WasmType::I32)
                    },
                    // BOOK: null-coalescing - Default is handled before this match
                    ast::BinaryOperator::Default => unreachable!("Default handled in generate_binary_expression"),
                }
            },

            (WasmType::I32, WasmType::F64) => {
                // Convert I32 to F64 and perform F64 operation
                // Need to convert the I32 (left operand) to F64
                // Stack currently has: [I32_left, F64_right]
                // We need: [F64_left, F64_right]

                // Store the F64 right operand temporarily
                let temp_f64_local = self.add_local(WasmType::F64);
                instructions.push(Instruction::LocalSet(temp_f64_local));

                // Convert the I32 left operand to F64
                instructions.push(Instruction::F64ConvertI32S);

                // Restore the F64 right operand
                instructions.push(Instruction::LocalGet(temp_f64_local));

                match op {
                    ast::BinaryOperator::Add => { instructions.push(Instruction::F64Add); Ok(WasmType::F64) },
                    ast::BinaryOperator::Subtract => { instructions.push(Instruction::F64Sub); Ok(WasmType::F64) },
                    ast::BinaryOperator::Multiply => { instructions.push(Instruction::F64Mul); Ok(WasmType::F64) },
                    ast::BinaryOperator::Divide => { instructions.push(Instruction::F64Div); Ok(WasmType::F64) },
                    ast::BinaryOperator::Equal => { instructions.push(Instruction::F64Eq); Ok(WasmType::I32) },
                    ast::BinaryOperator::NotEqual => { instructions.push(Instruction::F64Ne); Ok(WasmType::I32) },
                    ast::BinaryOperator::Less => { instructions.push(Instruction::F64Lt); Ok(WasmType::I32) },
                    ast::BinaryOperator::Greater => { instructions.push(Instruction::F64Gt); Ok(WasmType::I32) },
                    ast::BinaryOperator::LessEqual => { instructions.push(Instruction::F64Le); Ok(WasmType::I32) },
                    ast::BinaryOperator::GreaterEqual => { instructions.push(Instruction::F64Ge); Ok(WasmType::I32) },
                    _ => Err(CompilerError::type_error(
                        format!("Unsupported mixed I32/F64 binary operator: {op:?}"), None, None
                    ))
                }
            },
            (WasmType::F64, WasmType::I32) => {
                // Convert I32 to F64 and perform F64 operation
                instructions.push(Instruction::F64ConvertI32S);
                match op {
                    ast::BinaryOperator::Add => { instructions.push(Instruction::F64Add); Ok(WasmType::F64) },
                    ast::BinaryOperator::Subtract => { instructions.push(Instruction::F64Sub); Ok(WasmType::F64) },
                    ast::BinaryOperator::Multiply => { instructions.push(Instruction::F64Mul); Ok(WasmType::F64) },
                    ast::BinaryOperator::Divide => { instructions.push(Instruction::F64Div); Ok(WasmType::F64) },
                    ast::BinaryOperator::Equal => { instructions.push(Instruction::F64Eq); Ok(WasmType::I32) },
                    ast::BinaryOperator::NotEqual => { instructions.push(Instruction::F64Ne); Ok(WasmType::I32) },
                    ast::BinaryOperator::Less => { instructions.push(Instruction::F64Lt); Ok(WasmType::I32) },
                    ast::BinaryOperator::Greater => { instructions.push(Instruction::F64Gt); Ok(WasmType::I32) },
                    ast::BinaryOperator::LessEqual => { instructions.push(Instruction::F64Le); Ok(WasmType::I32) },
                    ast::BinaryOperator::GreaterEqual => { instructions.push(Instruction::F64Ge); Ok(WasmType::I32) },
                    _ => Err(CompilerError::type_error(
                        format!("Unsupported mixed F64/I32 binary operator: {op:?}"), None, None
                    ))
                }
            },

            (WasmType::F32, WasmType::F64) => {
                // Convert F32 to F64 and perform F64 operation
                // Stack currently has: [F32_left, F64_right]
                // We need: [F64_left, F64_right]

                // Store the F64 right operand temporarily
                let temp_f64_local = self.add_local(WasmType::F64);
                instructions.push(Instruction::LocalSet(temp_f64_local));

                // Convert the F32 left operand to F64
                instructions.push(Instruction::F64PromoteF32);

                // Restore the F64 right operand
                instructions.push(Instruction::LocalGet(temp_f64_local));

                match op {
                    ast::BinaryOperator::Add => { instructions.push(Instruction::F64Add); Ok(WasmType::F64) },
                    ast::BinaryOperator::Subtract => { instructions.push(Instruction::F64Sub); Ok(WasmType::F64) },
                    ast::BinaryOperator::Multiply => { instructions.push(Instruction::F64Mul); Ok(WasmType::F64) },
                    ast::BinaryOperator::Divide => { instructions.push(Instruction::F64Div); Ok(WasmType::F64) },
                    ast::BinaryOperator::Equal => { instructions.push(Instruction::F64Eq); Ok(WasmType::I32) },
                    ast::BinaryOperator::NotEqual => { instructions.push(Instruction::F64Ne); Ok(WasmType::I32) },
                    ast::BinaryOperator::Less => { instructions.push(Instruction::F64Lt); Ok(WasmType::I32) },
                    ast::BinaryOperator::Greater => { instructions.push(Instruction::F64Gt); Ok(WasmType::I32) },
                    ast::BinaryOperator::LessEqual => { instructions.push(Instruction::F64Le); Ok(WasmType::I32) },
                    ast::BinaryOperator::GreaterEqual => { instructions.push(Instruction::F64Ge); Ok(WasmType::I32) },
                    ast::BinaryOperator::Modulo => {
                        if let Some(mod_index) = self.get_function_index("mod") {
                            instructions.push(Instruction::Call(mod_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error("Modulo function not found".to_string(), None, None))
                        }
                    },
                    ast::BinaryOperator::Power => {
                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error("Power function not found".to_string(), None, None))
                        }
                    },
                    _ => Err(CompilerError::type_error(
                        format!("Unsupported mixed F32/F64 binary operator: {op:?}"), None, None
                    ))
                }
            },
            (WasmType::F64, WasmType::F32) => {
                // Convert F32 to F64 and perform F64 operation
                // Stack currently has: [F64_left, F32_right]
                // We need: [F64_left, F64_right]

                // Convert the F32 right operand to F64
                instructions.push(Instruction::F64PromoteF32);

                match op {
                    ast::BinaryOperator::Add => { instructions.push(Instruction::F64Add); Ok(WasmType::F64) },
                    ast::BinaryOperator::Subtract => { instructions.push(Instruction::F64Sub); Ok(WasmType::F64) },
                    ast::BinaryOperator::Multiply => { instructions.push(Instruction::F64Mul); Ok(WasmType::F64) },
                    ast::BinaryOperator::Divide => { instructions.push(Instruction::F64Div); Ok(WasmType::F64) },
                    ast::BinaryOperator::Equal => { instructions.push(Instruction::F64Eq); Ok(WasmType::I32) },
                    ast::BinaryOperator::NotEqual => { instructions.push(Instruction::F64Ne); Ok(WasmType::I32) },
                    ast::BinaryOperator::Less => { instructions.push(Instruction::F64Lt); Ok(WasmType::I32) },
                    ast::BinaryOperator::Greater => { instructions.push(Instruction::F64Gt); Ok(WasmType::I32) },
                    ast::BinaryOperator::LessEqual => { instructions.push(Instruction::F64Le); Ok(WasmType::I32) },
                    ast::BinaryOperator::GreaterEqual => { instructions.push(Instruction::F64Ge); Ok(WasmType::I32) },
                    ast::BinaryOperator::Modulo => {
                        if let Some(mod_index) = self.get_function_index("mod") {
                            instructions.push(Instruction::Call(mod_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error("Modulo function not found".to_string(), None, None))
                        }
                    },
                    ast::BinaryOperator::Power => {
                        if let Some(pow_index) = self.get_function_index("pow") {
                            instructions.push(Instruction::Call(pow_index));
                            Ok(WasmType::F64)
                        } else {
                            Err(CompilerError::type_error("Power function not found".to_string(), None, None))
                        }
                    },
                    _ => Err(CompilerError::type_error(
                        format!("Unsupported mixed F64/F32 binary operator: {op:?}"), None, None
                    ))
                }
            },

            _ => {
                Err(CompilerError::detailed_type_error(
                    format!("Type mismatch: Cannot apply {op:?} to incompatible types"),
                    left_type,
                    right_type,
                    None,
                    Some(format!("The operator {op:?} cannot be applied to types {left_type:?} and {right_type:?}. Consider using type conversion."))
                ))
            }
        }
    }

    // BOOK: null-coalescing - Generate code for default operator (null coalescing)
    // Semantics: `a default b` returns a if a is not null (not 0), otherwise returns b
    pub(crate) fn generate_default_operation(
        &mut self,
        left: &Expression,
        right: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate left expression and store in temp local
        let left_type = self.generate_expression(left, instructions)?;
        let left_local = self.add_local(left_type);
        instructions.push(Instruction::LocalSet(left_local));

        // Generate right expression (fallback value)
        let _right_type = self.generate_expression(right, instructions)?;

        // Now stack has: [right_value]
        // We need to push: left_value, condition, then select
        // select(val1, val2, cond) returns val1 if cond != 0, else val2
        // Stack order for select: [val2, val1, cond]
        // We want: return left if (left != 0), else return right
        // So: val1 = left, val2 = right, cond = (left != 0)

        // Stack currently: [right_value]
        // Push left_value (this is val1)
        instructions.push(Instruction::LocalGet(left_local));

        // Push condition (left != 0)
        instructions.push(Instruction::LocalGet(left_local));
        instructions.push(Instruction::I32Const(0));
        instructions.push(Instruction::I32Ne);

        // Stack now: [right_value, left_value, condition]
        // select will return left_value if condition != 0, else right_value
        instructions.push(Instruction::Select);

        // Return type is the type of the values (they should match)
        // For now, we return the left type; type checking should ensure compatibility
        Ok(left_type)
    }

    pub(crate) fn is_string_type(&self, expr: &Expression) -> bool {
        match expr {
            // String literals
            Expression::Literal(Value::String(_)) => true,
            // String interpolations
            Expression::StringInterpolation(_) => true,
            // Variables - look up their type
            Expression::Variable(name) => {
                if let Some(var_type) = self.variable_types.get(name) {
                    matches!(var_type, ast::Type::String)
                } else {
                    false
                }
            }
            // Method calls that return strings
            Expression::MethodCall { object, method, .. } => {
                // Common string methods
                let string_returning_methods = [
                    "toString",
                    "trim",
                    "trimStart",
                    "trimEnd",
                    "toLowerCase",
                    "toUpperCase",
                    "substring",
                    "replace",
                    "replaceAll",
                    "charAt",
                    "padStart",
                    "padEnd",
                    "concat",
                ];
                if string_returning_methods.contains(&method.as_str()) {
                    return true;
                }
                // If calling a method on a string variable
                if let Expression::Variable(obj_name) = object.as_ref() {
                    if let Some(var_type) = self.variable_types.get(obj_name) {
                        if matches!(var_type, ast::Type::String) {
                            return true;
                        }
                    }
                }
                false
            }
            // Binary operations that produce strings
            Expression::Binary(left, op, right) => {
                if matches!(op, ast::BinaryOperator::Add) {
                    self.is_string_type(left) || self.is_string_type(right)
                } else {
                    false
                }
            }
            // Function calls that return strings
            Expression::Call(name, _) => {
                let string_returning_fns = [
                    "int_to_str",
                    "number_to_str",
                    "bool_to_str",
                    "integer.toString",
                    "number.toString",
                    "boolean.toString",
                ];
                string_returning_fns.contains(&name.as_str())
            }
            _ => false,
        }
    }

    pub(crate) fn generate_unary_operation(
        &mut self,
        op: &UnaryOperator,
        expr: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Generate the operand first
        let operand_type = self.generate_expression(expr, instructions)?;

        match op {
            UnaryOperator::Negate => {
                match operand_type {
                    WasmType::I32 => {
                        // Negate integer: 0 - x
                        instructions.insert(instructions.len() - 1, Instruction::I32Const(0));
                        instructions.push(Instruction::I32Sub);
                        Ok(WasmType::I32)
                    }
                    WasmType::F64 => {
                        // Negate float: -x
                        instructions.push(Instruction::F64Neg);
                        Ok(WasmType::F64)
                    }
                    _ => Err(CompilerError::type_error(
                        format!("Cannot negate type {operand_type:?}"),
                        None,
                        None,
                    )),
                }
            }
            UnaryOperator::Not => {
                match operand_type {
                    WasmType::I32 => {
                        // Logical NOT: x == 0
                        instructions.push(Instruction::I32Eqz);
                        Ok(WasmType::I32)
                    }
                    _ => Err(CompilerError::type_error(
                        format!("Cannot apply logical NOT to type {operand_type:?}"),
                        None,
                        None,
                    )),
                }
            }
        }
    }

    /// Emit WASM instructions that assert the top-of-stack value is not null
    /// and trap if it is.  Called by both the legacy `UnaryOperator`-based path
    /// and the new `Expression::Postfix { Required }` path.
    ///
    /// foundation/spec/grammar.ebnf: `required_op = "!"`
    pub(crate) fn generate_required_assertion(
        &mut self,
        operand_type: WasmType,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        match operand_type {
            WasmType::I32 => {
                let temp_local = self.add_local(WasmType::I32);
                instructions.push(Instruction::LocalSet(temp_local));
                instructions.push(Instruction::LocalGet(temp_local));
                instructions.push(Instruction::I32Eqz);
                instructions.push(Instruction::If(wasm_encoder::BlockType::Empty));
                instructions.push(Instruction::Unreachable);
                instructions.push(Instruction::End);
                instructions.push(Instruction::LocalGet(temp_local));
                Ok(WasmType::I32)
            }
            WasmType::F64 => {
                let temp_local = self.add_local(WasmType::F64);
                instructions.push(Instruction::LocalSet(temp_local));
                instructions.push(Instruction::LocalGet(temp_local));
                instructions.push(Instruction::F64Const(0.0));
                instructions.push(Instruction::F64Eq);
                instructions.push(Instruction::If(wasm_encoder::BlockType::Empty));
                instructions.push(Instruction::Unreachable);
                instructions.push(Instruction::End);
                instructions.push(Instruction::LocalGet(temp_local));
                Ok(WasmType::F64)
            }
            _ => {
                // Non-nullable types pass through unchanged.
                Ok(operand_type)
            }
        }
    }

    /// Emit WASM for a chained method call: evaluate the receiver then apply
    /// each `(method, args)` segment in left-to-right order.
    ///
    /// Used by `Expression::ChainedMethodCall` and `Expression::MultipleMethodCall`.
    fn generate_chained_calls(
        &mut self,
        receiver: &Expression,
        chain: &[(String, Vec<Expression>)],
        location: &crate::ast::SourceLocation,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        // Build an Expression tree that the existing MethodCall codegen can handle.
        // Each segment wraps the previous result as the object.
        let mut current_expr: Expression = *Box::new(receiver.clone());
        for (method, args) in chain {
            current_expr = Expression::MethodCall {
                object: Box::new(current_expr),
                method: method.clone(),
                arguments: args.clone(),
                location: location.clone(),
            };
        }
        self.generate_expression(&current_expr, instructions)
    }

    pub(crate) fn generate_conversion(
        &self,
        from: WasmType,
        to: WasmType,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        match (from, to) {
            // Integer conversions
            (WasmType::I32, WasmType::I64) => {
                instructions.push(Instruction::I64ExtendI32S);
                Ok(())
            }
            (WasmType::I64, WasmType::I32) => {
                instructions.push(Instruction::I32WrapI64);
                Ok(())
            }
            // Float conversions
            (WasmType::F32, WasmType::F64) => {
                instructions.push(Instruction::F64PromoteF32);
                Ok(())
            }
            (WasmType::F64, WasmType::F32) => {
                instructions.push(Instruction::F32DemoteF64);
                Ok(())
            }
            // Integer to float conversions
            (WasmType::I32, WasmType::F32) => {
                instructions.push(Instruction::F32ConvertI32S);
                Ok(())
            }
            (WasmType::I32, WasmType::F64) => {
                instructions.push(Instruction::F64ConvertI32S);
                Ok(())
            }
            (WasmType::I64, WasmType::F32) => {
                instructions.push(Instruction::F32ConvertI64S);
                Ok(())
            }
            (WasmType::I64, WasmType::F64) => {
                instructions.push(Instruction::F64ConvertI64S);
                Ok(())
            }
            // Float to integer conversions
            (WasmType::F32, WasmType::I32) => {
                instructions.push(Instruction::I32TruncF32S);
                Ok(())
            }
            (WasmType::F64, WasmType::I32) => {
                instructions.push(Instruction::I32TruncF64S);
                Ok(())
            }
            (WasmType::F32, WasmType::I64) => {
                instructions.push(Instruction::I64TruncF32S);
                Ok(())
            }
            (WasmType::F64, WasmType::I64) => {
                instructions.push(Instruction::I64TruncF64S);
                Ok(())
            }
            // No conversion needed
            (t1, t2) if t1 == t2 => Ok(()),
            // Unsupported conversion
            _ => Err(CompilerError::codegen_error(
                format!("Cannot convert from {from:?} to {to:?}"),
                None,
                None,
            )),
        }
    }

    pub(crate) fn get_string_concat_index(&self) -> Result<u32, CompilerError> {
        self.get_function_index_or_error("string.concat")
    }

    /// Expand a string expression to (content_ptr, length) format for string.concat
    /// String memory layout: [4-byte length][content]
    /// This pushes (content_ptr, length) to the stack
    pub fn expand_string_expression(
        &mut self,
        expr: &Expression,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Generate the expression (pushes string pointer to stack)
        self.generate_expression(expr, instructions)?;

        // Store pointer in temp local
        let temp_local = self.add_local(WasmType::I32);
        instructions.push(Instruction::LocalSet(temp_local));

        // Push content pointer (ptr + 4, skip length prefix)
        instructions.push(Instruction::LocalGet(temp_local));
        instructions.push(Instruction::I32Const(4));
        instructions.push(Instruction::I32Add);

        // Push length (load i32 from ptr)
        instructions.push(Instruction::LocalGet(temp_local));
        instructions.push(Instruction::I32Load(wasm_encoder::MemArg {
            offset: 0,
            align: 2, // 4-byte alignment
            memory_index: 0,
        }));

        Ok(())
    }

    pub(crate) fn generate_value(
        &mut self,
        value: &Value,
        instructions: &mut Vec<Instruction>,
    ) -> Result<WasmType, CompilerError> {
        match value {
            Value::Number(n) => {
                instructions.push(Instruction::F64Const(*n));
                Ok(WasmType::F64)
            }
            Value::Integer(i) => {
                // Handle large integers that don't fit in i32
                if *i >= i32::MIN as i64 && *i <= i32::MAX as i64 {
                    instructions.push(Instruction::I32Const(*i as i32));
                    Ok(WasmType::I32)
                } else {
                    // Use i64 for large integers
                    instructions.push(Instruction::I64Const(*i));
                    Ok(WasmType::I64)
                }
            }
            Value::String(s) => {
                let ptr = self.allocate_string(s)?;
                instructions.push(Instruction::I32Const(ptr as i32));
                Ok(WasmType::I32)
            }
            Value::Boolean(b) => {
                instructions.push(Instruction::I32Const(if *b { 1 } else { 0 }));
                Ok(WasmType::I32)
            }
            Value::List(elements) => {
                let ptr = self.allocate_array_with_target_type(elements, None)?;
                instructions.push(Instruction::I32Const(ptr as i32));
                Ok(WasmType::I32)
            }
            Value::Matrix(rows) => {
                // Convert the matrix Values to f64 for WASM memory layout.
                // Each element is coerced: Number/Integer → f64, others → 0.0.
                let mut matrix_data: Vec<f64> = Vec::new();
                for row in rows {
                    for val in row {
                        let f = match val {
                            Value::Number(f) => *f,
                            Value::Integer(i) => *i as f64,
                            Value::Number32(f) => *f as f64,
                            Value::Number64(f) => *f,
                            Value::Integer8(i) => *i as f64,
                            Value::Integer8u(u) => *u as f64,
                            Value::Integer16(i) => *i as f64,
                            Value::Integer16u(u) => *u as f64,
                            Value::Integer32(i) => *i as f64,
                            Value::Integer64(i) => *i as f64,
                            _ => 0.0,
                        };
                        matrix_data.push(f);
                    }
                }

                let num_rows = rows.len();
                let num_cols = rows.first().map(|r| r.len()).unwrap_or(0);
                let ptr = self.allocate_matrix(&matrix_data, num_rows, num_cols)?;
                instructions.push(Instruction::I32Const(ptr as i32));
                Ok(WasmType::I32)
            }
            _ => Err(CompilerError::type_error(
                format!("Unsupported literal value: {value:?}"),
                Some("Use supported literal types".to_string()),
                None,
            )),
        }
    }
}
