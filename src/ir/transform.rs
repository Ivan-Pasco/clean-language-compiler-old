//! IR transformation passes

use crate::ast;
use crate::ir::*;
use std::collections::HashMap;

/// IR ID generator for unique node identification
struct IRIdGenerator {
    next_id: usize,
}

impl IRIdGenerator {
    fn new() -> Self {
        Self { next_id: 0 }
    }

    fn next(&mut self) -> IRId {
        let id = IRId::new(self.next_id);
        self.next_id += 1;
        id
    }
}

/// Transform AST to HIR
pub fn ast_to_hir(program: crate::ast::Program) -> IRResult<HIRProgram> {
    let mut transformer = ASTToHIRTransformer::new();
    transformer.transform_program(program)
}

struct ASTToHIRTransformer {
    id_gen: IRIdGenerator,
}

impl ASTToHIRTransformer {
    fn new() -> Self {
        Self {
            id_gen: IRIdGenerator::new(),
        }
    }

    fn transform_program(&mut self, program: ast::Program) -> IRResult<HIRProgram> {
        let mut declarations = Vec::new();

        // Transform classes first (needed for method resolution)
        for class in program.classes {
            let hir_class = self.transform_class(class)?;
            declarations.push(HIRDeclaration::Class(hir_class));
        }

        // Transform global functions
        for function in program.functions {
            let hir_function = self.transform_function(function)?;
            declarations.push(HIRDeclaration::Function(hir_function));
        }

        // Transform start function if present
        if let Some(start_fn) = program.start_function {
            let hir_function = self.transform_function(start_fn)?;
            declarations.push(HIRDeclaration::Function(hir_function));
        }

        Ok(HIRProgram {
            declarations,
            debug_info: DebugInfo {
                source_span: None,
                original_name: Some("program".to_string()),
                ir_level: IRLevel::HIR,
            },
        })
    }

    fn transform_class(&mut self, class: ast::Class) -> IRResult<HIRClass> {
        let mut methods = Vec::new();
        let mut fields = Vec::new();

        // Transform fields
        for field in class.fields {
            fields.push(HIRField {
                name: field.name,
                field_type: self.transform_type(field.type_)?,
                visibility: match field.visibility {
                    ast::Visibility::Public => hir::Visibility::Public,
                    ast::Visibility::Private => hir::Visibility::Private,
                },
            });
        }

        // Transform methods
        for method in class.methods {
            let hir_method = self.transform_function(method)?;
            methods.push(hir_method);
        }

        Ok(HIRClass {
            id: self.id_gen.next(),
            name: class.name.clone(),
            parent: class.base_class,
            fields,
            methods,
            debug_info: DebugInfo {
                source_span: None,
                original_name: Some(class.name),
                ir_level: IRLevel::HIR,
            },
        })
    }

    fn transform_function(&mut self, function: ast::Function) -> IRResult<HIRFunction> {
        let mut parameters = Vec::new();
        for param in function.parameters {
            parameters.push(HIRParameter {
                name: param.name,
                param_type: self.transform_type(param.type_)?,
                default_value: match param.default_value {
                    Some(expr) => Some(self.transform_expression(expr)?),
                    None => None,
                },
            });
        }

        let mut body = Vec::new();
        for stmt in function.body {
            let hir_stmt = self.transform_statement(stmt)?;
            body.push(hir_stmt);
        }

        Ok(HIRFunction {
            id: self.id_gen.next(),
            name: function.name.clone(),
            parameters,
            return_type: self.transform_type(function.return_type)?,
            body,
            is_async: matches!(function.modifier, ast::FunctionModifier::Background),
            debug_info: DebugInfo {
                source_span: None,
                original_name: Some(function.name),
                ir_level: IRLevel::HIR,
            },
        })
    }

