//! `background` and `later` call sites lowered to host-scheduled tasks
//! (specification 19-async.md, HOST_BRIDGE.md §Async).
//!
//! A deferred call is split in two:
//!
//! * At the statement, the receiver and the arguments are evaluated (left to
//!   right) and stored with the task state in a heap **environment block**,
//!   and `_async_fire_slot(slot, env)` hands the task to the host, which runs
//!   it once the current entry point (`start:`, a request handler, an event
//!   handler) has returned.
//! * A **task function** `__async_task_N(env: i32) -> i32`, referenced through
//!   `__indirect_function_table` (`slot` is its table index), loads the
//!   captured values and performs the call inside an `onError` region, so an
//!   error raised by the task never reaches the code that started it. It
//!   stores the result in the block and returns the address of that result
//!   cell. A task runs once: a second run returns the cell as it is.
//!
//! Reading a `later` variable calls `_async_await_slot(slot, env)` — the host
//! runs the task now if it has not run yet — and loads the result from the
//! returned cell. Assigning to a `later` variable awaits the task and then
//! overwrites the cell.
//!
//! Environment block layout (all offsets in bytes):
//!
//! ```text
//! 0   state     i32  0 = not run yet, 1 = started (the result cell is final)
//! 8   result    8 bytes, the call's value in its WASM representation;
//!                 starts as the zero value of the result type ("" for a
//!                 string), which a failed task leaves in place
//! 16  captures  one 8-byte slot per captured value, in evaluation order
//! ```

use super::*;

/// Byte offset of the task state word in an environment block.
const ENV_STATE_OFFSET: i64 = 0;
/// Byte offset of the result cell in an environment block.
const ENV_RESULT_OFFSET: i64 = 8;
/// Byte offset of the first captured value in an environment block.
const ENV_CAPTURES_OFFSET: i64 = 16;
/// Size of one captured-value slot (wide enough for i64 / f64).
const ENV_SLOT_SIZE: i64 = 8;

/// Host bridge that schedules a task (`HOST_BRIDGE.md` §Async).
pub(super) const ASYNC_FIRE_SLOT: &str = "_async_fire_slot";
/// Host bridge that runs a task now if it has not run, returning its result
/// cell (`HOST_BRIDGE.md` §Async).
pub(super) const ASYNC_AWAIT_SLOT: &str = "_async_await_slot";

/// A task function still to be built (`MirBuilder::build_async_task_function`).
#[derive(Debug)]
pub(in crate::mir) struct AsyncTaskSpec {
    pub(super) symbol_id: SymbolId,
    pub(super) name: String,
    /// Name each captured value is bound to in the task, with its MIR type,
    /// in environment-slot order.
    pub(super) captures: Vec<(String, MirType)>,
    /// The call, with its receiver and arguments replaced by the captured
    /// names.
    pub(super) call: TastExpression,
    /// The type the result cell holds, `None` when the result is discarded.
    pub(super) result: Option<(ConcreteType, MirType)>,
    pub(super) location: SourceLocation,
}

/// A `later` variable: the task whose result it reads.
#[derive(Debug, Clone)]
pub(in crate::mir) struct LaterBinding {
    pub(super) task: SymbolId,
    /// The MIR type of the value in the result cell.
    pub(super) value_type: MirType,
}

impl MirBuilder {
    /// Whether `expression` is a call that `background` / `later` defer to a
    /// task: a function, method or static call, optionally followed by
    /// `onError` (the fallback then runs inside the task).
    pub(super) fn is_deferrable_call(expression: &TastExpression) -> bool {
        match &expression.kind {
            TastExpressionKind::FunctionCall { .. }
            | TastExpressionKind::MethodCall { .. }
            | TastExpressionKind::StaticMethodCall { .. } => true,
            TastExpressionKind::OnError { expression, .. } => matches!(
                expression.kind,
                TastExpressionKind::FunctionCall { .. }
                    | TastExpressionKind::MethodCall { .. }
                    | TastExpressionKind::StaticMethodCall { .. }
            ),
            _ => false,
        }
    }

