//! Type helpers — type conversion and type mapping utilities.

use super::*;

impl MirBuilder {
    /// Convert ConcreteType to MirType
    pub(super) fn convert_concrete_type(&self, concrete_type: &ConcreteType) -> MirType {
        MirType::from_concrete_type(concrete_type)
    }

    /// Get the AnyTypeTag for a given ConcreteType
    pub(super) fn get_any_type_tag(concrete_type: &ConcreteType) -> AnyTypeTag {
        match concrete_type {
            ConcreteType::Integer => AnyTypeTag::Integer,
            ConcreteType::Number | ConcreteType::NumberSized { .. } => AnyTypeTag::Number,
            // Boxed as a `number` (see `emit_box_value`).
            ConcreteType::IntegerSized { bits: 64, .. } => AnyTypeTag::Number,
            ConcreteType::Boolean => AnyTypeTag::Boolean,
            ConcreteType::String => AnyTypeTag::String,
            ConcreteType::Array(_) | ConcreteType::Matrix(_) => AnyTypeTag::List,
            ConcreteType::Class { .. } | ConcreteType::Interface { .. } => AnyTypeTag::Object,
            ConcreteType::Null | ConcreteType::Undefined => AnyTypeTag::Null,
            // For any other types, default to Integer (they'll be stored as i32)
            _ => AnyTypeTag::Integer,
        }
    }

    /// Box a value to any type - emits a BoxAny instruction.
    ///
    /// For native Clean collections (`List<T>` / `Pairs<K,V>`) the raw value
    /// pointer can't be stored directly in the box: `List`-tagged and
    /// `Object`-tagged payloads follow the JSON-tree layout
    /// (`[count][boxed_elem]…` / `[count][key,boxed_val]…`,
    /// BOXED_ANY_ABI §3), but Clean's typed-collection heap stores raw
    /// elements at a different stride. Normalize the value at box time
    /// by routing it through `__json_from_cln_list` / `__json_from_cln_pairs`
    /// — the box's payload is then a real JSON tree node, and every Any
    /// consumer (json.encode, json.dataToText, embedded `any` object
    /// literals) sees the same canonical representation.
    pub(super) fn emit_box_any(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        source_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        // A value that is already a boxed `any` at the MIR level (a JSON
        // bridge result typed from its context by the checker) is not boxed
        // a second time.
        if context
            .function
            .locals
            .get(&value_id)
            .is_some_and(|local| local.local_type == MirType::Any)
        {
            return value_id;
        }

        let type_tag = Self::get_any_type_tag(source_type);
        let mir_source_type = self.convert_concrete_type(source_type);

        // Normalize raw Clean collections into the JSON-tree layout before
        // boxing. The converter result is stored in a fresh local (i32) and
        // becomes the BoxAny payload. For primitive sources we skip this step.
        let normalized_value_id = match source_type {
            // Elements that are already boxed (`list<any>`) or that box
            // through their class's JSON builder.
            ConcreteType::Array(elem_type)
                if matches!(**elem_type, ConcreteType::Any | ConcreteType::Class { .. }) =>
            {
                let operation = self.json_list_tree_call(context, value_id, elem_type);
                let result_id = ValueId(context.function.next_value_id);
                context.function.next_value_id += 1;
                self.register_temp_local(context, result_id, MirType::I32, location.clone());
                self.add_instruction(
                    context,
                    MirInstruction {
                        dest: Some(result_id),
                        operation,
                        location: location.clone(),
                    },
                );
                result_id
            }
            ConcreteType::Array(elem_type) => {
                let elem_tag = Self::get_any_type_tag(elem_type) as i64;
                let converter =
                    if matches!(**elem_type, ConcreteType::IntegerSized { bits: 64, .. }) {
                        "__json_from_cln_list_i64"
                    } else {
                        "__json_from_cln_list"
                    };
                self.emit_cln_collection_to_json(context, value_id, converter, elem_tag, location)
            }
            ConcreteType::Pairs(_key_type, val_type) => {
                let val_tag = Self::get_any_type_tag(val_type) as i64;
                self.emit_cln_collection_to_json(
                    context,
                    value_id,
                    "__json_from_cln_pairs",
                    val_tag,
                    location,
                )
            }
            _ => value_id,
        };

        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        // Register the result as a temp local with Any type (boxed value)
        // NOTE: Boxed values must have MirType::Any, not MirType::I32
        // This ensures proper type tracking for Any variables
        self.register_temp_local(context, result_id, MirType::Any, location.clone());

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation: MirOperation::BoxAny {
                value: MirOperand::Value(normalized_value_id),
                type_tag,
                source_type: mir_source_type,
            },
            location: location.clone(),
        };