    fn transform_statement(&mut self, stmt: ast::Statement) -> IRResult<HIRStatement> {
        match stmt {
            ast::Statement::Expression { expr, .. } => {
                Ok(HIRStatement::Expression(self.transform_expression(expr)?))
            }
            ast::Statement::Assignment { target, value, .. } => {
                Ok(HIRStatement::Assignment(HIRAssignment {
                    target: HIRLValue::Variable(target),
                    value: self.transform_expression(value)?,
                }))
            }
            ast::Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let mut then_stmts = Vec::new();
                for stmt in then_branch {
                    then_stmts.push(self.transform_statement(stmt)?);
                }

                let else_stmts = match else_branch {
                    Some(else_stmts_ast) => {
                        let mut else_stmts_hir = Vec::new();
                        for stmt in else_stmts_ast {
                            else_stmts_hir.push(self.transform_statement(stmt)?);
                        }
                        Some(else_stmts_hir)
                    }
                    None => None,
                };

                Ok(HIRStatement::If(HIRIf {
                    condition: self.transform_expression(condition)?,
                    then_branch: then_stmts,
                    else_branch: else_stmts,
                }))
            }
            ast::Statement::While {
                condition, body, ..
            } => {
                let mut body_stmts = Vec::new();
                for stmt in body {
                    body_stmts.push(self.transform_statement(stmt)?);
                }

                Ok(HIRStatement::While(HIRWhile {
                    condition: self.transform_expression(condition)?,
                    body: body_stmts,
                }))
            }
            ast::Statement::Iterate {
                iterator,
                collection,
                body,
                ..
            } => {
                let mut body_stmts = Vec::new();
                for stmt in body {
                    body_stmts.push(self.transform_statement(stmt)?);
                }

                Ok(HIRStatement::For(HIRFor {
                    variable: iterator,
                    iterable: self.transform_expression(collection)?,
                    body: body_stmts,
                }))
            }
            ast::Statement::Return { value, .. } => {
                let return_value = match value {
                    Some(expr) => Some(self.transform_expression(expr)?),
                    None => None,
                };
                Ok(HIRStatement::Return(return_value))
            }
            ast::Statement::VariableDecl {
                name,
                type_,
                initializer,
                ..
            } => {
                // Variable declarations are transformed into assignments
                // The variable itself is handled by scope analysis
                match initializer {
                    Some(init_expr) => Ok(HIRStatement::Assignment(HIRAssignment {
                        target: HIRLValue::Variable(name),
                        value: self.transform_expression(init_expr)?,
                    })),
                    None => {
                        // Uninitialized variable - create default value
                        let default_value = self.create_default_value(&type_)?;
                        Ok(HIRStatement::Assignment(HIRAssignment {
                            target: HIRLValue::Variable(name),
                            value: default_value,
                        }))
                    }
                }
            }
            // Handle other statement types
            _ => {
                // For now, convert unsupported statements to expression statements
                // TODO: Add proper handling for all statement types
                Ok(HIRStatement::Expression(HIRExpression::Literal(
                    HIRLiteral::Boolean(true),
                )))
            }
        }
    }

    fn transform_expression(&mut self, expr: ast::Expression) -> IRResult<HIRExpression> {
        match expr {
            ast::Expression::Literal(value) => {
                Ok(HIRExpression::Literal(self.transform_literal(value)?))
            }
            ast::Expression::Variable(name) => Ok(HIRExpression::Variable(name)),
            ast::Expression::Binary(left, op, right) => Ok(HIRExpression::Binary(HIRBinary {
                left: Box::new(self.transform_expression(*left)?),
                operator: self.transform_binary_op(op),
                right: Box::new(self.transform_expression(*right)?),
            })),
            ast::Expression::Unary(op, operand) => Ok(HIRExpression::Unary(HIRUnary {
                operator: self.transform_unary_op(op),
                operand: Box::new(self.transform_expression(*operand)?),
            })),
            ast::Expression::Call(function_name, args) => {
                let mut hir_args = Vec::new();
                for arg in args {
                    hir_args.push(self.transform_expression(arg)?);
                }
                Ok(HIRExpression::Call(HIRCall {
                    function: Box::new(HIRExpression::Variable(function_name)),
                    arguments: hir_args,
                }))
            }
            ast::Expression::PropertyAccess {
                object, property, ..
            } => Ok(HIRExpression::Member(HIRMember {
                object: Box::new(self.transform_expression(*object)?),
                member: property,
            })),
            ast::Expression::MethodCall {
                object,
                method,
                arguments,
                ..
            } => {
                let mut hir_args = Vec::new();
                hir_args.push(self.transform_expression(*object)?); // 'this' parameter
                for arg in arguments {
                    hir_args.push(self.transform_expression(arg)?);
                }
                Ok(HIRExpression::Call(HIRCall {
                    function: Box::new(HIRExpression::Variable(method)),
                    arguments: hir_args,
                }))
            }
            ast::Expression::ListAccess(list, index) => Ok(HIRExpression::Index(HIRIndex {
                object: Box::new(self.transform_expression(*list)?),
                index: Box::new(self.transform_expression(*index)?),
            })),
            // Handle other expression types
            _ => {
                // For now, return a placeholder for unsupported expressions
                // TODO: Add proper handling for all expression types
                Ok(HIRExpression::Literal(HIRLiteral::Boolean(true)))
            }
        }
    }

    fn transform_literal(&self, value: ast::Value) -> IRResult<HIRLiteral> {
        match value {
            ast::Value::Integer(i) => Ok(HIRLiteral::Integer(i)),
            ast::Value::Number(f) => Ok(HIRLiteral::Number(f)),
            ast::Value::String(s) => Ok(HIRLiteral::String(s)),
            ast::Value::Boolean(b) => Ok(HIRLiteral::Boolean(b)),
            ast::Value::List(items) => {
                let mut hir_items = Vec::new();
                for item in items {
                    hir_items.push(HIRExpression::Literal(self.transform_literal(item)?));
                }
                Ok(HIRLiteral::List(hir_items))
            }
            ast::Value::Matrix(matrix) => {
                let mut hir_matrix = Vec::new();
                for row in matrix {
                    let mut hir_row = Vec::new();
                    for item in row {
                        hir_row.push(HIRExpression::Literal(HIRLiteral::Number(item)));
                    }
                    hir_matrix.push(hir_row);
                }
                Ok(HIRLiteral::Matrix(hir_matrix))
            }
            _ => Ok(HIRLiteral::Boolean(true)), // Default for unsupported types
        }
    }

    fn transform_type(&self, ast_type: ast::Type) -> IRResult<HIRType> {
        match ast_type {
            ast::Type::Integer => Ok(HIRType::Integer(None)),
            ast::Type::Number => Ok(HIRType::Number(None)),
            ast::Type::String => Ok(HIRType::String),
            ast::Type::Boolean => Ok(HIRType::Boolean),
            ast::Type::Void => Ok(HIRType::Any), // Void maps to Any in HIR
            ast::Type::List(inner) => Ok(HIRType::List(Box::new(self.transform_type(*inner)?))),
            ast::Type::Matrix(inner) => Ok(HIRType::Matrix(Box::new(self.transform_type(*inner)?))),
            ast::Type::Object(name) => Ok(HIRType::Class(name)),
            ast::Type::Class { name, .. } => Ok(HIRType::Class(name)),
            ast::Type::Function(params, ret) => {
                let mut param_types = Vec::new();
                for param in params {
                    param_types.push(self.transform_type(param)?);
                }
                Ok(HIRType::Function(
                    param_types,
                    Box::new(self.transform_type(*ret)?),
                ))
            }
            ast::Type::IntegerSized { bits, .. } => Ok(HIRType::Integer(Some(bits))),
            ast::Type::NumberSized { bits } => Ok(HIRType::Number(Some(bits))),
            ast::Type::Any => Ok(HIRType::Any),
            _ => Ok(HIRType::Any), // Default for unsupported types
        }
    }

    fn transform_binary_op(&self, op: ast::BinaryOperator) -> hir::BinaryOperator {
        match op {
            ast::BinaryOperator::Add => hir::BinaryOperator::Add,
            ast::BinaryOperator::Subtract => hir::BinaryOperator::Sub,
            ast::BinaryOperator::Multiply => hir::BinaryOperator::Mul,
            ast::BinaryOperator::Divide => hir::BinaryOperator::Div,
            ast::BinaryOperator::Modulo => hir::BinaryOperator::Mod,
            ast::BinaryOperator::Power => hir::BinaryOperator::Pow,
            ast::BinaryOperator::Equal => hir::BinaryOperator::Equal,
            ast::BinaryOperator::NotEqual => hir::BinaryOperator::NotEqual,
            ast::BinaryOperator::Less => hir::BinaryOperator::Less,
            ast::BinaryOperator::Greater => hir::BinaryOperator::Greater,
            ast::BinaryOperator::LessEqual => hir::BinaryOperator::LessEqual,
            ast::BinaryOperator::GreaterEqual => hir::BinaryOperator::GreaterEqual,
            ast::BinaryOperator::And => hir::BinaryOperator::And,
            ast::BinaryOperator::Or => hir::BinaryOperator::Or,
            _ => hir::BinaryOperator::Add, // Default
        }
    }

    fn transform_unary_op(&self, op: ast::UnaryOperator) -> hir::UnaryOperator {
        match op {
            ast::UnaryOperator::Negate => hir::UnaryOperator::Minus,
            ast::UnaryOperator::Not => hir::UnaryOperator::Not,
        }
    }

    fn create_default_value(&self, type_: &ast::Type) -> IRResult<HIRExpression> {
        let default_literal = match type_ {
            ast::Type::Integer | ast::Type::IntegerSized { .. } => HIRLiteral::Integer(0),
            ast::Type::Number | ast::Type::NumberSized { .. } => HIRLiteral::Number(0.0),
            ast::Type::String => HIRLiteral::String(String::new()),
            ast::Type::Boolean => HIRLiteral::Boolean(false),
            ast::Type::List(_) => HIRLiteral::List(Vec::new()),
            _ => HIRLiteral::Boolean(false), // Default for other types
        };
        Ok(HIRExpression::Literal(default_literal))
    }
}