    /// Lower a deferred call: evaluate its receiver and arguments into a new
    /// environment block, schedule the task with `_async_fire_slot`, and
    /// queue the task function to be built. `keep_result` keeps the call's
    /// value in the block for a `later` binding. Returns the block and the
    /// task.
    pub(super) fn emit_async_task(
        &mut self,
        context: &mut FunctionBuildContext,
        expression: &TastExpression,
        keep_result: bool,
        location: &SourceLocation,
    ) -> Result<(ValueId, LaterBinding), Vec<CompilerError>> {
        let mut call = expression.clone();
        let mut captured: Vec<(String, ValueId, MirType)> = Vec::new();
        self.capture_call_operands(context, &mut call, &mut captured)?;

        let result = if keep_result {
            match &expression.expr_type {
                ConcreteType::Undefined => None,
                result_type => Some((result_type.clone(), self.convert_concrete_type(result_type))),
            }
        } else {
            None
        };

        // The environment block.
        let env = self.new_async_temp(context, MirType::I32, location);
        let size = ENV_CAPTURES_OFFSET + ENV_SLOT_SIZE * captured.len() as i64;
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(env),
                operation: MirOperation::Alloca {
                    size: MirOperand::Constant(MirConstant::Integer(size)),
                    alignment: 8,
                },
                location: location.clone(),
            },
        );
        let not_run = self.new_async_temp(context, MirType::I32, location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(not_run),
                operation: MirOperation::Copy {
                    source: MirOperand::Constant(MirConstant::Integer(0)),
                },
                location: location.clone(),
            },
        );
        self.emit_env_store(context, env, ENV_STATE_OFFSET, not_run, location);
        if let Some((result_type, value_type)) = &result {
            let zero = self.emit_zero_value(context, result_type, value_type, location);
            self.emit_env_store(context, env, ENV_RESULT_OFFSET, zero, location);
        }
        for (index, (_, value, _)) in captured.iter().enumerate() {
            let offset = ENV_CAPTURES_OFFSET + ENV_SLOT_SIZE * index as i64;
            self.emit_env_store(context, env, offset, *value, location);
        }

        let symbol_id = SymbolId(self.next_async_task_symbol_id);
        let task_index = self.next_async_task_symbol_id - SYM_ASYNC_TASK_BASE;
        self.next_async_task_symbol_id += 1;
        let value_type = result
            .as_ref()
            .map(|(_, value_type)| value_type.clone())
            .unwrap_or(MirType::Void);
        self.pending_async_tasks.push(AsyncTaskSpec {
            symbol_id,
            name: format!("__async_task_{task_index}"),
            captures: captured
                .into_iter()
                .map(|(name, _, mir_type)| (name, mir_type))
                .collect(),
            call,
            result,
            location: location.clone(),
        });

        self.emit_async_host_call(context, ASYNC_FIRE_SLOT, symbol_id, env, None, location);
        Ok((
            env,
            LaterBinding {
                task: symbol_id,
                value_type,
            },
        ))
    }

    /// Read a `later` variable: await its task and load the result.
    pub(super) fn emit_later_read(
        &mut self,
        context: &mut FunctionBuildContext,
        env: ValueId,
        binding: &LaterBinding,
        location: &SourceLocation,
    ) -> ValueId {
        let cell = self.new_async_temp(context, MirType::I32, location);
        self.emit_async_host_call(
            context,
            ASYNC_AWAIT_SLOT,
            binding.task,
            env,
            Some(cell),
            location,
        );
        let value = self.new_async_temp(context, binding.value_type.clone(), location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(value),
                operation: MirOperation::Load {
                    source: MirOperand::Value(cell),
                },
                location: location.clone(),
            },
        );
        value
    }

    /// Assign to a `later` variable: await its task (so it runs before the
    /// assignment, and only once), then overwrite the result cell.
    pub(super) fn emit_later_write(
        &mut self,
        context: &mut FunctionBuildContext,
        env: ValueId,
        binding: &LaterBinding,
        value: ValueId,
        location: &SourceLocation,
    ) {
        let cell = self.new_async_temp(context, MirType::I32, location);
        self.emit_async_host_call(
            context,
            ASYNC_AWAIT_SLOT,
            binding.task,
            env,
            Some(cell),
            location,
        );
        self.add_instruction(
            context,
            MirInstruction {
                dest: None,
                operation: MirOperation::Store {
                    destination: MirOperand::Value(cell),
                    value: MirOperand::Value(value),
                },
                location: location.clone(),
            },
        );
    }

    /// The `later` binding a variable's local holds, if any.
    pub(super) fn later_binding(
        context: &FunctionBuildContext,
        value: ValueId,
    ) -> Option<LaterBinding> {
        context.later_bindings.get(&value).cloned()
    }

    /// Replace the operands of the deferred call with captured names,
    /// evaluating them now: the receiver (or the object a receiver field is
    /// read from), then the arguments. Local variables the `onError`
    /// fallback reads are captured too; the fallback itself runs in the task.
    fn capture_call_operands(
        &mut self,
        context: &mut FunctionBuildContext,
        call: &mut TastExpression,
        captured: &mut Vec<(String, ValueId, MirType)>,
    ) -> Result<(), Vec<CompilerError>> {
        match &mut call.kind {
            TastExpressionKind::OnError {
                expression,
                fallback,
            } => {
                self.capture_call_operands(context, expression, captured)?;
                self.capture_free_locals(context, fallback, captured)?;
            }
            TastExpressionKind::FunctionCall {
                function,
                arguments,
                ..
            } => {
                // A function name stays; a function value is captured.
                let names_function = matches!(
                    &function.kind,
                    TastExpressionKind::Variable { name, .. }
                        if !Self::is_local(context, name)
                );
                if !names_function {
                    self.capture_value(context, function, captured)?;
                }
                for argument in arguments.iter_mut() {
                    self.capture_value(context, argument, captured)?;
                }
            }
            TastExpressionKind::MethodCall {
                receiver,
                arguments,
                ..
            } => {
                self.capture_receiver(context, receiver, captured)?;
                for argument in arguments.iter_mut() {
                    self.capture_value(context, argument, captured)?;
                }
            }
            TastExpressionKind::StaticMethodCall { arguments, .. } => {
                for argument in arguments.iter_mut() {
                    self.capture_value(context, argument, captured)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Capture a method receiver. `this` keeps its name, so the call is
    /// lowered as a call on `this`. A receiver field (`obj.items.add(x)`)
    /// is read in the task from the captured object, so a list that grows
    /// is written back into the field. A name that is not a local (a state
    /// variable or a namespace) is read in the task.
    fn capture_receiver(
        &mut self,
        context: &mut FunctionBuildContext,
        receiver: &mut TastExpression,
        captured: &mut Vec<(String, ValueId, MirType)>,
    ) -> Result<(), Vec<CompilerError>> {
        match &mut receiver.kind {
            TastExpressionKind::Variable { name, .. } if !Self::is_local(context, name) => Ok(()),
            TastExpressionKind::PropertyAccess { object, .. }
                if matches!(object.kind, TastExpressionKind::Variable { .. }) =>
            {
                self.capture_receiver(context, object, captured)
            }
            _ => self.capture_value(context, receiver, captured),
        }
    }

    /// Evaluate `expression` now and replace it with the captured name.
    fn capture_value(
        &mut self,
        context: &mut FunctionBuildContext,
        expression: &mut TastExpression,
        captured: &mut Vec<(String, ValueId, MirType)>,
    ) -> Result<(), Vec<CompilerError>> {
        let value = self.build_expression(context, expression)?;
        let mir_type = Self::value_type(context, value)
            .unwrap_or_else(|| self.convert_concrete_type(&expression.expr_type));
        // `this` keeps its name: the task's call is then a call on `this`.
        let name = match &expression.kind {
            TastExpressionKind::Variable { name, .. } if name == "this" => "this".to_string(),
            _ => format!("__async_arg{}", captured.len()),
        };
        if !captured.iter().any(|(existing, _, _)| *existing == name) {
            captured.push((name.clone(), value, mir_type));
        }
        expression.kind = TastExpressionKind::Variable {
            symbol_id: SymbolId(0),
            name,
        };
        Ok(())
    }

    /// Capture, under their own names, the local variables `expression`
    /// reads (the `onError` fallback of a deferred call).
    fn capture_free_locals(
        &mut self,
        context: &mut FunctionBuildContext,
        expression: &TastExpression,
        captured: &mut Vec<(String, ValueId, MirType)>,
    ) -> Result<(), Vec<CompilerError>> {
        let mut names = Vec::new();
        Self::collect_variable_names(expression, &mut names);
        for (name, variable) in names {
            if !Self::is_local(context, &name) || captured.iter().any(|(n, _, _)| *n == name) {
                continue;
            }
            let value = self.build_expression(context, &variable)?;
            let mir_type = Self::value_type(context, value)
                .unwrap_or_else(|| self.convert_concrete_type(&variable.expr_type));
            captured.push((name, value, mir_type));
        }
        Ok(())
    }

    fn collect_variable_names(
        expression: &TastExpression,
        out: &mut Vec<(String, TastExpression)>,
    ) {
        let mut visit = |e: &TastExpression| Self::collect_variable_names(e, out);
        match &expression.kind {
            TastExpressionKind::Literal { .. } => {}
            TastExpressionKind::Variable { name, .. } => {
                if !out.iter().any(|(n, _)| n == name) {
                    out.push((name.clone(), expression.clone()));
                }
            }
            TastExpressionKind::BinaryOperation { left, right, .. } => {
                visit(left);
                visit(right);
            }
            TastExpressionKind::UnaryOperation { operand, .. } => visit(operand),
            TastExpressionKind::FunctionCall {
                function,
                arguments,
                ..
            } => {
                visit(function);
                arguments.iter().for_each(visit);
            }
            TastExpressionKind::MethodCall {
                receiver,
                arguments,
                ..
            } => {
                visit(receiver);
                arguments.iter().for_each(visit);
            }
            TastExpressionKind::StaticMethodCall { arguments, .. }
            | TastExpressionKind::BaseCall { arguments, .. } => arguments.iter().for_each(visit),
            TastExpressionKind::PropertyAccess { object, .. } => visit(object),
            TastExpressionKind::ArrayLiteral { elements, .. } => elements.iter().for_each(visit),
            TastExpressionKind::ArrayAccess { array, index } => {
                visit(array);
                visit(index);
            }
            TastExpressionKind::ObjectLiteral { fields } => {
                fields.iter().for_each(|field| visit(&field.value))
            }
            TastExpressionKind::OnError {
                expression,
                fallback,
            } => {
                visit(expression);
                visit(fallback);
            }
            TastExpressionKind::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                visit(condition);
                visit(then_expr);
                visit(else_expr);
            }
            TastExpressionKind::Range {
                start, end, step, ..
            } => {
                visit(start);
                visit(end);
                if let Some(step) = step {
                    visit(step);
                }
            }
        }
    }

    /// Whether `name` is a local variable or parameter of the function
    /// being built (`this` included).
    fn is_local(context: &FunctionBuildContext, name: &str) -> bool {
        context
            .scope_stack
            .iter()
            .any(|scope| scope.contains_key(name))
            || (name == "this"
                && context
                    .function
                    .parameters
                    .first()
                    .is_some_and(|p| p.name == "this"))
    }

    /// The MIR type of a value: its local's or its parameter's.
    pub(super) fn value_type(context: &FunctionBuildContext, value: ValueId) -> Option<MirType> {
        context
            .function
            .locals
            .get(&value)
            .map(|local| local.local_type.clone())
            .or_else(|| {
                context
                    .function
                    .parameters
                    .iter()
                    .find(|p| p.value_id == value)
                    .map(|p| p.param_type.clone())
            })
    }

    fn new_async_temp(
        &mut self,
        context: &mut FunctionBuildContext,
        mir_type: MirType,
        location: &SourceLocation,
    ) -> ValueId {
        let id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, id, mir_type, location.clone());
        id
    }

    /// `*(env + offset) = value`, as wide as the value's type.
    fn emit_env_store(
        &mut self,
        context: &mut FunctionBuildContext,
        env: ValueId,
        offset: i64,
        value: ValueId,
        location: &SourceLocation,
    ) {
        let address = self.emit_env_address(context, env, offset, location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: None,
                operation: MirOperation::Store {
                    destination: MirOperand::Value(address),
                    value: MirOperand::Value(value),
                },
                location: location.clone(),
            },
        );
    }

    fn emit_env_address(
        &mut self,
        context: &mut FunctionBuildContext,
        env: ValueId,
        offset: i64,
        location: &SourceLocation,
    ) -> ValueId {
        let address = self.new_async_temp(context, MirType::I32, location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(address),
                operation: MirOperation::GetElementPtr {
                    base: MirOperand::Value(env),
                    indices: vec![MirOperand::Constant(MirConstant::Integer(offset))],
                    is_array: false,
                },
                location: location.clone(),
            },
        );
        address
    }

    /// The zero value of a result type: what a `later` binding reads when
    /// its task failed.
    fn emit_zero_value(
        &mut self,
        context: &mut FunctionBuildContext,
        result_type: &ConcreteType,
        value_type: &MirType,
        location: &SourceLocation,
    ) -> ValueId {
        let constant = match (result_type, value_type) {
            (ConcreteType::String, _) => MirConstant::String(self.get_string_index(String::new())),
            (_, MirType::F64 | MirType::F32) => MirConstant::Float(0.0),
            (_, MirType::I64 | MirType::U64) => MirConstant::Integer64(0),
            _ => MirConstant::Integer(0),
        };
        let zero = self.new_async_temp(context, value_type.clone(), location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(zero),
                operation: MirOperation::Copy {
                    source: MirOperand::Constant(constant),
                },
                location: location.clone(),
            },
        );
        zero
    }

    /// `bridge(slot of task, env)`, the slot being the task function's
    /// table index (a function reference, which also keeps the task alive).
    fn emit_async_host_call(
        &mut self,
        context: &mut FunctionBuildContext,
        bridge: &str,
        task: SymbolId,
        env: ValueId,
        dest: Option<ValueId>,
        location: &SourceLocation,
    ) {
        let slot = self.new_async_temp(context, MirType::I32, location);
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(slot),
                operation: MirOperation::Copy {
                    source: MirOperand::Function(task),
                },
                location: location.clone(),
            },
        );
        self.add_instruction(
            context,
            MirInstruction {
                dest,
                operation: MirOperation::Call {
                    function: MirOperand::NamedFunction {
                        name: bridge.to_string(),
                        symbol_id: SymbolId(0),
                    },
                    arguments: vec![MirOperand::Value(slot), MirOperand::Value(env)],
                },
                location: location.clone(),
            },
        );
    }

    /// Build a task function:
    ///
    /// ```text
    /// entry:  branch env.state -> done, run
    /// done:   return env + 8
    /// run:    env.state = 1
    ///         <load the captured values>
    ///         onError region: result = <call>; env.result = result
    ///         __err_take()          (a failed task stops here, silently)
    ///         return env + 8
    /// ```
    pub(super) fn build_async_task_function(
        &mut self,
        spec: AsyncTaskSpec,
    ) -> Result<MirFunction, Vec<CompilerError>> {
        let location = spec.location.clone();
        let function = MirFunction {
            symbol_id: spec.symbol_id,
            name: spec.name.clone(),
            parameters: Vec::new(),
            return_type: MirType::I32,
            blocks: HashMap::new(),
            entry_block: BasicBlockId(0),
            locals: HashMap::new(),
            next_value_id: 0,
            next_block_id: 3,
            if_merges: Default::default(),
            attributes: MirFunctionAttributes {
                entry_point: false,
                exported: false,
            },
            location: location.clone(),
        };
        let mut context = FunctionBuildContext {
            function,
            scope_stack: vec![HashMap::new()],
            loop_stack: Vec::new(),
            class_context: None,
            all_classes: self.all_classes.clone(),
            all_functions: self.all_functions.clone(),
            ensure_conditions: Vec::new(),
            error_regions: Vec::new(),
            return_type: ConcreteType::Integer,
            later_bindings: HashMap::new(),
        };

        let env = ValueId(0);
        context.function.next_value_id = 1;
        context.function.parameters.push(MirParameter {
            value_id: env,
            name: "env".to_string(),
            param_type: MirType::I32,
            location: location.clone(),
        });

        let [entry, done, run] = [0, 1, 2].map(BasicBlockId);
        for (id, label) in [
            (entry, "entry"),
            (done, "async_task_done"),
            (run, "async_task_run"),
        ] {
            context.function.blocks.insert(
                id,
                MirBasicBlock {
                    id,
                    label: Some(label.to_string()),
                    instructions: Vec::new(),
                    terminator: MirTerminator::Unreachable,
                    location: location.clone(),
                },
            );
        }

        // entry: has the task run?
        self.current_block = Some(entry);
        let state = self.new_async_temp(&mut context, MirType::I32, &location);
        let state_address = self.emit_env_address(&mut context, env, ENV_STATE_OFFSET, &location);
        self.add_instruction(
            &mut context,
            MirInstruction {
                dest: Some(state),
                operation: MirOperation::Load {
                    source: MirOperand::Value(state_address),
                },
                location: location.clone(),
            },
        );
        self.set_block_terminator(
            &mut context,
            MirTerminator::Branch {
                condition: MirOperand::Value(state),
                true_block: done,
                false_block: run,
            },
        );

        // done: the result cell is final.
        self.current_block = Some(done);
        let cell = self.emit_env_address(&mut context, env, ENV_RESULT_OFFSET, &location);
        self.set_block_terminator(
            &mut context,
            MirTerminator::Return {
                value: Some(MirOperand::Value(cell)),
            },
        );

        // run: mark the task started, so it runs once even if it awaits
        // itself.
        self.current_block = Some(run);
        let started = self.new_async_temp(&mut context, MirType::I32, &location);
        self.add_instruction(
            &mut context,
            MirInstruction {
                dest: Some(started),
                operation: MirOperation::Copy {
                    source: MirOperand::Constant(MirConstant::Integer(1)),
                },
                location: location.clone(),
            },
        );
        self.emit_env_store(&mut context, env, ENV_STATE_OFFSET, started, &location);

        for (index, (name, mir_type)) in spec.captures.iter().enumerate() {
            let offset = ENV_CAPTURES_OFFSET + ENV_SLOT_SIZE * index as i64;
            let address = self.emit_env_address(&mut context, env, offset, &location);
            let value = self.new_async_temp(&mut context, mir_type.clone(), &location);
            self.add_instruction(
                &mut context,
                MirInstruction {
                    dest: Some(value),
                    operation: MirOperation::Load {
                        source: MirOperand::Value(address),
                    },
                    location: location.clone(),
                },
            );
            if let Some(scope) = context.scope_stack.last_mut() {
                scope.insert(name.clone(), value);
            }
        }

        self.begin_error_region(&mut context, &location);
        let mut result = self.build_expression(&mut context, &spec.call)?;
        if let Some((result_type, _)) = &spec.result {
            let actual_is_any = matches!(Self::value_type(&context, result), Some(MirType::Any));
            if (actual_is_any || matches!(spec.call.expr_type, ConcreteType::Any))
                && !matches!(result_type, ConcreteType::Any)
            {
                result = self.emit_unbox_any(&mut context, result, result_type, &location);
            }
            self.emit_env_store(&mut context, env, ENV_RESULT_OFFSET, result, &location);
        }
        self.end_error_region(&mut context, &location)?;
        // An error raised by the call ends the task here and goes no further
        // (19-async.md, Background Task Error Handling).
        let failed = self.new_async_temp(&mut context, MirType::I32, &location);
        self.emit_error_runtime_call(&mut context, "__err_take", Some(failed), &location);
        let cell = self.emit_env_address(&mut context, env, ENV_RESULT_OFFSET, &location);
        self.set_block_terminator(
            &mut context,
            MirTerminator::Return {
                value: Some(MirOperand::Value(cell)),
            },
        );
        self.current_block = None;
        Ok(context.function)
    }
}