        self.add_instruction(context, instruction);
        result_id
    }

    /// Emit a Call to one of the Clean-collection→JSON-tree converters
    /// (`__json_from_cln_list` / `__json_from_cln_pairs`) and return the
    /// ValueId of the converted JSON tree pointer.
    fn emit_cln_collection_to_json(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        converter_name: &str,
        type_tag: i64,
        location: &SourceLocation,
    ) -> ValueId {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, MirType::I32, location.clone());

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation: MirOperation::Call {
                function: MirOperand::NamedFunction {
                    name: converter_name.to_string(),
                    symbol_id: SymbolId(0),
                },
                arguments: vec![
                    MirOperand::Value(value_id),
                    MirOperand::Constant(MirConstant::Integer(type_tag)),
                ],
            },
            location: location.clone(),
        };
        self.add_instruction(context, instruction);
        result_id
    }

    /// Lower a `json.*` call to its Layer 2 host bridge
    /// (foundation/spec/stdlib-reference.md §8):
    ///
    /// | Call | Bridge |
    /// |------|--------|
    /// | `json.textToData` / `json.decode` | `_json_decode_v2`, raising on the 0 sentinel |
    /// | `json.tryTextToData` | `_json_decode_v2`, null on the 0 sentinel |
    /// | `json.dataToText` / `json.encode` | `_json_encode_v2` |
    /// | `json.prettyDataToText` | `_json_encode_pretty_v2` |
    /// | `json.get` | `_json_get` |
    ///
    /// Values handed to the encoders and to `json.get` are boxed to `any`
    /// first. Returns `None` when the call is not one of these.
    pub(super) fn build_json_bridge_call(
        &mut self,
        context: &mut FunctionBuildContext,
        name: Option<&str>,
        arguments: &[TastExpression],
        location: &SourceLocation,
    ) -> Result<Option<ValueId>, Vec<CompilerError>> {
        let Some(name) = name else {
            return Ok(None);
        };
        let text_type = MirType::Ptr(Box::new(MirType::U8));
        match (name, arguments) {
            ("json.textToData" | "json.decode" | "json.tryTextToData", [text]) => {
                let text_id = self.build_expression(context, text)?;
                // The result is a boxed `any` (or the 0 sentinel): typing it
                // `any` makes `== null`, printing and string conversion
                // dispatch on the box's tag instead of reading it as text.
                let raw_id = self.emit_named_call(
                    context,
                    "_json_decode_v2",
                    vec![MirOperand::Value(text_id)],
                    MirType::Any,
                    location,
                );
                // Sentinel handling (D5): the bridge returns 0 on a parse
                // failure. json.tryTextToData substitutes a null; the other
                // decoders raise.
                let checked_id = if name == "json.tryTextToData" {
                    self.emit_json_try_decode_v2_wrapper(context, raw_id, location)
                } else {
                    self.emit_json_decode_v2_or_raise_wrapper(context, raw_id, location)
                };
                Ok(Some(checked_id))
            }
            ("json.dataToText" | "json.encode" | "json.prettyDataToText", [value]) => {
                let boxed_id = self.build_json_value(context, value, location)?;
                let bridge = if name == "json.prettyDataToText" {
                    "_json_encode_pretty_v2"
                } else {
                    "_json_encode_v2"
                };
                Ok(Some(self.emit_named_call(
                    context,
                    bridge,
                    vec![MirOperand::Value(boxed_id)],
                    text_type,
                    location,
                )))
            }
            ("json.get", [source, path]) => {
                let source_id = self.build_json_value(context, source, location)?;
                let path_id = self.build_expression(context, path)?;
                Ok(Some(self.emit_named_call(
                    context,
                    "_json_get",
                    vec![MirOperand::Value(source_id), MirOperand::Value(path_id)],
                    MirType::Any,
                    location,
                )))
            }
            _ => Ok(None),
        }
    }

    /// Lower a built-in `db.*` call (stdlib-reference §9) to the canonical
    /// bridge import the function registry lists it under (e.g. `db.query`
    /// → `_db_query`). Returns `None` for any other call.
    pub(super) fn build_db_bridge_call(
        &mut self,
        context: &mut FunctionBuildContext,
        name: Option<&str>,
        arguments: &[TastExpression],
        result_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<Option<ValueId>, Vec<CompilerError>> {
        const DB_BUILTINS: &[&str] = &[
            "db.query",
            "db.execute",
            "db.begin",
            "db.commit",
            "db.rollback",
        ];
        let Some(name) = name.filter(|n| DB_BUILTINS.contains(n)) else {
            return Ok(None);
        };
        let Some(bridge) = crate::plugins::registry_loader::RegistryIndex::load()
            .ok()
            .and_then(|registry| registry.lookup(name).map(|f| f.name.clone()))
        else {
            return Ok(None);
        };
        let mut operands = Vec::with_capacity(arguments.len());
        for argument in arguments {
            operands.push(MirOperand::Value(self.build_expression(context, argument)?));
        }
        let mir_type = self.convert_concrete_type(result_type);
        Ok(Some(self.emit_named_call(
            context, &bridge, operands, mir_type, location,
        )))
    }

    /// Evaluate `value` as a boxed-Any pointer for a JSON bridge.
    fn build_json_value(
        &mut self,
        context: &mut FunctionBuildContext,
        value: &TastExpression,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        let value_id = self.build_expression(context, value)?;
        Ok(self.json_value_of(context, value_id, &value.expr_type, location))
    }

    /// The boxed JSON value of `value_id` for a JSON bridge. A class
    /// instance becomes the JSON object of its own class
    /// (`__json_tree_instance` reads the class from the instance header);
    /// any other typed value is boxed (`emit_box_any`). When the program
    /// declares classes, a value that can hold an instance (`any`, lists,
    /// pairs, objects) is passed through `__json_normalize`, which replaces
    /// every boxed instance inside it with the object of its fields — the
    /// host only understands JSON-tree objects (BOXED_ANY_ABI §3.3).
    pub(super) fn json_value_of(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        value_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        let has_classes = !self.class_json_tree_ids.is_empty();
        let boxed = match value_type {
            ConcreteType::Class { .. } | ConcreteType::Interface { .. } if has_classes => self
                .emit_named_call(
                    context,
                    "__json_tree_instance",
                    vec![MirOperand::Value(value_id)],
                    MirType::Any,
                    location,
                ),
            ConcreteType::Any => value_id,
            other => self.emit_box_any(context, value_id, other, location),
        };
        let primitive = matches!(
            value_type,
            ConcreteType::String
                | ConcreteType::Boolean
                | ConcreteType::Integer
                | ConcreteType::IntegerSized { .. }
                | ConcreteType::Number
                | ConcreteType::NumberSized { .. }
                | ConcreteType::Null
        );
        if !has_classes || primitive {
            return boxed;
        }
        self.emit_named_call(
            context,
            "__json_normalize",
            vec![MirOperand::Value(boxed)],
            MirType::Any,
            location,
        )
    }

    /// The call that builds the JSON-tree array of a list whose elements are
    /// `any` (copied, `__json_from_cln_list_boxed`) or class instances
    /// (`__json_tree_list_Class`).
    pub(super) fn json_list_tree_call(
        &self,
        context: &FunctionBuildContext,
        list_id: ValueId,
        element_type: &ConcreteType,
    ) -> MirOperation {
        let class_list_fn = match element_type {
            ConcreteType::Class { symbol_id, .. } => context
                .all_classes
                .iter()
                .find(|c| c.symbol_id == *symbol_id)
                .and_then(|c| self.class_json_list_ids.get(&c.name).copied()),
            _ => None,
        };
        MirOperation::Call {
            function: match class_list_fn {
                Some(list_fn) => MirOperand::Function(list_fn),
                None => MirOperand::NamedFunction {
                    name: "__json_from_cln_list_boxed".to_string(),
                    symbol_id: SymbolId(0),
                },
            },
            arguments: vec![MirOperand::Value(list_id)],
        }
    }

    /// Convert one branch of `expr onError fallback` to the type of the whole
    /// expression: box into `any`, widen an `integer` to `number` or
    /// `integer:64`.
    pub(super) fn coerce_on_error_branch(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        branch: &TastExpression,
        result_type: &ConcreteType,
    ) -> ValueId {
        if matches!(result_type, ConcreteType::Any) {
            if matches!(branch.expr_type, ConcreteType::Any | ConcreteType::Null) {
                return value_id;
            }
            return self.emit_box_any(context, value_id, &branch.expr_type, &branch.location);
        }
        let target = self.convert_concrete_type(result_type);
        let source = context
            .function
            .locals
            .get(&value_id)
            .map(|local| local.local_type.clone());
        let widens =
            matches!(target, MirType::F64 | MirType::I64) && matches!(source, Some(MirType::I32));
        if !widens {
            return value_id;
        }
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, target.clone(), branch.location.clone());
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result_id),
                operation: MirOperation::Cast {
                    value: MirOperand::Value(value_id),
                    target_type: target,
                },
                location: branch.location.clone(),
            },
        );
        result_id
    }

    /// The compact JSON text of a class instance: the object of its own
    /// class (`json_value_of`) encoded by `_json_encode_v2`. Used where a
    /// class instance is passed for a string (bridge arguments, `later`
    /// calls).
    pub(super) fn emit_class_json_text(
        &mut self,
        context: &mut FunctionBuildContext,
        instance_id: ValueId,
        instance_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        let tree_id = self.json_value_of(context, instance_id, instance_type, location);
        self.emit_named_call(
            context,
            "_json_encode_v2",
            vec![MirOperand::Value(tree_id)],
            MirType::Ptr(Box::new(MirType::I8)),
            location,
        )
    }

    /// Emit a call to the function or bridge named `name` into a fresh
    /// temporary of type `result_type`.
    pub(super) fn emit_named_call(
        &mut self,
        context: &mut FunctionBuildContext,
        name: &str,
        arguments: Vec<MirOperand>,
        result_type: MirType,
        location: &SourceLocation,
    ) -> ValueId {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, result_type, location.clone());
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result_id),
                operation: MirOperation::Call {
                    function: MirOperand::NamedFunction {
                        name: name.to_string(),
                        symbol_id: SymbolId(0),
                    },
                    arguments,
                },
                location: location.clone(),
            },
        );
        result_id
    }

    /// Emit a Call to the `__json_try_decode_v2` WASM wrapper. The wrapper
    /// null-checks its argument and, on 0 (parse failure per D5), substitutes
    /// a freshly-allocated null-tag boxed-Any pointer; otherwise it returns
    /// the argument unchanged. Used by `json.tryTextToData` to translate the
    /// raw sentinel into the null tag the surrounding `json.dataToText`/etc.
    /// expect.
    pub(super) fn emit_json_try_decode_v2_wrapper(
        &mut self,
        context: &mut FunctionBuildContext,
        raw_result_id: ValueId,
        location: &SourceLocation,
    ) -> ValueId {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, MirType::Any, location.clone());

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation: MirOperation::Call {
                function: MirOperand::NamedFunction {
                    name: "__json_try_decode_v2".to_string(),
                    symbol_id: SymbolId(0),
                },
                arguments: vec![MirOperand::Value(raw_result_id)],
            },
            location: location.clone(),
        };
        self.add_instruction(context, instruction);
        result_id
    }

    /// Emit a Call to the `__json_decode_v2_or_raise` WASM wrapper. The
    /// wrapper null-checks its argument and, on 0 (parse failure per D5),
    /// raises the error "JSON parse failure", which `onError` catches;
    /// otherwise it returns the argument unchanged. Used by
    /// `json.textToData` and `json.decode` per D5's raise semantics.
    pub(super) fn emit_json_decode_v2_or_raise_wrapper(
        &mut self,
        context: &mut FunctionBuildContext,
        raw_result_id: ValueId,
        location: &SourceLocation,
    ) -> ValueId {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, MirType::Any, location.clone());

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation: MirOperation::Call {
                function: MirOperand::NamedFunction {
                    name: "__json_decode_v2_or_raise".to_string(),
                    symbol_id: SymbolId(0),
                },
                arguments: vec![MirOperand::Value(raw_result_id)],
            },
            location: location.clone(),
        };
        self.add_instruction(context, instruction);
        result_id
    }

    /// Give a numeric value the MIR type of the memory slot it is stored into.
    ///
    /// A field store writes as many bytes as the stored value's type has, so
    /// `p.r = 200` into an `integer:8u` field must store a one-byte value and
    /// `a.total = 5` into an `integer:64` field an eight-byte one. Without the
    /// conversion an `integer` value overwrote the next field or left half of
    /// an `integer:64` field stale. The Copy into a local of the slot's type
    /// performs the widening (`emit_numeric_coercion`).
    pub(super) fn coerce_to_slot_type(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        slot_type: &MirType,
        location: &SourceLocation,
    ) -> ValueId {
        let is_numeric = |t: &MirType| {
            matches!(
                t,
                MirType::I8
                    | MirType::I16
                    | MirType::I32
                    | MirType::I64
                    | MirType::U8
                    | MirType::U16
                    | MirType::U32
                    | MirType::U64
                    | MirType::F64
            )
        };
        let Some(value_type) = context
            .function
            .locals
            .get(&value_id)
            .map(|l| l.local_type.clone())
        else {
            return value_id;
        };
        // `number` only converts to and from `integer` and `integer:64`
        // (type-system.md §5); narrower targets are rejected by the type checker.
        let float_pair_unsupported = (value_type == MirType::F64 || *slot_type == MirType::F64)
            && ![&value_type, slot_type]
                .iter()
                .all(|t| matches!(t, MirType::F64 | MirType::I32 | MirType::I64 | MirType::U64));
        if value_type == *slot_type
            || !is_numeric(&value_type)
            || !is_numeric(slot_type)
            || float_pair_unsupported
        {
            return value_id;
        }
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;
        self.register_temp_local(context, result_id, slot_type.clone(), location.clone());
        self.add_instruction(
            context,
            MirInstruction {
                dest: Some(result_id),
                operation: MirOperation::Copy {
                    source: MirOperand::Value(value_id),
                },
                location: location.clone(),
            },
        );
        result_id
    }

    /// An `any` value passed where a parameter expects a primitive is unboxed
    /// at the call site, as `string v = json.get(...)` is at a declaration.
    /// Without it the callee read the boxed Any's address as the string:
    /// `TaskCard().card(json.get(rows, p + ".id"), ...)` rendered garbage.
    pub(super) fn unbox_any_argument(
        &mut self,
        context: &mut FunctionBuildContext,
        arg_id: ValueId,
        arg_type: &ConcreteType,
        param_type: Option<&ConcreteType>,
        location: &SourceLocation,
    ) -> ValueId {
        let Some(param_type) = param_type else {
            return arg_id;
        };
        // Primitives, and the lists, pairs maps and class instances an
        // `any` converts to (`emit_unbox_any`).
        let primitive_param = Self::is_any_conversion_target(param_type);
        let arg_is_any = matches!(arg_type, ConcreteType::Any)
            || matches!(
                context.function.locals.get(&arg_id).map(|l| &l.local_type),
                Some(MirType::Any)
            );
        if primitive_param && arg_is_any {
            self.emit_unbox_any(context, arg_id, param_type, location)
        } else {
            arg_id
        }
    }

    /// Unbox an any value to a specific type - emits an UnboxAny instruction
    pub(super) fn emit_unbox_any(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        target_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ValueId {
        // A class target: the instance the box holds (the header tells an
        // instance from a JSON object), or null. An instance of a class the
        // target does not accept raises an error.
        if matches!(
            target_type,
            ConcreteType::Class { .. } | ConcreteType::Interface { .. }
        ) && !self.class_json_tree_ids.is_empty()
        {
            let conversion = self.instance_conversion(target_type);
            return self.emit_named_call(
                context,
                &conversion,
                vec![MirOperand::Value(value_id)],
                MirType::I32,
                location,
            );
        }

        // A `list<any>` target: a boxed JSON array is a `list<any>`
        // (stdlib-reference.md §8), but its JSON-tree layout is not a Clean
        // list's, so the elements are copied into one.
        if let ConcreteType::Array(element_type) = target_type {
            if matches!(**element_type, ConcreteType::Any) {
                let list_type = self.convert_concrete_type(target_type);
                return self.emit_named_call(
                    context,
                    crate::stdlib::json_class::JSON_TO_CLN_LIST,
                    vec![MirOperand::Value(value_id)],
                    list_type,
                    location,
                );
            }
        }

        // Any other list, and a pairs map: a JSON array or object (the
        // JSON-tree layouts of BOXED_ANY_ABI §3.2-3.3) is converted element
        // by element into the Clean layout; an element of another type
        // raises an error.
        if matches!(
            target_type,
            ConcreteType::Array(_) | ConcreteType::Pairs(_, _)
        ) {
            let result_type = self.convert_concrete_type(target_type);
            let Some(descriptor) = Self::any_conversion_descriptor(target_type) else {
                self.any_conversion_errors.push(CompilerError::type_error(
                    format!("An `any` value cannot be converted to {}", target_type),
                    Some(
                        "An `any` converts to a list or `pairs<string, V>` of integer, number, \
                         boolean, string, `any`, class, list or pairs elements \
                         (`pairs` values cannot be `number` or `integer:64`)"
                            .to_string(),
                    ),
                    Some(location.clone()),
                ));
                return value_id;
            };
            let descriptor_index = self.get_string_index(descriptor);
            return self.emit_named_call(
                context,
                crate::stdlib::json_class::ANY_CONVERT,
                vec![
                    MirOperand::Value(value_id),
                    MirOperand::Constant(MirConstant::String(descriptor_index)),
                    MirOperand::Constant(MirConstant::Integer(0)),
                ],
                result_type,
                location,
            );
        }

        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        let mir_target_type = self.convert_concrete_type(target_type);
        self.register_temp_local(
            context,
            result_id,
            mir_target_type.clone(),
            location.clone(),
        );

        let operation = match target_type {
            ConcreteType::Number => MirOperation::UnboxAnyToF64 {
                value: MirOperand::Value(value_id),
            },
            ConcreteType::IntegerSized { bits: 64, .. } => MirOperation::UnboxAnyToI64 {
                value: MirOperand::Value(value_id),
            },
            ConcreteType::Boolean => MirOperation::UnboxAnyToBoolean {
                value: MirOperand::Value(value_id),
            },
            // For String targets, dispatch on the actual type tag so an Any
            // containing an Integer/Boolean/Number gets `int_to_string` /
            // `bool_to_string` / `float_to_string`, an Any containing a
            // String returns the inner pointer as-is, and an Any containing
            // an Object/Array gets re-encoded as JSON text via
            // `json.dataToText`. Without the dispatch a plain
            // `UnboxAnyToI32` reads offset 4 of the any-struct and treats
            // those bytes as a string pointer — fine for String-tagged Any
            // but produces garbage for every other tag (the root cause of
            // `json.get(arrayOfObjects, "0")` returning an empty/garbage
            // string in production SSR).
            ConcreteType::String => MirOperation::AnyToString {
                value: MirOperand::Value(value_id),
            },
            // Integer and every pointer-represented type (lists, pairs,
            // instances without a JSON runtime): i32 unboxing
            _ => MirOperation::UnboxAnyToI32 {
                value: MirOperand::Value(value_id),
            },
        };

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation,
            location: location.clone(),
        };

        self.add_instruction(context, instruction);
        result_id
    }

    /// The runtime function converting an `any` to the class or capability
    /// `target_type` (`MirProgram::instance_conversions`), registered on
    /// first use: `__any_to_instance_Name`. A target the program declares no
    /// class for converts with the bare `__any_to_instance`.
    fn instance_conversion(&mut self, target_type: &ConcreteType) -> String {
        let (name, accepted): (String, Vec<SymbolId>) = match target_type {
            // The class and every class that inherits from it.
            ConcreteType::Class { symbol_id, .. } => {
                let accepted = self
                    .all_classes
                    .iter()
                    .filter(|class| {
                        let mut current = Some(class.symbol_id);
                        while let Some(sym) = current {
                            if sym == *symbol_id {
                                return true;
                            }
                            current = self
                                .all_classes
                                .iter()
                                .find(|c| c.symbol_id == sym)
                                .and_then(|c| c.parent_class);
                        }
                        false
                    })
                    .map(|class| class.symbol_id)
                    .collect();
                let Some(class) = self.all_classes.iter().find(|c| c.symbol_id == *symbol_id)
                else {
                    return "__any_to_instance".to_string();
                };
                (class.name.clone(), accepted)
            }
            ConcreteType::Interface { symbol_id, .. } => {
                match self.capability_classes.get(symbol_id) {
                    Some((name, classes)) if !name.is_empty() => (name.clone(), classes.clone()),
                    _ => return "__any_to_instance".to_string(),
                }
            }
            _ => return "__any_to_instance".to_string(),
        };
        let mut class_ids: Vec<u32> = accepted
            .iter()
            .filter_map(|sym| self.class_ids.get(sym).copied())
            .collect();
        if class_ids.is_empty() {
            return "__any_to_instance".to_string();
        }
        class_ids.sort_unstable();
        let function: String = format!("__any_to_instance_{}", name)
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.instance_conversions
            .entry(function.clone())
            .or_insert_with(|| crate::mir::mir_types::InstanceConversion {
                message: format!("value is not a {}", name),
                class_ids,
            });
        function
    }

    /// A type an `any` value is converted to where one is returned or
    /// passed as an argument (`emit_unbox_any`).
    pub(super) fn is_any_conversion_target(target_type: &ConcreteType) -> bool {
        matches!(
            target_type,
            ConcreteType::String
                | ConcreteType::Integer
                | ConcreteType::Number
                | ConcreteType::Boolean
                | ConcreteType::Array(_)
                | ConcreteType::Pairs(_, _)
                | ConcreteType::Class { .. }
                | ConcreteType::Interface { .. }
        )
    }

    /// The `__any_convert` descriptor of a list or pairs target
    /// (`json_class::any_descriptor`), or `None` when the type holds an
    /// element no `any` converts to.
    fn any_conversion_descriptor(target_type: &ConcreteType) -> Option<String> {
        use crate::stdlib::json_class::any_descriptor as code;
        fn element(target: &ConcreteType, out: &mut Vec<u8>, in_pairs: bool) -> Option<()> {
            let byte = match target {
                ConcreteType::Any => code::ANY,
                ConcreteType::Integer
                | ConcreteType::IntegerSized {
                    bits: 8 | 16 | 32, ..
                } => code::INTEGER,
                // 8-byte values: list elements only (a pairs entry holds
                // 4 bytes).
                ConcreteType::IntegerSized { bits: 64, .. } if !in_pairs => code::INTEGER_64,
                ConcreteType::Number | ConcreteType::NumberSized { bits: 64 } if !in_pairs => {
                    code::NUMBER
                }
                ConcreteType::Boolean => code::BOOLEAN,
                ConcreteType::String => code::STRING,
                ConcreteType::Class { .. } | ConcreteType::Interface { .. } => code::INSTANCE,
                ConcreteType::Array(inner) => {
                    out.push(code::LIST);
                    return element(inner, out, false);
                }
                ConcreteType::Pairs(key, value) if matches!(**key, ConcreteType::String) => {
                    out.push(code::PAIRS);
                    return element(value, out, true);
                }
                _ => return None,
            };
            out.push(byte);
            Some(())
        }
        let mut out = Vec::new();
        element(target_type, &mut out, false)?;
        String::from_utf8(out).ok()
    }

    /// Convert a boxed `any` value to a string via runtime type-tag dispatch.
    ///
    /// Unlike `emit_unbox_any` with a String target — which would emit a raw
    /// `UnboxAnyToI32` that just reads offset 4 of the any-struct as an i32 —
    /// this dispatches on the type tag and produces an actual length-prefixed
    /// string pointer regardless of whether the contained value was originally
    /// a string, number, integer, or boolean. This is the correct operand for
    /// `string_compare` when comparing `any` against a string literal.
    ///
    /// Result is registered as `Ptr(U8)` (the canonical string pointer type
    /// used elsewhere for host-function string results).
    pub(super) fn emit_any_to_string(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        location: &SourceLocation,
    ) -> ValueId {
        let result_id = ValueId(context.function.next_value_id);
        context.function.next_value_id += 1;

        self.register_temp_local(
            context,
            result_id,
            MirType::Ptr(Box::new(MirType::U8)),
            location.clone(),
        );

        let instruction = MirInstruction {
            dest: Some(result_id),
            operation: MirOperation::AnyToString {
                value: MirOperand::Value(value_id),
            },
            location: location.clone(),
        };

        self.add_instruction(context, instruction);
        result_id
    }

    /// Convert TAST literal to MIR constant. `expr_type` is the type the typechecker assigned
    /// to the surrounding expression — used to decide whether an integer literal should be
    /// emitted as 32-bit (`MirConstant::Integer`) or 64-bit (`MirConstant::Integer64`).
    /// Without this routing the codegen narrows every literal to i32 in `load_constant`,
    /// silently truncating values destined for `integer:64` slots.
    pub(super) fn convert_literal(
        &mut self,
        literal: &TastLiteral,
        expr_type: &ConcreteType,
    ) -> MirConstant {
        match literal {
            TastLiteral::Integer(i) => {
                if Self::is_i64_literal(*i, expr_type) {
                    MirConstant::Integer64(*i)
                } else {
                    MirConstant::Integer(*i)
                }
            }
            TastLiteral::Number(f) => MirConstant::Float(*f),
            TastLiteral::String(s) => {
                let index = self.get_string_index(s.clone());
                MirConstant::String(index)
            }
            TastLiteral::Boolean(b) => MirConstant::Boolean(*b),
            TastLiteral::Null => MirConstant::Null,
        }
    }

    /// Convert TAST literal to its corresponding MIR type, using the surrounding expression's
    /// type to widen integer literals to i64 when appropriate.
    pub(super) fn convert_literal_type(
        &self,
        literal: &TastLiteral,
        expr_type: &ConcreteType,
    ) -> MirType {
        match literal {
            TastLiteral::Integer(i) => {
                if Self::is_i64_literal(*i, expr_type) {
                    MirType::I64
                } else {
                    MirType::I32 // Default integer type
                }
            }
            TastLiteral::Number(_) => MirType::F64, // Default float type
            TastLiteral::String(_) => MirType::Ptr(Box::new(MirType::I8)), // String as i8 pointer
            TastLiteral::Boolean(_) => MirType::Bool,
            TastLiteral::Null => MirType::Ptr(Box::new(MirType::Void)),
        }
    }

    /// True when an integer literal is emitted as an i64: its expression is
    /// typed `integer:64`, or its value does not fit an i32. The second case
    /// covers a literal whose type stayed the unsized `integer` next to an
    /// `integer:64` operand or parameter (`a.total * 3000000000`,
    /// `acc.add(3000000000)`): as an i32 constant it wrapped before widening.
    fn is_i64_literal(value: i64, expr_type: &ConcreteType) -> bool {
        Self::is_i64_target(expr_type) || i32::try_from(value).is_err()
    }

    /// True when the expression's inferred type indicates a 64-bit signed integer destination.
    fn is_i64_target(expr_type: &ConcreteType) -> bool {
        matches!(
            expr_type,
            ConcreteType::IntegerSized {
                bits: 64,
                unsigned: false
            }
        )
    }

    /// Register a ValueId as a temporary local for codegen
    pub(super) fn register_temp_local(
        &self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        mir_type: MirType,
        location: SourceLocation,
    ) {
        let local = MirLocal {
            name: None, // Temporary values don't need names
            local_type: mir_type,
            is_mutable: false, // Temporary results are immutable
            location,
        };
        context.function.locals.insert(value_id, local);
    }

    /// Convert a value to string for print() calls
    /// Returns the ValueId of the string result
    pub(super) fn convert_value_to_string(
        &mut self,
        context: &mut FunctionBuildContext,
        value_id: ValueId,
        value_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<ValueId, Vec<CompilerError>> {
        use crate::typechecker::tast::ConcreteType;

        match value_type {
            ConcreteType::String => {
                // Already a string, use directly
                Ok(value_id)
            }
            ConcreteType::Integer => {
                // Convert integer to string using int_to_string
                self.emit_builtin_call(
                    context,
                    "int_to_string",
                    vec![MirOperand::Value(value_id)],
                    MirType::Ptr(Box::new(MirType::U8)),
                    location,
                )
            }
            ConcreteType::IntegerSized { bits: 64, .. } => {
                // 64-bit integers use int64_to_string (registered with i64 param)
                // so the full signed-64 range round-trips through .toString() without
                // narrowing through int_to_string's i32 parameter.
                self.emit_builtin_call(
                    context,
                    "int64_to_string",
                    vec![MirOperand::Value(value_id)],
                    MirType::Ptr(Box::new(MirType::U8)),
                    location,
                )
            }
            ConcreteType::Number => {
                // Convert float to string using float_to_string
                self.emit_builtin_call(
                    context,
                    "float_to_string",
                    vec![MirOperand::Value(value_id)],
                    MirType::Ptr(Box::new(MirType::U8)),
                    location,
                )
            }
            ConcreteType::Boolean => {
                // Convert boolean to string using bool_to_string
                self.emit_builtin_call(
                    context,
                    "bool_to_string",
                    vec![MirOperand::Value(value_id)],
                    MirType::Ptr(Box::new(MirType::U8)),
                    location,
                )
            }
            _ => {
                // For other types (objects, arrays, etc.), use the value as-is for now
                // In a complete implementation, these would also have toString() methods
                Ok(value_id)
            }
        }
    }

    /// Convert MirType back to ConcreteType for type inference
    /// This is the inverse of MirType::from_concrete_type()
    pub(super) fn mir_type_to_concrete(mir_type: &MirType) -> ConcreteType {
        match mir_type {
            MirType::I32 => ConcreteType::Integer,
            MirType::F64 => ConcreteType::Number,
            MirType::Bool => ConcreteType::Boolean,
            MirType::Void => ConcreteType::Undefined,
            MirType::Ptr(inner) => {
                match **inner {
                    MirType::I8 => ConcreteType::String,
                    // Ptr(U8) is used for string pointers returned from host functions
                    // (e.g., substring, trim, toString conversions). These are strings,
                    // not null values. This is critical for correct method chaining:
                    // e.g., line.substring(...).trim() — the receiver of .trim() is Ptr(U8).
                    MirType::U8 => ConcreteType::String,
                    MirType::Void => ConcreteType::Null,
                    _ => ConcreteType::Null, // Fallback for other pointer types
                }
            }
            MirType::Function {
                parameters,
                return_type,
            } => ConcreteType::Function {
                parameters: parameters.iter().map(Self::mir_type_to_concrete).collect(),
                return_type: Box::new(Self::mir_type_to_concrete(return_type)),
                is_background: false,
            },
            // For types that can't be precisely converted back, use safe defaults
            MirType::I8 | MirType::I16 | MirType::I64 => ConcreteType::Integer,
            MirType::U8 | MirType::U16 | MirType::U32 | MirType::U64 => ConcreteType::Integer,
            MirType::F32 => ConcreteType::Number,
            MirType::Any => ConcreteType::Any, // Boxed any type
        }
    }

    /// Infer the result type of a binary operation
    pub(super) fn infer_binary_operation_type(
        &self,
        left_type: &ConcreteType,
        right_type: &ConcreteType,
        operator: &BinaryOperator,
    ) -> MirType {
        trace!(
            operator = ?operator,
            left_type = ?left_type,
            right_type = ?right_type,
            "Binary operation type inference"
        );

        // NOTE: Comparison and logical operations always return i32 (boolean)
        match operator {
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::LessThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::LessThanOrEqual
            | BinaryOperator::GreaterThanOrEqual
            | BinaryOperator::And
            | BinaryOperator::Or => {
                trace!("Comparison/logical op -> I32");
                return MirType::I32; // Boolean result
            }
            _ => {}
        }

        // For arithmetic and other operations, infer from operand types
        let result = match (left_type, right_type) {
            // Arithmetic operations between numeric types
            (ConcreteType::Integer, ConcreteType::Integer) => MirType::I32,
            (ConcreteType::Number, ConcreteType::Number) => MirType::F64,
            (ConcreteType::Number, ConcreteType::Integer) => MirType::F64,
            (ConcreteType::Integer, ConcreteType::Number) => MirType::F64,

            // Boolean operations
            (ConcreteType::Boolean, ConcreteType::Boolean) => MirType::Bool,

            // String operations (concatenation) - result is string
            // NOTE: Strings are i32 pointers to [len|content] structure in memory
            (ConcreteType::String, ConcreteType::String) => MirType::I32,
            (ConcreteType::String, _) => MirType::I32, // String + any = String
            (_, ConcreteType::String) => MirType::I32, // Any + String = String

            // Array operations (if supported) - result is array pointer
            (ConcreteType::Array(elem_type), ConcreteType::Array(_)) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Same type operations - use the type's MIR representation
            (left, right) if left == right => {
                // Same type operations - use the type's MIR representation
                MirType::from_concrete_type(left)
            }

            // Mixed types or unknown - use left operand type as fallback
            // This handles cases like Class operations, Function operations, etc.
            (left, _) => MirType::from_concrete_type(left),
        };
        trace!(result = ?result, "Type inference result");
        result
    }

    /// Infer the result type of a unary operation
    pub(super) fn infer_unary_operation_type(&self, operand_type: &ConcreteType) -> MirType {
        match operand_type {
            // Numeric operations preserve type
            ConcreteType::Integer => MirType::I32,
            ConcreteType::Number => MirType::F64,

            // Boolean operations preserve type
            ConcreteType::Boolean => MirType::Bool,

            // String operations preserve type
            // Use Ptr(I8) consistent with from_concrete_type and string literal types.
            // Using I32 here caused load_string_argument_for_print to treat the string pointer as
            // an integer and call int_to_string on it, producing garbage memory address output.
            ConcreteType::String => MirType::Ptr(Box::new(MirType::I8)),

            // Array operations preserve pointer type
            ConcreteType::Array(elem_type) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Matrix operations preserve pointer type
            ConcreteType::Matrix(elem_type) => {
                MirType::Ptr(Box::new(MirType::from_concrete_type(elem_type)))
            }

            // Function, Class, and other complex types - use from_concrete_type
            // This handles all remaining ConcreteType variants properly
            other => MirType::from_concrete_type(other),
        }
    }

    /// Convert TAST binary operator to MIR binary operator
    ///
    /// Note: Some operators cannot be directly represented in MIR and should be
    /// handled specially in build_expression (Power, Concatenate).
    pub(super) fn convert_binary_op(&self, op: &BinaryOperator) -> MirBinaryOp {
        match op {
            // Arithmetic operators
            BinaryOperator::Add => MirBinaryOp::Add,
            BinaryOperator::Subtract => MirBinaryOp::Sub,
            BinaryOperator::Multiply => MirBinaryOp::Mul,
            BinaryOperator::Divide => MirBinaryOp::Div,
            BinaryOperator::Modulo => MirBinaryOp::Rem,

            // Comparison operators
            BinaryOperator::Equal => MirBinaryOp::Eq,
            BinaryOperator::NotEqual => MirBinaryOp::Ne,
            BinaryOperator::LessThan => MirBinaryOp::Lt,
            BinaryOperator::GreaterThan => MirBinaryOp::Gt,
            BinaryOperator::LessThanOrEqual => MirBinaryOp::Le,
            BinaryOperator::GreaterThanOrEqual => MirBinaryOp::Ge,
            BinaryOperator::Is => MirBinaryOp::Eq, // Identity is equality for value types

            // Logical operators (And/Or work on booleans, lowered to i32.and/i32.or in WASM)
            // The type system ensures these are used on boolean types
            BinaryOperator::And => MirBinaryOp::And,
            BinaryOperator::Or => MirBinaryOp::Or,

            // Bitwise operators (And/Or work on integers, lowered to i32.and/i32.or in WASM)
            // The type system ensures these are used on integer types

            // CRITICAL: These operators should NEVER reach here - they must be handled in build_expression
            BinaryOperator::Power => {
                panic!("BUG: Power operator should be handled in build_expression as runtime function call, not converted to MIR operator")
            }
            BinaryOperator::Concatenate => {
                panic!("BUG: String concatenation should be handled in build_expression as string.concat call, not converted to MIR operator")
            }
            BinaryOperator::NullCoalesce => {
                panic!("BUG: NullCoalesce operator should be handled in build_expression as a two-way branch, not converted to MIR operator")
            }
        }
    }

    /// Convert TAST unary operator to MIR unary operator
    pub(super) fn convert_unary_op(&self, op: &UnaryOperator) -> MirUnaryOp {
        match op {
            UnaryOperator::Negate => MirUnaryOp::Neg,
            UnaryOperator::Not => MirUnaryOp::Not,
            // BOOK: required-operator - Postfix ! assertion for null check
            UnaryOperator::Required => MirUnaryOp::Required,
        }
    }

    /// Get or create string pool index
    pub(super) fn get_string_index(&mut self, string: String) -> usize {
        if let Some(&index) = self.string_indices.get(&string) {
            index
        } else {
            let index = self.string_pool.len();
            self.string_pool.push(string.clone());
            self.string_indices.insert(string, index);
            index
        }
    }
}