/// Transform HIR to MIR
pub fn hir_to_mir(program: HIRProgram) -> IRResult<MIRProgram> {
    let mut transformer = HIRToMIRTransformer::new();
    transformer.transform_program(program)
}

struct HIRToMIRTransformer {
    local_counter: usize,
    block_counter: usize,
}

impl HIRToMIRTransformer {
    fn new() -> Self {
        Self {
            local_counter: 0,
            block_counter: 0,
        }
    }

    fn next_local(&mut self) -> LocalId {
        let id = self.local_counter;
        self.local_counter += 1;
        id
    }

    fn next_block(&mut self) -> BlockId {
        let id = self.block_counter;
        self.block_counter += 1;
        id
    }

    fn transform_program(&mut self, program: HIRProgram) -> IRResult<MIRProgram> {
        let mut functions = HashMap::new();
        let mut classes = HashMap::new();
        let mut globals = Vec::new();

        for declaration in program.declarations {
            match declaration {
                HIRDeclaration::Function(hir_func) => {
                    let mir_func = self.transform_function(hir_func)?;
                    functions.insert(mir_func.name.clone(), mir_func);
                }
                HIRDeclaration::Class(hir_class) => {
                    let mir_class = self.transform_class(hir_class)?;
                    classes.insert(mir_class.name.clone(), mir_class);
                }
                HIRDeclaration::Variable(hir_var) => {
                    let mir_global = MIRGlobal {
                        name: hir_var.name,
                        global_type: self.transform_hir_type_to_mir(hir_var.var_type)?,
                        initializer: None, // TODO: Handle initializers
                    };
                    globals.push(mir_global);
                }
            }
        }

        Ok(MIRProgram {
            functions,
            classes,
            globals,
        })
    }

    fn transform_function(&mut self, function: HIRFunction) -> IRResult<MIRFunction> {
        // Reset counters for each function
        self.local_counter = 0;
        self.block_counter = 0;

        let mut parameters = Vec::new();
        for param in function.parameters {
            parameters.push(MIRLocal {
                id: self.next_local(),
                name: param.name,
                local_type: self.transform_hir_type_to_mir(param.param_type)?,
            });
        }

        let return_type = self.transform_hir_type_to_mir(function.return_type)?;
        let entry_block = self.next_block();

        // Transform function body to basic blocks
        let (basic_blocks, mut locals) =
            self.transform_statements_to_blocks(function.body, entry_block)?;

        // Add parameters to locals
        for param in &parameters {
            locals.push(param.clone());
        }

        Ok(MIRFunction {
            name: function.name,
            parameters,
            return_type,
            locals,
            basic_blocks,
            entry_block,
        })
    }

    fn transform_class(&mut self, class: HIRClass) -> IRResult<MIRClass> {
        let mut fields = Vec::new();
        let mut methods = HashMap::new();
        let mut field_offset = 0;

        // Transform fields
        for field in class.fields {
            let field_type = self.transform_hir_type_to_mir(field.field_type)?;
            fields.push(MIRField {
                name: field.name,
                field_type: field_type.clone(),
                offset: field_offset,
            });
            field_offset += self.get_type_size(&field_type);
        }

        // Transform methods
        for method in class.methods {
            let mir_method = self.transform_function(method)?;
            methods.insert(mir_method.name.clone(), mir_method);
        }

        Ok(MIRClass {
            name: class.name,
            fields,
            methods,
        })
    }

    fn transform_statements_to_blocks(
        &mut self,
        statements: Vec<HIRStatement>,
        entry_block: BlockId,
    ) -> IRResult<(Vec<MIRBasicBlock>, Vec<MIRLocal>)> {
        let mut basic_blocks = Vec::new();
        let mut locals = Vec::new();
        let mut current_instructions = Vec::new();

        let mut current_block_id = entry_block;

        for statement in statements {
            match statement {
                HIRStatement::Expression(expr) => {
                    let _result_local = self.transform_expression_to_instructions(
                        expr,
                        &mut current_instructions,
                        &mut locals,
                    )?;
                }
                HIRStatement::Assignment(assignment) => {
                    let value_local = self.transform_expression_to_instructions(
                        assignment.value,
                        &mut current_instructions,
                        &mut locals,
                    )?;

                    match assignment.target {
                        HIRLValue::Variable(var_name) => {
                            // Find or create local for the variable
                            let target_local = self.find_or_create_local(&var_name, &mut locals);
                            current_instructions.push(MIRInstruction::Load(
                                target_local,
                                MIROperand::Local(value_local),
                            ));
                        }
                        _ => {
                            // TODO: Handle other LValue types
                        }
                    }
                }
                HIRStatement::Return(return_value) => {
                    let return_operand = match return_value {
                        Some(expr) => {
                            let local = self.transform_expression_to_instructions(
                                expr,
                                &mut current_instructions,
                                &mut locals,
                            )?;
                            Some(MIROperand::Local(local))
                        }
                        None => None,
                    };

                    // End current block with return
                    let block = MIRBasicBlock {
                        id: current_block_id,
                        instructions: current_instructions.clone(),
                        terminator: MIRTerminator::Return(return_operand),
                    };
                    basic_blocks.push(block);
                    current_instructions.clear();
                    return Ok((basic_blocks, locals));
                }
                HIRStatement::If(if_stmt) => {
                    // Transform conditional to basic blocks
                    let condition_local = self.transform_expression_to_instructions(
                        if_stmt.condition,
                        &mut current_instructions,
                        &mut locals,
                    )?;

                    let then_block = self.next_block();
                    let else_block = self.next_block();
                    let merge_block = self.next_block();

                    // End current block with conditional branch
                    let branch_block = MIRBasicBlock {
                        id: current_block_id,
                        instructions: current_instructions.clone(),
                        terminator: MIRTerminator::Branch {
                            condition: MIROperand::Local(condition_local),
                            then_block,
                            else_block,
                        },
                    };
                    basic_blocks.push(branch_block);
                    current_instructions.clear();

                    // Transform then branch
                    let mut then_instructions = Vec::new();
                    for stmt in if_stmt.then_branch {
                        match stmt {
                            HIRStatement::Return(_) => {
                                let stmt_result = self.transform_statement_to_instructions(
                                    stmt,
                                    &mut then_instructions,
                                    &mut locals,
                                )?;
                                if let Some(terminator) = stmt_result {
                                    let then_bb = MIRBasicBlock {
                                        id: then_block,
                                        instructions: then_instructions.clone(),
                                        terminator,
                                    };
                                    basic_blocks.push(then_bb);
                                    then_instructions.clear(); // Mark as processed
                                    break;
                                }
                            }
                            _ => {
                                let _stmt_result = self.transform_statement_to_instructions(
                                    stmt,
                                    &mut then_instructions,
                                    &mut locals,
                                )?;
                            }
                        }
                    }

                    // Add then block if it wasn't terminated by return
                    if !basic_blocks.iter().any(|bb| bb.id == then_block)
                        && !then_instructions.is_empty()
                    {
                        let then_bb = MIRBasicBlock {
                            id: then_block,
                            instructions: then_instructions,
                            terminator: MIRTerminator::Goto(merge_block),
                        };
                        basic_blocks.push(then_bb);
                    }

                    // Transform else branch
                    let mut else_instructions = Vec::new();
                    if let Some(else_branch) = if_stmt.else_branch {
                        for stmt in else_branch {
                            let _stmt_result = self.transform_statement_to_instructions(
                                stmt,
                                &mut else_instructions,
                                &mut locals,
                            )?;
                        }
                    }

                    let else_bb = MIRBasicBlock {
                        id: else_block,
                        instructions: else_instructions,
                        terminator: MIRTerminator::Goto(merge_block),
                    };
                    basic_blocks.push(else_bb);

                    // Continue with merge block
                    current_block_id = merge_block;
                }
                _ => {
                    // Handle other statement types
                    let _result = self.transform_statement_to_instructions(
                        statement,
                        &mut current_instructions,
                        &mut locals,
                    )?;
                }
            }
        }

        // If we have remaining instructions, create final block
        if !current_instructions.is_empty() || basic_blocks.is_empty() {
            let final_block = MIRBasicBlock {
                id: current_block_id,
                instructions: current_instructions,
                terminator: MIRTerminator::Return(None), // Default return
            };
            basic_blocks.push(final_block);
        }

        Ok((basic_blocks, locals))
    }

    fn transform_statement_to_instructions(
        &mut self,
        statement: HIRStatement,
        instructions: &mut Vec<MIRInstruction>,
        locals: &mut Vec<MIRLocal>,
    ) -> IRResult<Option<MIRTerminator>> {
        match statement {
            HIRStatement::Return(return_value) => {
                let return_operand = match return_value {
                    Some(expr) => {
                        let local =
                            self.transform_expression_to_instructions(expr, instructions, locals)?;
                        Some(MIROperand::Local(local))
                    }
                    None => None,
                };
                Ok(Some(MIRTerminator::Return(return_operand)))
            }
            HIRStatement::Expression(expr) => {
                let _result =
                    self.transform_expression_to_instructions(expr, instructions, locals)?;
                Ok(None)
            }
            HIRStatement::Assignment(assignment) => {
                let value_local = self.transform_expression_to_instructions(
                    assignment.value,
                    instructions,
                    locals,
                )?;

                match assignment.target {
                    HIRLValue::Variable(var_name) => {
                        let target_local = self.find_or_create_local(&var_name, locals);
                        instructions.push(MIRInstruction::Load(
                            target_local,
                            MIROperand::Local(value_local),
                        ));
                    }
                    _ => {
                        // TODO: Handle other LValue types
                    }
                }
                Ok(None)
            }
            _ => Ok(None), // TODO: Handle other statement types
        }
    }

    fn transform_expression_to_instructions(
        &mut self,
        expression: HIRExpression,
        instructions: &mut Vec<MIRInstruction>,
        locals: &mut Vec<MIRLocal>,
    ) -> IRResult<LocalId> {
        match expression {
            HIRExpression::Literal(literal) => {
                let result_local = self.next_local();
                let constant = match literal {
                    HIRLiteral::Integer(i) => MIRConstant::Integer(i),
                    HIRLiteral::Number(f) => MIRConstant::Number(f),
                    HIRLiteral::String(s) => MIRConstant::String(s),
                    HIRLiteral::Boolean(b) => MIRConstant::Boolean(b),
                    _ => MIRConstant::Boolean(false), // Default for unsupported types
                };

                // Add local for result
                locals.push(MIRLocal {
                    id: result_local,
                    name: format!("temp_{}", result_local),
                    local_type: self.infer_mir_type_from_constant(&constant),
                });

                instructions.push(MIRInstruction::Const(result_local, constant));
                Ok(result_local)
            }
            HIRExpression::Variable(name) => {
                let local_id = self.find_or_create_local(&name, locals);
                Ok(local_id)
            }
            HIRExpression::Binary(binary) => {
                let left_local =
                    self.transform_expression_to_instructions(*binary.left, instructions, locals)?;
                let right_local =
                    self.transform_expression_to_instructions(*binary.right, instructions, locals)?;
                let result_local = self.next_local();

                // Add result local (assume i32 for now)
                locals.push(MIRLocal {
                    id: result_local,
                    name: format!("temp_{}", result_local),
                    local_type: MIRType::I32,
                });

                let instruction = match binary.operator {
                    hir::BinaryOperator::Add => MIRInstruction::Add(
                        result_local,
                        MIROperand::Local(left_local),
                        MIROperand::Local(right_local),
                    ),
                    hir::BinaryOperator::Sub => MIRInstruction::Sub(
                        result_local,
                        MIROperand::Local(left_local),
                        MIROperand::Local(right_local),
                    ),
                    hir::BinaryOperator::Mul => MIRInstruction::Mul(
                        result_local,
                        MIROperand::Local(left_local),
                        MIROperand::Local(right_local),
                    ),
                    hir::BinaryOperator::Div => MIRInstruction::Div(
                        result_local,
                        MIROperand::Local(left_local),
                        MIROperand::Local(right_local),
                    ),
                    _ => {
                        // For other operators, use Add as default
                        MIRInstruction::Add(
                            result_local,
                            MIROperand::Local(left_local),
                            MIROperand::Local(right_local),
                        )
                    }
                };

                instructions.push(instruction);
                Ok(result_local)
            }
            HIRExpression::Call(call) => {
                // Transform function arguments
                let mut arg_locals = Vec::new();
                for arg in call.arguments {
                    let arg_local =
                        self.transform_expression_to_instructions(arg, instructions, locals)?;
                    arg_locals.push(MIROperand::Local(arg_local));
                }

                let result_local = self.next_local();
                locals.push(MIRLocal {
                    id: result_local,
                    name: format!("temp_{}", result_local),
                    local_type: MIRType::I32, // Default return type
                });

                // Extract function name
                let function_name = match *call.function {
                    HIRExpression::Variable(name) => name,
                    _ => "unknown".to_string(),
                };

                instructions.push(MIRInstruction::Call(
                    result_local,
                    function_name,
                    arg_locals,
                ));
                Ok(result_local)
            }
            _ => {
                // For other expressions, return a default constant
                let result_local = self.next_local();
                locals.push(MIRLocal {
                    id: result_local,
                    name: format!("temp_{}", result_local),
                    local_type: MIRType::I32,
                });
                instructions.push(MIRInstruction::Const(result_local, MIRConstant::Integer(0)));
                Ok(result_local)
            }
        }
    }

    fn find_or_create_local(&mut self, name: &str, locals: &mut Vec<MIRLocal>) -> LocalId {
        // Look for existing local
        for local in locals.iter() {
            if local.name == name {
                return local.id;
            }
        }

        // Create new local
        let local_id = self.next_local();
        locals.push(MIRLocal {
            id: local_id,
            name: name.to_string(),
            local_type: MIRType::I32, // Default type
        });
        local_id
    }

    fn transform_hir_type_to_mir(&self, hir_type: HIRType) -> IRResult<MIRType> {
        match hir_type {
            HIRType::Integer(Some(8)) => Ok(MIRType::I8),
            HIRType::Integer(Some(16)) => Ok(MIRType::I16),
            HIRType::Integer(Some(64)) => Ok(MIRType::I64),
            HIRType::Integer(_) => Ok(MIRType::I32), // Default integer
            HIRType::Number(Some(32)) => Ok(MIRType::F32),
            HIRType::Number(_) => Ok(MIRType::F64), // Default number
            HIRType::Boolean => Ok(MIRType::Bool),
            HIRType::String => Ok(MIRType::Ptr(Box::new(MIRType::I8))), // String as pointer to chars
            HIRType::List(_) => Ok(MIRType::Ptr(Box::new(MIRType::I8))), // Generic pointer
            HIRType::Class(name) => Ok(MIRType::Struct(name)),
            HIRType::Function(params, ret) => {
                let mut param_types = Vec::new();
                for param in params {
                    param_types.push(self.transform_hir_type_to_mir(param)?);
                }
                Ok(MIRType::Function(
                    param_types,
                    Box::new(self.transform_hir_type_to_mir(*ret)?),
                ))
            }
            HIRType::Any => Ok(MIRType::I32), // Default for Any type
            _ => Ok(MIRType::I32),            // Default for other types
        }
    }

    fn infer_mir_type_from_constant(&self, constant: &MIRConstant) -> MIRType {
        match constant {
            MIRConstant::Integer(_) => MIRType::I32,
            MIRConstant::Number(_) => MIRType::F64,
            MIRConstant::String(_) => MIRType::Ptr(Box::new(MIRType::I8)),
            MIRConstant::Boolean(_) => MIRType::Bool,
        }
    }

    fn get_type_size(&self, mir_type: &MIRType) -> usize {
        match mir_type {
            MIRType::I8 => 1,
            MIRType::I16 => 2,
            MIRType::I32 | MIRType::F32 | MIRType::Bool => 4,
            MIRType::I64 | MIRType::F64 => 8,
            MIRType::Ptr(_) => 4,                  // Assume 32-bit pointers
            MIRType::Array(_, count) => count * 4, // Assume element size of 4
            MIRType::Struct(_) => 8,               // Default struct size
            MIRType::Function(_, _) => 4,          // Function pointer
            MIRType::Void => 0,
        }
    }
}

/// Transform MIR to LIR
pub fn mir_to_lir(program: MIRProgram) -> IRResult<LIRProgram> {
    let mut transformer = MIRToLIRTransformer::new();
    transformer.transform_program(program)
}

struct MIRToLIRTransformer {
    string_constants: HashMap<String, u32>,
    next_string_id: u32,
}

impl MIRToLIRTransformer {
    fn new() -> Self {
        Self {
            string_constants: HashMap::new(),
            next_string_id: 0,
        }
    }

    fn transform_program(&mut self, program: MIRProgram) -> IRResult<LIRProgram> {
        let mut functions = Vec::new();
        let mut imports = Vec::new();
        let mut exports = Vec::new();

        // Add standard imports for Clean Language runtime
        imports.push(LIRImport {
            module: "env".to_string(),
            name: "print".to_string(),
            import_type: LIRImportType::Function(vec![LIRType::I32], None),
        });

        imports.push(LIRImport {
            module: "env".to_string(),
            name: "memory".to_string(),
            import_type: LIRImportType::Memory(1, Some(16)), // 1 initial page, max 16 pages
        });

        // Transform functions
        for (_name, mir_func) in program.functions {
            let lir_func = self.transform_function(mir_func)?;

            // Export the start function if it exists
            if lir_func.name == "start" {
                exports.push(LIRExport {
                    name: "_start".to_string(),
                    export_type: LIRExportType::Function(functions.len()),
                });
            }

            functions.push(lir_func);
        }

        // Export memory
        exports.push(LIRExport {
            name: "memory".to_string(),
            export_type: LIRExportType::Memory(0),
        });

        let memory_layout = LIRMemoryLayout {
            initial_pages: 1,
            max_pages: Some(16),
            heap_start: 1024,   // Start heap after reserved space
            stack_start: 65536, // Start stack at 64KB
        };

        Ok(LIRProgram {
            functions,
            memory_layout,
            imports,
            exports,
        })
    }

    fn transform_function(&mut self, function: MIRFunction) -> IRResult<LIRFunction> {
        let mut instructions = Vec::new();
        let mut locals = Vec::new();
        let mut parameters = Vec::new();

        // Store whether this is a void function for later use
        let _is_void_function = matches!(function.return_type, MIRType::Void);

        // Transform parameters
        for param in &function.parameters {
            parameters.push(self.transform_mir_type_to_lir(param.local_type.clone())?);
        }

        // Transform return type
        let return_type = match function.return_type {
            MIRType::Void => None,
            other => Some(self.transform_mir_type_to_lir(other)?),
        };

        // Transform locals (excluding parameters)
        for local in &function.locals {
            // Skip parameters (they are handled separately in WASM)
            if !function.parameters.iter().any(|p| p.id == local.id) {
                locals.push(self.transform_mir_type_to_lir(local.local_type.clone())?);
            }
        }

        // Transform basic blocks to linear instruction sequence
        for basic_block in &function.basic_blocks {
            if basic_block.id == function.entry_block {
                // Entry block - no label needed
            } else {
                // Add block label (using block instruction for scoping)
                instructions.push(LIRInstruction::Block(LIRType::I32));
            }

            // Transform instructions in the basic block
            for instruction in &basic_block.instructions {
                self.transform_mir_instruction_to_lir(instruction, &mut instructions)?;
            }

            // Transform terminator
            self.transform_mir_terminator_to_lir(
                &basic_block.terminator,
                &mut instructions,
                &function.basic_blocks,
            )?;
        }

        Ok(LIRFunction {
            name: function.name,
            parameters,
            return_type,
            locals,
            instructions,
        })
    }

    fn transform_mir_instruction_to_lir(
        &mut self,
        instruction: &MIRInstruction,
        instructions: &mut Vec<LIRInstruction>,
    ) -> IRResult<()> {
        match instruction {
            MIRInstruction::Add(result, left, right) => {
                // Load operands onto stack
                self.transform_operand_to_lir(left, instructions)?;
                self.transform_operand_to_lir(right, instructions)?;

                // Perform addition
                instructions.push(LIRInstruction::I32Add);

                // Store result
                instructions.push(LIRInstruction::LocalSet(*result as u32));
            }
            MIRInstruction::Sub(result, left, right) => {
                self.transform_operand_to_lir(left, instructions)?;
                self.transform_operand_to_lir(right, instructions)?;
                instructions.push(LIRInstruction::I32Sub);
                instructions.push(LIRInstruction::LocalSet(*result as u32));
            }
            MIRInstruction::Mul(result, left, right) => {
                self.transform_operand_to_lir(left, instructions)?;
                self.transform_operand_to_lir(right, instructions)?;
                instructions.push(LIRInstruction::I32Mul);
                instructions.push(LIRInstruction::LocalSet(*result as u32));
            }
            MIRInstruction::Div(result, left, right) => {
                self.transform_operand_to_lir(left, instructions)?;
                self.transform_operand_to_lir(right, instructions)?;
                instructions.push(LIRInstruction::I32DivS); // Signed division
                instructions.push(LIRInstruction::LocalSet(*result as u32));
            }
            MIRInstruction::Load(result, operand) => {
                // Load from operand and store in result
                self.transform_operand_to_lir(operand, instructions)?;
                instructions.push(LIRInstruction::LocalSet(*result as u32));
            }
            MIRInstruction::Store(target, value) => {
                // For memory store operations
                self.transform_operand_to_lir(target, instructions)?;
                self.transform_operand_to_lir(value, instructions)?;
                instructions.push(LIRInstruction::I32Store(4, 0)); // Align=4, offset=0
            }
            MIRInstruction::Call(result, function_name, args) => {
                // Load arguments onto stack
                for arg in args {
                    self.transform_operand_to_lir(arg, instructions)?;
                }

                // Call function (using index 0 for now - should be resolved properly)
                let function_index = self.get_function_index(function_name);
                instructions.push(LIRInstruction::Call(function_index));

                // Store result if needed, or drop if unused
                // For now, we'll check if this is a simple case where we should drop
                let should_drop = self.should_drop_result(function_name);
                // eprintln!(
                //     "🔥 FUNCTION CALL DEBUG: '{}' -> should_drop: {}",
                //     function_name, should_drop
                // );
                if should_drop {
                    // eprintln!("🔥 ADDING DROP for function: {}", function_name);
                    instructions.push(LIRInstruction::Drop);
                } else {
                    // eprintln!("🔥 ADDING LOCALSET for function: {}", function_name);
                    instructions.push(LIRInstruction::LocalSet(*result as u32));
                }
            }
            MIRInstruction::Cast(result, operand, target_type) => {
                self.transform_operand_to_lir(operand, instructions)?;

                // Add type conversion instructions based on target type
                match target_type {
                    MIRType::F32 => instructions.push(LIRInstruction::F32ConvertI32S),
                    MIRType::F64 => instructions.push(LIRInstruction::F64ConvertI32S),
                    MIRType::I64 => instructions.push(LIRInstruction::I64ExtendI32S),
                    _ => {} // No conversion needed or not supported
                }

                instructions.push(LIRInstruction::LocalSet(*result as u32));
            }
            MIRInstruction::Const(result, constant) => {
                match constant {
                    MIRConstant::Integer(i) => {
                        instructions.push(LIRInstruction::I32Const(*i as i32));
                    }
                    MIRConstant::Number(f) => {
                        instructions.push(LIRInstruction::F64Const(*f));
                    }
                    MIRConstant::String(s) => {
                        // String constants are handled specially
                        let string_id = self.get_string_constant_id(s.clone());
                        instructions.push(LIRInstruction::I32Const(string_id as i32));
                    }
                    MIRConstant::Boolean(b) => {
                        instructions.push(LIRInstruction::I32Const(if *b { 1 } else { 0 }));
                    }
                }
                instructions.push(LIRInstruction::LocalSet(*result as u32));
            }
        }
        Ok(())
    }

    fn transform_mir_terminator_to_lir(
        &mut self,
        terminator: &MIRTerminator,
        instructions: &mut Vec<LIRInstruction>,
        _basic_blocks: &[MIRBasicBlock],
    ) -> IRResult<()> {
        match terminator {
            MIRTerminator::Return(operand) => {
                if let Some(op) = operand {
                    self.transform_operand_to_lir(op, instructions)?;
                }
                instructions.push(LIRInstruction::Return);
            }
            MIRTerminator::Goto(_target_block) => {
                // Simple goto - use branch instruction
                instructions.push(LIRInstruction::Br(0)); // Branch to label 0
            }
            MIRTerminator::Branch {
                condition,
                then_block: _,
                else_block: _,
            } => {
                // Conditional branch
                self.transform_operand_to_lir(condition, instructions)?;
                instructions.push(LIRInstruction::If(LIRType::I32));
                // Then branch
                instructions.push(LIRInstruction::Br(1)); // Branch to then
                instructions.push(LIRInstruction::Else);
                // Else branch
                instructions.push(LIRInstruction::Br(2)); // Branch to else
                instructions.push(LIRInstruction::End);
            }
            MIRTerminator::Unreachable => {
                // This could be implemented as a trap or error
                instructions.push(LIRInstruction::Return); // Fallback
            }
        }
        Ok(())
    }

    fn transform_operand_to_lir(
        &mut self,
        operand: &MIROperand,
        instructions: &mut Vec<LIRInstruction>,
    ) -> IRResult<()> {
        match operand {
            MIROperand::Local(local_id) => {
                instructions.push(LIRInstruction::LocalGet(*local_id as u32));
            }
            MIROperand::Constant(constant) => match constant {
                MIRConstant::Integer(i) => {
                    instructions.push(LIRInstruction::I32Const(*i as i32));
                }
                MIRConstant::Number(f) => {
                    instructions.push(LIRInstruction::F64Const(*f));
                }
                MIRConstant::String(s) => {
                    let string_id = self.get_string_constant_id(s.clone());
                    instructions.push(LIRInstruction::I32Const(string_id as i32));
                }
                MIRConstant::Boolean(b) => {
                    instructions.push(LIRInstruction::I32Const(if *b { 1 } else { 0 }));
                }
            },
            MIROperand::Global(name) => {
                // For now, treat globals as local variables
                // This should be improved to use proper global indices
                let global_index = self.get_global_index(name);
                instructions.push(LIRInstruction::GlobalGet(global_index));
            }
        }
        Ok(())
    }

    fn transform_mir_type_to_lir(&self, mir_type: MIRType) -> IRResult<LIRType> {
        match mir_type {
            MIRType::I8 | MIRType::I16 | MIRType::I32 | MIRType::Bool => Ok(LIRType::I32),
            MIRType::I64 => Ok(LIRType::I64),
            MIRType::F32 => Ok(LIRType::F32),
            MIRType::F64 => Ok(LIRType::F64),
            MIRType::Ptr(_) => Ok(LIRType::I32), // Pointers are 32-bit addresses
            MIRType::Array(_, _) => Ok(LIRType::I32), // Arrays are represented as pointers
            MIRType::Struct(_) => Ok(LIRType::I32), // Structs are represented as pointers
            MIRType::Function(_, _) => Ok(LIRType::I32), // Function pointers
            MIRType::Void => Ok(LIRType::I32), // Default to I32 for void (shouldn't happen in practice)
        }
    }

    fn get_string_constant_id(&mut self, string: String) -> u32 {
        if let Some(id) = self.string_constants.get(&string) {
            *id
        } else {
            let id = self.next_string_id;
            self.string_constants.insert(string, id);
            self.next_string_id += 1;
            id
        }
    }

    fn get_function_index(&self, _function_name: &str) -> u32 {
        // For now, return a placeholder index
        // This should be resolved properly with a function table
        0
    }

    fn should_drop_result(&self, function_name: &str) -> bool {
        // Determine if the function call result should be dropped instead of stored
        // This is a heuristic for common void-returning functions
        match function_name {
            // Console output functions typically return void or status codes that can be ignored
            "print" | "printl" => true,
            // File operations that return status codes often ignored in simple cases
            "file_write" | "file_append" | "file_delete" => true,
            // Most method-style calls that return self for chaining
            _ if function_name.ends_with(".toString") => false, // Keep string results
            _ if function_name.ends_with(".length") => false,   // Keep length results
            _ if function_name.starts_with("math.") => false,   // Keep math results
            _ if function_name.starts_with("string.") => false, // Keep string operation results
            _ if function_name.starts_with("list.") => false,   // Keep list operation results
            // Default: don't drop - store the result
            _ => false,
        }
    }

    fn get_global_index(&self, _global_name: &str) -> u32 {
        // For now, return a placeholder index
        // This should be resolved properly with a global table
        0
    }
}
