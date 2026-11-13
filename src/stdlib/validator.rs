use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::{register_stdlib_function, register_stdlib_function_with_locals};
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Type ID for ValidationResult objects in memory
pub const VALIDATION_RESULT_TYPE_ID: u32 = 20;
/// Type ID for ValidationRules objects in memory
pub const VALIDATION_RULES_TYPE_ID: u32 = 21;

/// Byte size of a single field entry in the fields array
/// Layout (80 bytes total):
///   0:  field_name_ptr       (4 bytes)
///   4:  required flag        (4 bytes, 0=optional 1=required)
///   8:  pattern_ptr          (4 bytes, 0=none; a pattern name)
///  12:  range_rule_ptr       (4 bytes, 0=none, ptr to {lo: f64, hi: f64})
///  16:  min_length           (4 bytes, 0=no constraint)
///  20:  max_length           (4 bytes, -1=no constraint)
///  24:  message_ptr          (4 bytes, 0=use default)
///  28:  custom_fn            (4 bytes, 0=none; function table index)
///  32:  type_name_ptr        (4 bytes, 0=none; ptr to type name string)
///  36:  trim_flag            (4 bytes, 0=no trim 1=trim)
///  40:  allowed_values_ptr   (4 bytes, 0=none, ptr to list<string>)
///  44:  default_value_ptr    (4 bytes, 0=none)
///  48:  required_if_field_ptr(4 bytes, 0=none, ptr to field name)
///  52:  required_if_value_ptr(4 bytes, 0=none, ptr to expected value)
///  56:  shape_rules_ptr      (4 bytes, 0=none, ptr to nested ValidationRules)
///  60:  each_rules_ptr       (4 bytes, 0=none, ptr to ValidationRules for list items)
///  64:  min_value            (4 bytes, i32 minimum value)
///  68:  max_value            (4 bytes, i32 maximum value)
///  72:  min_value_active     (4 bytes, 0=inactive 1=active)
///  76:  max_value_active     (4 bytes, 0=inactive 1=active)
///
/// A field entry holds rules only, never data: the values come from the input
/// of `validator.run` / `validator.runField`.
const FIELD_ENTRY_SIZE: i32 = 80;

/// Number of 4-byte words in a field entry.
const FIELD_ENTRY_WORDS: u32 = (FIELD_ENTRY_SIZE / 4) as u32;

/// Boxed-`any` tags (BOXED_ANY_ABI.md §2) the validator reads.
const ANY_TAG_INTEGER: i32 = 1;
const ANY_TAG_BOOLEAN: i32 = 2;
const ANY_TAG_NUMBER: i32 = 3;
const ANY_TAG_STRING: i32 = 4;
const ANY_TAG_ARRAY: i32 = 5;
const ANY_TAG_OBJECT: i32 = 6;

/// Byte size of a boxed `any` (BOXED_ANY_ABI.md §1).
const ANY_BOX_SIZE: i32 = 12;

/// Initial capacity for the fields array (number of field slots)
const INITIAL_FIELD_CAPACITY: i32 = 8;

/// Byte offset of the first element of a list (MEMORY_MODEL.md).
const LIST_DATA_OFFSET: u64 = 16;

/// The pattern names of `validator.match`, in the order of their
/// `string_matches` pattern ids (`text.matches`, stdlib-reference.md §4).
const PATTERNS: [(&str, &str); 8] = [
    ("email", " must be a valid email address"),
    ("url", " must be a valid URL"),
    ("uuid", " must be a valid UUID"),
    ("phone", " must be a valid phone number"),
    ("date", " must be a date in YYYY-MM-DD format"),
    ("integer", " must contain only digits"),
    ("number", " must be a number"),
    ("alphanumeric", " must contain only letters and digits"),
];

/// Byte size of the ValidationResult structure (24 bytes):
///   0:  is_ok              (4 bytes, 1=success 0=failure)
///   4:  value_ptr          (4 bytes, the validated value when ok)
///   8:  errors_list_ptr    (4 bytes, list<string>)
///  12:  field_errors_ptr   (4 bytes, 0=none, field→errors pairs ptr)
///  16:  field_name_ptr     (4 bytes, set by runField, 0 by run)
///  20:  validator_name_ptr (4 bytes, from createWithName, 0 if unnamed)
const RESULT_SIZE: i32 = 24;

/// Byte size of the ValidationRules header:
///   0:  field_count  (4 bytes)
///   4:  capacity     (4 bytes)
///   8:  fields_ptr   (4 bytes)
///  12:  name_ptr     (4 bytes, 0 if unnamed)
const _RULES_HEADER_SIZE: i32 = 16;

/// Help for a call to a validator rule builder with too many arguments:
/// rules never hold data, so the value goes to `validator.run` /
/// `validator.runField` instead.
pub fn too_many_arguments_help(function: &str) -> Option<&'static str> {
    match function {
        "validator.field" => Some(
            "validator.field(v, name) takes no value: rules never hold data. \
             Pass the data to validator.run(v, input) or validator.runField(v, name, value)",
        ),
        "validator.required" => Some(
            "validator.required(v) takes no flag; use validator.optional(v) for an optional field",
        ),
        _ => None,
    }
}

/// `i32.load` / `i32.store` of a 4-aligned word at `offset`.
const fn word(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 2,
        memory_index: 0,
    }
}

fn load(offset: u64) -> Instruction<'static> {
    Instruction::I32Load(word(offset))
}

fn store(offset: u64) -> Instruction<'static> {
    Instruction::I32Store(word(offset))
}

/// An `i32` constant holding the address of a pooled string.
fn text(ptr: u32) -> Instruction<'static> {
    Instruction::I32Const(ptr as i32)
}

fn if_i32() -> Instruction<'static> {
    Instruction::If(BlockType::Result(ValType::I32))
}

fn if_empty() -> Instruction<'static> {
    Instruction::If(BlockType::Empty)
}

/// Validator stdlib module for Clean Language: the rules a `validate` block
/// builds and `validator.run` checks (stdlib-reference.md §15).
pub struct ValidatorManager {
    imports: ValidatorImports,
    helpers: Helpers,
}

/// The pooled strings the validator's error messages are built from.
#[derive(Debug, Clone, Copy)]
pub struct ValidatorStrings {
    pub empty: u32,
    pub dot: u32,
    pub open_bracket: u32,
    pub close_bracket_dot: u32,
    pub comma_space: u32,
    pub is_required: u32,
    pub must_be_string: u32,
    pub must_be_whole: u32,
    pub must_be_number: u32,
    pub must_be_boolean: u32,
    pub must_be_a: u32,
    pub at_least: u32,
    pub at_most: u32,
    pub characters: u32,
    pub between: u32,
    pub and: u32,
    pub one_of: u32,
    pub is_invalid: u32,
    pub must_be_list: u32,
    pub no_match: u32,
    pub true_text: u32,
    pub false_text: u32,
    pub type_string: u32,
    pub type_integer: u32,
    pub type_number: u32,
    pub type_boolean: u32,
    /// The names in `PATTERNS`, in order.
    pub patterns: [u32; 8],
    /// The messages in `PATTERNS`, in order.
    pub pattern_messages: [u32; 8],
}

impl ValidatorStrings {
    /// Pool every validator string in `codegen`'s data section.
    pub fn pool(codegen: &mut CodeGenerator) -> Result<Self, CompilerError> {
        let mut s = |text: &str| codegen.get_or_create_string_offset(text);
        let mut patterns = [0; 8];
        let mut pattern_messages = [0; 8];
        for (i, (name, message)) in PATTERNS.iter().enumerate() {
            patterns[i] = s(name)?;
            pattern_messages[i] = s(message)?;
        }
        Ok(Self {
            empty: s("")?,
            dot: s(".")?,
            open_bracket: s("[")?,
            close_bracket_dot: s("].")?,
            comma_space: s(", ")?,
            is_required: s(" is required")?,
            must_be_string: s(" must be a string")?,
            must_be_whole: s(" must be a whole number")?,
            must_be_number: s(" must be a number")?,
            must_be_boolean: s(" must be true or false")?,
            must_be_a: s(" must be a ")?,
            at_least: s(" must be at least ")?,
            at_most: s(" must be at most ")?,
            characters: s(" characters")?,
            between: s(" must be between ")?,
            and: s(" and ")?,
            one_of: s(" must be one of: ")?,
            is_invalid: s(" is invalid")?,
            must_be_list: s(" must be a list")?,
            no_match: s(" does not match the required format")?,
            true_text: s("true")?,
            false_text: s("false")?,
            type_string: s("string")?,
            type_integer: s("integer")?,
            type_number: s("number")?,
            type_boolean: s("boolean")?,
            patterns,
            pattern_messages,
        })
    }
}

/// Function indices of the module functions the validator code calls.
#[derive(Debug, Clone, Copy)]
pub struct ValidatorImports {
    /// `__malloc(size) -> ptr` (MEMORY_MODEL.md "Allocation").
    pub malloc: u32,
    /// `__pairs_str_eq(a, b) -> 1 | 0`: byte equality of two strings.
    pub str_eq: u32,
    /// `__memcpy(dest, src, len)`.
    pub memcpy: u32,
    /// `__string_concat(a, b) -> string`.
    pub concat: u32,
    /// `__string_trim(s) -> string`.
    pub trim: u32,
    /// `list.allocate(capacity) -> list`.
    pub list_allocate: u32,
    /// `list.add(list, value) -> list` (the list may move).
    pub list_add: u32,
    /// `__int_to_string(i32) -> string`.
    pub int_to_string: u32,
    /// `__int64_to_string(i64) -> string`.
    pub int64_to_string: u32,
    /// `float_to_string(f64) -> string`.
    pub float_to_string: u32,
    /// `string_matches(ptr, len, pattern_id) -> 1 | 0`, registered when a
    /// `validator.match` rule is reachable.
    pub string_matches: Option<u32>,
    /// Type index of `(i32) -> i32`, the check function of
    /// `validator.custom`, when a `validator.custom` rule is reachable.
    pub custom_check_type: Option<u32>,
    /// The index the first validator helper function gets: the helpers are
    /// the next functions registered.
    pub first_helper: u32,
    pub strings: ValidatorStrings,
}

/// Indices of the validator's internal helper functions, registered first,
/// in this order, from `ValidatorImports::first_helper`.
#[derive(Debug, Clone, Copy)]
struct Helpers {
    /// `(cell, message)`: append `message` to the error list `cell` holds.
    add_error: u32,
    /// `(f64) -> string`: a whole number without a decimal point.
    number_text: u32,
    /// `(string, whole) -> f64`: the number the text spells, NaN if none.
    parse_number: u32,
    /// `(box, whole) -> f64`: the number a boxed value holds, NaN if none.
    numeric: u32,
    /// `(box) -> string | 0`: the text of a boxed scalar value.
    value_text: u32,
    /// `(pattern) -> id | -1`.
    pattern_id: u32,
    /// `(id) -> message`.
    pattern_message: u32,
    /// `(list<string>, separator) -> string`.
    join: u32,
    /// `(range) -> message`.
    range_message: u32,
    /// `(rules, name) -> index | -1`: the last field rule named `name`.
    field_index: u32,
    /// `(rules, object, prefix, cell) -> object`: check every field of
    /// `rules` against a JSON-tree object; returns the object with the
    /// changed (trimmed) values.
    check_object: u32,
    /// `(entry, box, label, cell) -> box`: check one value against a field
    /// rule; returns the (trimmed) value.
    check_value: u32,
}

impl Helpers {
    fn starting_at(first: u32) -> Self {
        Self {
            add_error: first,
            number_text: first + 1,
            parse_number: first + 2,
            numeric: first + 3,
            value_text: first + 4,
            pattern_id: first + 5,
            pattern_message: first + 6,
            join: first + 7,
            range_message: first + 8,
            field_index: first + 9,
            check_object: first + 10,
            check_value: first + 11,
        }
    }
}

impl ValidatorManager {
    /// Create a ValidatorManager that calls the functions in `imports`.
    pub fn new(imports: ValidatorImports) -> Self {
        Self {
            imports,
            helpers: Helpers::starting_at(imports.first_helper),
        }
    }

    /// Register all validator functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_helper_functions(codegen)?;
        self.register_validator_creation_functions(codegen)?;
        self.register_validation_execution_functions(codegen)?;
        self.register_validation_result_functions(codegen)?;
        self.register_validation_rule_functions(codegen)?;
        Ok(())
    }

    // ==================== Registration Groups ====================

    fn register_helper_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        use WasmType::{F64, I32};
        let h = self.helpers;
        let helpers: [(
            u32,
            &str,
            Vec<WasmType>,
            Option<WasmType>,
            Vec<WasmType>,
            Vec<Instruction<'static>>,
        ); 12] = [
            (
                h.add_error,
                "__validator_add_error",
                vec![I32, I32],
                None,
                vec![],
                self.generate_add_error(),
            ),
            (
                h.number_text,
                "__validator_number_text",
                vec![F64],
                Some(I32),
                vec![],
                self.generate_number_text(),
            ),
            (
                h.parse_number,
                "__validator_parse_number",
                vec![I32, I32],
                Some(F64),
                // 2 len, 3 i, 4 byte, 5 negative, 6 whole digits,
                // 7 fraction digits, 8 seen dot, 9 result, 10 scale
                vec![I32, I32, I32, I32, I32, I32, I32, F64, F64],
                Self::generate_parse_number(),
            ),
            (
                h.numeric,
                "__validator_numeric",
                vec![I32, I32],
                Some(F64),
                vec![I32, F64],
                self.generate_numeric(),
            ),
            (
                h.value_text,
                "__validator_value_text",
                vec![I32],
                Some(I32),
                vec![I32],
                self.generate_value_text(),
            ),
            (
                h.pattern_id,
                "__validator_pattern_id",
                vec![I32],
                Some(I32),
                vec![],
                self.generate_pattern_id(),
            ),
            (
                h.pattern_message,
                "__validator_pattern_message",
                vec![I32],
                Some(I32),
                vec![],
                self.generate_pattern_message(),
            ),
            (
                h.join,
                "__validator_join",
                vec![I32, I32],
                Some(I32),
                vec![I32, I32, I32],
                self.generate_join(),
            ),
            (
                h.range_message,
                "__validator_range_message",
                vec![I32],
                Some(I32),
                vec![],
                self.generate_range_message(),
            ),
            (
                h.field_index,
                "__validator_field_index",
                vec![I32, I32],
                Some(I32),
                vec![I32],
                self.generate_field_index(),
            ),
            (
                h.check_object,
                "__validator_check_object",
                vec![I32, I32, I32, I32],
                Some(I32),
                vec![I32; CHECK_FIELDS_LOCALS],
                self.generate_check_object(),
            ),
            (
                h.check_value,
                "__validator_check_value",
                vec![I32, I32, I32, I32],
                Some(I32),
                // 4..=15 i32 (see generate_check_value), 16 f64
                vec![
                    I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, I32, F64,
                ],
                self.generate_check_value(),
            ),
        ];
        for (expected, name, params, result, locals, body) in helpers {
            let index = register_stdlib_function_with_locals(
                codegen, name, &params, result, &locals, body,
            )?;
            if index != expected {
                return Err(CompilerError::codegen_error(
                    format!("validator helper {name} got index {index}, expected {expected}"),
                    None,
                    None,
                ));
            }
        }
        Ok(())
    }

    fn register_validator_creation_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // validator.create() -> ValidationRules
        register_stdlib_function_with_locals(
            codegen,
            "validator.create",
            &[],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32], // locals: rules_ptr, fields_ptr
            self.generate_validator_create(),
        )?;

        // validator.createWithName(name_ptr: i32) -> ValidationRules
        register_stdlib_function_with_locals(
            codegen,
            "validator.createWithName",
            &[WasmType::I32],                // param: name_ptr
            Some(WasmType::I32),             // returns rules_ptr
            &[WasmType::I32, WasmType::I32], // locals: rules_ptr, fields_ptr
            self.generate_validator_create_with_name(),
        )?;

        Ok(())
    }

    fn register_validation_execution_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // validator.run(rules_ptr, input: any) -> ValidationResult ptr
        let run = register_stdlib_function_with_locals(
            codegen,
            "validator.run",
            &[WasmType::I32, WasmType::I32], // rules_ptr=0, input_ptr=1
            Some(WasmType::I32),
            &[WasmType::I32; RUN_EXTRA_LOCALS],
            self.generate_validator_run(),
        )?;
        // validator.validate is a synonym of validator.run (§15).
        codegen.add_function_alias("validator.validate", run);

        // validator.runField(rules_ptr, field_name_ptr, value: any) -> ValidationResult ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.runField",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // rules_ptr=0, field_name_ptr=1, value=2
            Some(WasmType::I32),
            &[WasmType::I32; RUN_FIELD_EXTRA_LOCALS],
            self.generate_validator_run_field(),
        )?;

        Ok(())
    }

    fn register_validation_result_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // validator.ok(value_ptr: i32) -> ValidationResult ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.ok",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: result_ptr
            self.generate_validator_ok(),
        )?;

        // validator.error(errors_list_ptr: i32) -> ValidationResult ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.error",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: result_ptr
            self.generate_validator_error(),
        )?;

        // validator.isOk(result_ptr: i32) -> boolean
        register_stdlib_function(
            codegen,
            "validator.isOk",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_validator_is_ok(),
        )?;

        // validator.isError(result_ptr: i32) -> boolean
        register_stdlib_function(
            codegen,
            "validator.isError",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_validator_is_error(),
        )?;

        // validator.getValue(result_ptr: i32) -> value_ptr
        register_stdlib_function(
            codegen,
            "validator.getValue",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_validator_get_value(),
        )?;

        // validator.getErrors(result_ptr: i32) -> list_ptr
        register_stdlib_function(
            codegen,
            "validator.getErrors",
            &[WasmType::I32],
            Some(WasmType::I32),
            self.generate_validator_get_errors(),
        )?;

        // validator.getFirstError(result_ptr: i32) -> string_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.getFirstError",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: errors list
            self.generate_validator_get_first_error(),
        )?;

        // validator.fieldErrors(result_ptr: i32, field_name_ptr: i32) -> list_ptr
        register_stdlib_function(
            codegen,
            "validator.fieldErrors",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            self.generate_validator_field_errors(),
        )?;

        Ok(())
    }

    fn register_validation_rule_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // validator.field(rules_ptr, field_name_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.field",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            // locals: field_count, field_entry_ptr, capacity, new_fields_ptr
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            self.generate_validator_field(),
        )?;

        // validator.required(rules_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.required",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_required(),
        )?;

        // validator.optional(rules_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.optional",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_optional(),
        )?;

        // validator.match(rules_ptr, pattern_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.match",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_match(),
        )?;

        // validator.range(rules_ptr, lo: number, hi: number) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.range",
            &[WasmType::I32, WasmType::F64, WasmType::F64],
            Some(WasmType::I32),
            &[WasmType::I32, WasmType::I32], // locals: range_rule_ptr, field_entry_ptr
            self.generate_validator_range(),
        )?;

        // validator.minLength(rules_ptr, min) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.minLength",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_min_length(),
        )?;

        // validator.maxLength(rules_ptr, max) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.maxLength",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_max_length(),
        )?;

        // validator.message(rules_ptr, message_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.message",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_message(),
        )?;

        // validator.custom(rules_ptr, fn_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.custom",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_custom(),
        )?;

        // validator.type(rules_ptr, type_name_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.type",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_type(),
        )?;

        // validator.trim(rules_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.trim",
            &[WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_trim(),
        )?;

        // validator.allowedValues(rules_ptr, values_list_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.allowedValues",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_allowed_values(),
        )?;

        // validator.default(rules_ptr, default_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.default",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_default(),
        )?;

        // validator.requiredIf(rules_ptr, field_name_ptr, expected_value_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.requiredIf",
            &[WasmType::I32, WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_required_if(),
        )?;

        // validator.shape(rules_ptr, shape_rules_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.shape",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_shape(),
        )?;

        // validator.each(rules_ptr, each_rules_ptr) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.each",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_each(),
        )?;

        // validator.minValue(rules_ptr, min_i32) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.minValue",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_min_value(),
        )?;

        // validator.maxValue(rules_ptr, max_i32) -> rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.maxValue",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32], // local: field_entry_ptr
            self.generate_validator_max_value(),
        )?;

        // validator.merge(rules_a_ptr, rules_b_ptr) -> new_rules_ptr
        register_stdlib_function_with_locals(
            codegen,
            "validator.merge",
            &[WasmType::I32, WasmType::I32],
            Some(WasmType::I32),
            &[WasmType::I32; MERGE_EXTRA_LOCALS],
            self.generate_validator_merge(),
        )?;

        Ok(())
    }

    // ==================== Shared Helper: get last field entry ptr ====================

    /// Emits instructions that compute the address of the last field entry in `rules_ptr`
    /// and stores it into a local variable at `dest_local`.
    ///
    /// Stack before: empty (uses LocalGet internally)
    /// Result: dest_local holds the address of the last (most recently added) field entry.
    ///
    /// Assumes rules layout:
    ///   offset 0: field_count
    ///   offset 8: (rules_ptr + 8) holds a word whose value is fields_ptr
    fn emit_get_last_field_entry(rules_local: u32, dest_local: u32) -> Vec<Instruction<'static>> {
        // field_entry_ptr = fields_ptr + (field_count - 1) * FIELD_ENTRY_SIZE
        vec![
            // Compute (field_count - 1) * FIELD_ENTRY_SIZE
            Instruction::LocalGet(rules_local),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Const(FIELD_ENTRY_SIZE),
            Instruction::I32Mul,
            // Load fields_ptr from rules at offset 8
            Instruction::LocalGet(rules_local),
            Instruction::I32Load(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // fields_ptr + offset = field_entry_ptr
            Instruction::I32Add,
            Instruction::LocalSet(dest_local),
        ]
    }

    // ==================== Code Generation: Creation ====================

    /// `validator.create() -> ValidationRules`
    ///
    /// Allocates a 16-byte ValidationRules header and an 8-slot field array (8 * 84 = 672 bytes).
    /// Locals: 0=rules_ptr (extra 0), 1=fields_ptr (extra 1)
    /// (params: none, so extra locals start at index 0)
    fn generate_validator_create(&self) -> Vec<Instruction<'static>> {
        // No params, so additional locals are 0 and 1
        vec![
            // Allocate ValidationRules header (16 bytes): field_count, capacity, fields_ptr, name_ptr
            Instruction::I32Const(16),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalTee(0), // rules_ptr
            // field_count = 0 at offset 0
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // capacity = INITIAL_FIELD_CAPACITY at offset 4
            Instruction::LocalGet(0),
            Instruction::I32Const(INITIAL_FIELD_CAPACITY),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // name_ptr = 0 at offset 12 (unnamed)
            Instruction::LocalGet(0),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            // Allocate field array: INITIAL_FIELD_CAPACITY * FIELD_ENTRY_SIZE bytes
            Instruction::I32Const(INITIAL_FIELD_CAPACITY * FIELD_ENTRY_SIZE),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalSet(1), // fields_ptr — use Set (not Tee) to avoid leaving value on stack
            // Store fields_ptr at offset 8 of rules header
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr
            Instruction::LocalGet(0),
        ]
    }

    /// `validator.createWithName(name_ptr: i32) -> ValidationRules`
    ///
    /// Identical to create() but stores name_ptr at offset 12 of the header.
    /// Params: 0=name_ptr
    /// Extra locals: 1=rules_ptr, 2=fields_ptr
    fn generate_validator_create_with_name(&self) -> Vec<Instruction<'static>> {
        vec![
            // Allocate 16-byte header
            Instruction::I32Const(16),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalTee(1), // rules_ptr (extra local 1, since param is 0)
            // field_count = 0
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // capacity = INITIAL_FIELD_CAPACITY
            Instruction::LocalGet(1),
            Instruction::I32Const(INITIAL_FIELD_CAPACITY),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // name_ptr at offset 12
            Instruction::LocalGet(1),
            Instruction::LocalGet(0), // name_ptr param
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            // Allocate field array
            Instruction::I32Const(INITIAL_FIELD_CAPACITY * FIELD_ENTRY_SIZE),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalSet(2), // fields_ptr (extra local 2) — use Set (not Tee) to avoid leaving value on stack
            // Store fields_ptr at offset 8
            Instruction::LocalGet(1),
            Instruction::LocalGet(2),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr
            Instruction::LocalGet(1),
        ]
    }

    // ==================== Code Generation: Execution ====================

    /// Store a new boxed `any` with `tag` and the i32 payload in
    /// `payload_local` into `box_local`.
    fn emit_new_box(
        &self,
        tag: i32,
        payload_local: u32,
        box_local: u32,
    ) -> Vec<Instruction<'static>> {
        vec![
            Instruction::I32Const(ANY_BOX_SIZE),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalSet(box_local),
            Instruction::LocalGet(box_local),
            Instruction::I32Const(tag),
            store(0),
            Instruction::LocalGet(box_local),
            Instruction::LocalGet(payload_local),
            store(4),
            Instruction::LocalGet(box_local),
            Instruction::I32Const(0),
            store(8),
        ]
    }

    /// A new empty error list, held in a 4-byte cell (the list moves when
    /// it grows): `cell_local` = the cell.
    fn emit_new_error_cell(&self, cell_local: u32) -> Vec<Instruction<'static>> {
        vec![
            Instruction::I32Const(4),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalTee(cell_local),
            Instruction::I32Const(4),
            Instruction::Call(self.imports.list_allocate),
            store(0),
        ]
    }

    /// The loop of `validator.run` and the nested-object check: every field
    /// rule of `rules` is checked against the entry of the JSON-tree object
    /// (BOXED_ANY_ABI.md §3.3, `[count][key_ptr, value_box]...`; 0 = no
    /// entries) with the field's name, absent when there is none. A value
    /// the check changes (trimmed) goes into a copy of the object, left in
    /// `copy` (0 when nothing changed).
    fn emit_check_fields(&self, l: FieldLoop) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let mut v = vec![
            I::LocalGet(l.rules),
            load(0),
            I::LocalSet(l.count),
            I::LocalGet(l.object),
            if_i32(),
            I::LocalGet(l.object),
            load(0),
            I::Else,
            I::I32Const(0),
            I::End,
            I::LocalSet(l.object_count),
            I::I32Const(0),
            I::LocalSet(l.copy),
            I::I32Const(0),
            I::LocalSet(l.index),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(l.index),
            I::LocalGet(l.count),
            I::I32GeS,
            I::BrIf(1),
            // entry = fields + index * FIELD_ENTRY_SIZE
            I::LocalGet(l.rules),
            load(8),
            I::LocalGet(l.index),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::I32Mul,
            I::I32Add,
            I::LocalTee(l.entry),
            load(0),
            I::LocalSet(l.name),
            // value = the value of the object entry named `name` (0 if none);
            // entry_index = its index, object_count when there is none
            I::I32Const(0),
            I::LocalSet(l.value),
            I::I32Const(0),
            I::LocalSet(l.entry_index),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(l.entry_index),
            I::LocalGet(l.object_count),
            I::I32GeS,
            I::BrIf(1),
            I::LocalGet(l.object),
            I::LocalGet(l.entry_index),
            I::I32Const(8),
            I::I32Mul,
            I::I32Add,
            load(4),
            I::LocalGet(l.name),
            I::Call(self.imports.str_eq),
            if_empty(),
            I::LocalGet(l.object),
            I::LocalGet(l.entry_index),
            I::I32Const(8),
            I::I32Mul,
            I::I32Add,
            load(8),
            I::LocalSet(l.value),
            I::Br(2),
            I::End,
            I::LocalGet(l.entry_index),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(l.entry_index),
            I::Br(0),
            I::End,
            I::End,
        ];
        // The errors of a nested field are named `prefix` + field name.
        match l.prefix {
            Some(prefix) => v.extend([
                I::LocalGet(prefix),
                if_i32(),
                I::LocalGet(prefix),
                I::LocalGet(l.name),
                I::Call(self.imports.concat),
                I::Else,
                I::LocalGet(l.name),
                I::End,
                I::LocalSet(l.label),
            ]),
            None => v.extend([I::LocalGet(l.name), I::LocalSet(l.label)]),
        }
        v.extend([
            I::LocalGet(l.entry),
            I::LocalGet(l.value),
            I::LocalGet(l.label),
            I::LocalGet(l.cell),
            I::Call(self.helpers.check_value),
            I::LocalSet(l.checked),
            // A changed value replaces the entry in a copy of the object.
            I::LocalGet(l.checked),
            I::LocalGet(l.value),
            I::I32Ne,
            I::LocalGet(l.entry_index),
            I::LocalGet(l.object_count),
            I::I32LtS,
            I::I32And,
            if_empty(),
            I::LocalGet(l.copy),
            I::I32Eqz,
            if_empty(),
            I::LocalGet(l.object_count),
            I::I32Const(8),
            I::I32Mul,
            I::I32Const(4),
            I::I32Add,
            I::Call(self.imports.malloc),
            I::LocalSet(l.copy),
            I::LocalGet(l.copy),
            I::LocalGet(l.object),
            I::LocalGet(l.object_count),
            I::I32Const(8),
            I::I32Mul,
            I::I32Const(4),
            I::I32Add,
            I::Call(self.imports.memcpy),
            I::End,
            I::LocalGet(l.copy),
            I::LocalGet(l.entry_index),
            I::I32Const(8),
            I::I32Mul,
            I::I32Add,
            I::LocalGet(l.checked),
            store(8),
            I::End,
            I::LocalGet(l.index),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(l.index),
            I::Br(0),
            I::End,
            I::End,
        ]);
        v
    }

    /// Fill the 24-byte ValidationResult in `result_local`: ok when the
    /// error list in `cell_local` is empty, then holding the value
    /// `value` leaves on the stack.
    fn emit_result(
        &self,
        result_local: u32,
        errors_local: u32,
        cell_local: u32,
        value: Vec<Instruction<'static>>,
        field_name: Instruction<'static>,
    ) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let mut v = vec![
            I::I32Const(RESULT_SIZE),
            I::Call(self.imports.malloc),
            I::LocalSet(result_local),
            I::LocalGet(cell_local),
            load(0),
            I::LocalSet(errors_local),
            // is_ok = no errors
            I::LocalGet(result_local),
            I::LocalGet(errors_local),
            load(0),
            I::I32Eqz,
            store(0),
            // value when ok, 0 otherwise
            I::LocalGet(result_local),
            I::LocalGet(errors_local),
            load(0),
            I::I32Eqz,
            if_i32(),
        ];
        v.extend(value);
        v.extend([
            I::Else,
            I::I32Const(0),
            I::End,
            store(4),
            I::LocalGet(result_local),
            I::LocalGet(errors_local),
            store(8),
            I::LocalGet(result_local),
            I::I32Const(0),
            store(12),
            I::LocalGet(result_local),
            field_name,
            store(16),
            // validator_name_ptr from the rules header
            I::LocalGet(result_local),
            I::LocalGet(0),
            load(12),
            store(20),
            I::LocalGet(result_local),
        ]);
        v
    }

    /// `validator.run(rules_ptr, input) -> ValidationResult`
    ///
    /// Checks every field rule against `input`, an `any` (BOXED_ANY_ABI.md):
    /// a `pairs` value arrives as a tag-6 box holding a JSON-tree object, and
    /// each field's value is the entry whose key is the field's name. A field
    /// is absent when there is no such entry or the input is not an object.
    /// The result holds the errors in order and, when there are none, the
    /// input with the trimmed values (stdlib-reference.md §15).
    ///
    /// Params: 0=rules_ptr, 1=input; locals: see `RUN_EXTRA_LOCALS`.
    fn generate_validator_run(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let l = FieldLoop {
            rules: 0,
            object: 3,
            prefix: None,
            cell: 2,
            count: 4,
            index: 5,
            entry: 6,
            name: 7,
            object_count: 8,
            entry_index: 9,
            value: 10,
            checked: 11,
            copy: 12,
            label: 13,
        };
        let (result, errors, value_box) = (14, 15, 16);
        let mut v = self.emit_new_error_cell(l.cell);
        // object = the JSON-tree object a tag-6 input holds, else 0
        v.extend([
            I::I32Const(0),
            I::LocalSet(l.object),
            I::LocalGet(1),
            if_empty(),
            I::LocalGet(1),
            load(0),
            I::I32Const(ANY_TAG_OBJECT),
            I::I32Eq,
            if_empty(),
            I::LocalGet(1),
            load(4),
            I::LocalSet(l.object),
            I::End,
            I::End,
        ]);
        v.extend(self.emit_check_fields(l));
        // The value: the input, or a new box around the changed copy.
        let mut value = vec![I::LocalGet(l.copy), if_i32()];
        value.extend(self.emit_new_box(ANY_TAG_OBJECT, l.copy, value_box));
        value.extend([I::LocalGet(value_box), I::Else, I::LocalGet(1), I::End]);
        v.extend(self.emit_result(result, errors, l.cell, value, I::I32Const(0)));
        v
    }

    /// `validator.runField(rules_ptr, field_name_ptr, value) -> ValidationResult`
    ///
    /// Checks `value`, a boxed `any`, against the field rule named
    /// `field_name_ptr` (the last one with that name); with no such rule
    /// there is nothing to check. The result records the field name at
    /// offset 16.
    ///
    /// Params: 0=rules_ptr, 1=field_name_ptr, 2=value; locals: see
    /// `RUN_FIELD_EXTRA_LOCALS`.
    fn generate_validator_run_field(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let (cell, index, entry, checked, result, errors, fields) = (3, 4, 5, 6, 7, 8, 9);
        let mut v = self.emit_new_error_cell(cell);
        v.extend([
            I::I32Const(0),
            I::LocalSet(entry),
            I::LocalGet(0),
            load(8),
            I::LocalSet(fields),
            I::LocalGet(0),
            load(0),
            I::LocalSet(index),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(index),
            I::I32Eqz,
            I::BrIf(1),
            I::LocalGet(index),
            I::I32Const(1),
            I::I32Sub,
            I::LocalSet(index),
            I::LocalGet(fields),
            I::LocalGet(index),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::I32Mul,
            I::I32Add,
            load(0),
            I::LocalGet(1),
            I::Call(self.imports.str_eq),
            if_empty(),
            I::LocalGet(fields),
            I::LocalGet(index),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::I32Mul,
            I::I32Add,
            I::LocalSet(entry),
            I::Br(2),
            I::End,
            I::Br(0),
            I::End,
            I::End,
            I::LocalGet(entry),
            if_i32(),
            I::LocalGet(entry),
            I::LocalGet(2),
            I::LocalGet(1),
            I::LocalGet(cell),
            I::Call(self.helpers.check_value),
            I::Else,
            I::LocalGet(2),
            I::End,
            I::LocalSet(checked),
        ]);
        v.extend(self.emit_result(
            result,
            errors,
            cell,
            vec![I::LocalGet(checked)],
            I::LocalGet(1),
        ));
        v
    }

    // ==================== Code Generation: Helpers ====================

    /// `__validator_add_error(cell, message)`.
    fn generate_add_error(&self) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(0),
            load(0),
            Instruction::LocalGet(1),
            Instruction::Call(self.imports.list_add),
            store(0),
        ]
    }

    /// `__validator_number_text(x: f64) -> string`: a whole number in the
    /// exact integer range prints without a decimal point.
    fn generate_number_text(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        vec![
            I::LocalGet(0),
            I::LocalGet(0),
            I::F64Trunc,
            I::F64Eq,
            I::LocalGet(0),
            I::F64Abs,
            I::F64Const(9.0e15),
            I::F64Lt,
            I::I32And,
            if_i32(),
            I::LocalGet(0),
            I::I64TruncF64S,
            I::Call(self.imports.int64_to_string),
            I::Else,
            I::LocalGet(0),
            I::Call(self.imports.float_to_string),
            I::End,
        ]
    }

    /// `__validator_parse_number(text, whole) -> f64`: the number `text`
    /// spells — an optional sign, digits and, unless `whole`, an optional
    /// `.` followed by digits — or NaN.
    ///
    /// Locals: 2 len, 3 i, 4 byte, 5 negative, 6 whole digits,
    /// 7 fraction digits, 8 seen dot, 9 result (f64), 10 scale (f64).
    fn generate_parse_number() -> Vec<Instruction<'static>> {
        use Instruction as I;
        let byte = MemArg {
            offset: 4,
            align: 0,
            memory_index: 0,
        };
        let invalid = || [I::F64Const(f64::NAN), I::Return, I::End];
        let mut v = vec![
            I::LocalGet(0),
            load(0),
            I::LocalSet(2),
            I::I32Const(0),
            I::LocalSet(3),
            I::I32Const(0),
            I::LocalSet(5),
            I::I32Const(0),
            I::LocalSet(6),
            I::I32Const(0),
            I::LocalSet(7),
            I::I32Const(0),
            I::LocalSet(8),
            I::F64Const(0.0),
            I::LocalSet(9),
            I::F64Const(1.0),
            I::LocalSet(10),
            // sign
            I::LocalGet(2),
            if_empty(),
            I::LocalGet(0),
            I::I32Load8U(byte),
            I::LocalTee(4),
            I::I32Const(b'-' as i32),
            I::I32Eq,
            if_empty(),
            I::I32Const(1),
            I::LocalSet(5),
            I::I32Const(1),
            I::LocalSet(3),
            I::End,
            I::LocalGet(4),
            I::I32Const(b'+' as i32),
            I::I32Eq,
            if_empty(),
            I::I32Const(1),
            I::LocalSet(3),
            I::End,
            I::End,
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(3),
            I::LocalGet(2),
            I::I32GeS,
            I::BrIf(1),
            I::LocalGet(0),
            I::LocalGet(3),
            I::I32Add,
            I::I32Load8U(byte),
            I::LocalSet(4),
            I::LocalGet(4),
            I::I32Const(b'.' as i32),
            I::I32Eq,
            if_empty(),
            // a second dot, or any dot in a whole number
            I::LocalGet(1),
            I::LocalGet(8),
            I::I32Or,
            if_empty(),
        ];
        v.extend(invalid());
        v.extend([
            I::I32Const(1),
            I::LocalSet(8),
            I::Else,
            I::LocalGet(4),
            I::I32Const(b'0' as i32),
            I::I32Sub,
            I::LocalTee(4),
            I::I32Const(10),
            I::I32GeU,
            if_empty(),
        ]);
        v.extend(invalid());
        v.extend([
            I::LocalGet(8),
            if_empty(),
            I::LocalGet(10),
            I::F64Const(10.0),
            I::F64Div,
            I::LocalSet(10),
            I::LocalGet(9),
            I::LocalGet(4),
            I::F64ConvertI32U,
            I::LocalGet(10),
            I::F64Mul,
            I::F64Add,
            I::LocalSet(9),
            I::LocalGet(7),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(7),
            I::Else,
            I::LocalGet(9),
            I::F64Const(10.0),
            I::F64Mul,
            I::LocalGet(4),
            I::F64ConvertI32U,
            I::F64Add,
            I::LocalSet(9),
            I::LocalGet(6),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(6),
            I::End,
            I::End,
            I::LocalGet(3),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(3),
            I::Br(0),
            I::End,
            I::End,
            // no digit before the dot, or none after it
            I::LocalGet(6),
            I::I32Eqz,
            I::LocalGet(8),
            I::LocalGet(7),
            I::I32Eqz,
            I::I32And,
            I::I32Or,
            if_empty(),
        ]);
        v.extend(invalid());
        v.extend([
            I::LocalGet(9),
            I::F64Neg,
            I::LocalGet(9),
            I::LocalGet(5),
            I::Select,
        ]);
        v
    }

    /// `__validator_numeric(box, whole) -> f64`: the number an integer or
    /// number box holds, or that a string box spells; NaN for any other
    /// value, and with `whole` for a number with a fraction.
    ///
    /// Locals: 2 tag, 3 x (f64).
    fn generate_numeric(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        vec![
            I::LocalGet(0),
            I::I32Eqz,
            if_empty(),
            I::F64Const(f64::NAN),
            I::Return,
            I::End,
            I::LocalGet(0),
            load(0),
            I::LocalSet(2),
            I::LocalGet(2),
            I::I32Const(ANY_TAG_INTEGER),
            I::I32Eq,
            if_empty(),
            I::LocalGet(0),
            I::I64Load(word(4)),
            I::F64ConvertI64S,
            I::Return,
            I::End,
            I::LocalGet(2),
            I::I32Const(ANY_TAG_NUMBER),
            I::I32Eq,
            if_empty(),
            I::LocalGet(0),
            I::F64Load(word(4)),
            I::LocalSet(3),
            I::LocalGet(1),
            I::LocalGet(3),
            I::LocalGet(3),
            I::F64Trunc,
            I::F64Ne,
            I::I32And,
            if_empty(),
            I::F64Const(f64::NAN),
            I::Return,
            I::End,
            I::LocalGet(3),
            I::Return,
            I::End,
            I::LocalGet(2),
            I::I32Const(ANY_TAG_STRING),
            I::I32Eq,
            if_empty(),
            I::LocalGet(0),
            load(4),
            I::LocalGet(1),
            I::Call(self.helpers.parse_number),
            I::Return,
            I::End,
            I::F64Const(f64::NAN),
        ]
    }

    /// `__validator_value_text(box) -> string | 0`: the text of a string,
    /// integer, boolean or number box; 0 for any other value.
    ///
    /// Locals: 1 tag.
    fn generate_value_text(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let s = self.imports.strings;
        vec![
            I::LocalGet(0),
            I::I32Eqz,
            if_empty(),
            I::I32Const(0),
            I::Return,
            I::End,
            I::LocalGet(0),
            load(0),
            I::LocalSet(1),
            I::LocalGet(1),
            I::I32Const(ANY_TAG_STRING),
            I::I32Eq,
            if_empty(),
            I::LocalGet(0),
            load(4),
            I::Return,
            I::End,
            I::LocalGet(1),
            I::I32Const(ANY_TAG_INTEGER),
            I::I32Eq,
            if_empty(),
            I::LocalGet(0),
            I::I64Load(word(4)),
            I::Call(self.imports.int64_to_string),
            I::Return,
            I::End,
            I::LocalGet(1),
            I::I32Const(ANY_TAG_BOOLEAN),
            I::I32Eq,
            if_empty(),
            I::LocalGet(0),
            load(4),
            if_i32(),
            text(s.true_text),
            I::Else,
            text(s.false_text),
            I::End,
            I::Return,
            I::End,
            I::LocalGet(1),
            I::I32Const(ANY_TAG_NUMBER),
            I::I32Eq,
            if_empty(),
            I::LocalGet(0),
            I::F64Load(word(4)),
            I::Call(self.helpers.number_text),
            I::Return,
            I::End,
            I::I32Const(0),
        ]
    }

    /// `__validator_pattern_id(pattern) -> id | -1`.
    fn generate_pattern_id(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let mut v = Vec::new();
        for (id, name) in self.imports.strings.patterns.iter().enumerate() {
            v.extend([
                I::LocalGet(0),
                text(*name),
                I::Call(self.imports.str_eq),
                if_empty(),
                I::I32Const(id as i32),
                I::Return,
                I::End,
            ]);
        }
        v.push(I::I32Const(-1));
        v
    }

    /// `__validator_pattern_message(id) -> message`.
    fn generate_pattern_message(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let s = self.imports.strings;
        let mut v = Vec::new();
        for (id, message) in s.pattern_messages.iter().enumerate() {
            v.extend([
                I::LocalGet(0),
                I::I32Const(id as i32),
                I::I32Eq,
                if_empty(),
                text(*message),
                I::Return,
                I::End,
            ]);
        }
        v.push(text(s.no_match));
        v
    }

    /// `__validator_join(list<string>, separator) -> string`.
    ///
    /// Locals: 2 result, 3 i, 4 n.
    fn generate_join(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        vec![
            text(self.imports.strings.empty),
            I::LocalSet(2),
            I::LocalGet(0),
            load(0),
            I::LocalSet(4),
            I::I32Const(0),
            I::LocalSet(3),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(3),
            I::LocalGet(4),
            I::I32GeS,
            I::BrIf(1),
            I::LocalGet(3),
            if_empty(),
            I::LocalGet(2),
            I::LocalGet(1),
            I::Call(self.imports.concat),
            I::LocalSet(2),
            I::End,
            I::LocalGet(2),
            I::LocalGet(0),
            I::LocalGet(3),
            I::I32Const(4),
            I::I32Mul,
            I::I32Add,
            load(LIST_DATA_OFFSET),
            I::Call(self.imports.concat),
            I::LocalSet(2),
            I::LocalGet(3),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(3),
            I::Br(0),
            I::End,
            I::End,
            I::LocalGet(2),
        ]
    }

    /// `__validator_range_message(range) -> message`: " must be at most
    /// hi", " must be at least lo" or " must be between lo and hi".
    fn generate_range_message(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let s = self.imports.strings;
        let bound = |offset: u64| {
            [
                I::LocalGet(0),
                I::F64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }),
                I::Call(self.helpers.number_text),
            ]
        };
        let mut v = vec![I::LocalGet(0)];
        v.extend([
            I::F64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            I::F64Const(f64::NEG_INFINITY),
            I::F64Eq,
            if_i32(),
            text(s.at_most),
        ]);
        v.extend(bound(8));
        v.extend([
            I::Call(self.imports.concat),
            I::Else,
            I::LocalGet(0),
            I::F64Load(MemArg {
                offset: 8,
                align: 3,
                memory_index: 0,
            }),
            I::F64Const(f64::INFINITY),
            I::F64Eq,
            if_i32(),
            text(s.at_least),
        ]);
        v.extend(bound(0));
        v.extend([I::Call(self.imports.concat), I::Else, text(s.between)]);
        v.extend(bound(0));
        v.extend([
            I::Call(self.imports.concat),
            text(s.and),
            I::Call(self.imports.concat),
        ]);
        v.extend(bound(8));
        v.extend([I::Call(self.imports.concat), I::End, I::End]);
        v
    }

    /// `__validator_field_index(rules, name) -> index | -1`: the last field
    /// rule named `name`.
    ///
    /// Locals: 2 i.
    fn generate_field_index(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        vec![
            I::LocalGet(0),
            load(0),
            I::LocalSet(2),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(2),
            I::I32Eqz,
            I::BrIf(1),
            I::LocalGet(2),
            I::I32Const(1),
            I::I32Sub,
            I::LocalSet(2),
            I::LocalGet(0),
            load(8),
            I::LocalGet(2),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::I32Mul,
            I::I32Add,
            load(0),
            I::LocalGet(1),
            I::Call(self.imports.str_eq),
            if_empty(),
            I::LocalGet(2),
            I::Return,
            I::End,
            I::Br(0),
            I::End,
            I::End,
            I::I32Const(-1),
        ]
    }

    /// `__validator_check_object(rules, object, prefix, cell) -> object`:
    /// the field loop of `validator.run` for a nested object (`shape`,
    /// `each`); the errors are named `prefix` + field name.
    fn generate_check_object(&self) -> Vec<Instruction<'static>> {
        let l = FieldLoop {
            rules: 0,
            object: 1,
            prefix: Some(2),
            cell: 3,
            count: 4,
            index: 5,
            entry: 6,
            name: 7,
            object_count: 8,
            entry_index: 9,
            value: 10,
            checked: 11,
            copy: 12,
            label: 13,
        };
        let mut v = self.emit_check_fields(l);
        v.extend([
            Instruction::LocalGet(l.copy),
            if_i32(),
            Instruction::LocalGet(l.copy),
            Instruction::Else,
            Instruction::LocalGet(l.object),
            Instruction::End,
        ]);
        v
    }

    /// `__validator_check_value(entry, box, label, cell) -> box`: check one
    /// value (a boxed `any`, 0 when absent) against a field rule, appending
    /// `label` + message for each failed rule, and return the value — the
    /// trimmed one for a `trim` rule. The rules run in the order of
    /// stdlib-reference.md §15: `trim`, then `required` (an absent value,
    /// null or empty text fails it and skips the others; an optional one
    /// passes), `type` (when it fails the others are skipped), the lengths,
    /// `range`, `match`, the allowed values, `custom`, `shape` and `each`.
    /// A field `message` replaces every error of the field.
    ///
    /// Locals: 4 errors before, 5 tag, 6 text of a string value (0 if not
    /// a string), 7 scratch, 8 ok, 9 rule, 10 i, 11 count, 12 copy,
    /// 13 list / text, 14 object, 15 checked object, 16 x (f64).
    fn generate_check_value(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let s = self.imports.strings;
        let concat = I::Call(self.imports.concat);
        let add_error = I::Call(self.helpers.add_error);
        // `label` + the message on the stack, appended to the errors.
        let report = |message: Vec<Instruction<'static>>| {
            let mut v = vec![I::LocalGet(3), I::LocalGet(2)];
            v.extend(message);
            v.extend([
                I::Call(self.imports.concat),
                I::Call(self.helpers.add_error),
            ]);
            v
        };
        let f64_at = |offset: u64| {
            I::F64Load(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            })
        };

        let mut v = vec![
            // errors before this field
            I::LocalGet(3),
            load(0),
            load(0),
            I::LocalSet(4),
            // tag (0 when absent)
            I::LocalGet(1),
            if_i32(),
            I::LocalGet(1),
            load(0),
            I::Else,
            I::I32Const(0),
            I::End,
            I::LocalSet(5),
            // trim: the trimmed text replaces the value
            I::LocalGet(0),
            load(36),
            I::LocalGet(5),
            I::I32Const(ANY_TAG_STRING),
            I::I32Eq,
            I::I32And,
            if_empty(),
            I::LocalGet(1),
            load(4),
            I::Call(self.imports.trim),
            I::LocalSet(6),
        ];
        v.extend(self.emit_new_box(ANY_TAG_STRING, 6, 1));
        v.extend([
            I::End,
            I::LocalGet(5),
            I::I32Const(ANY_TAG_STRING),
            I::I32Eq,
            if_i32(),
            I::LocalGet(1),
            load(4),
            I::Else,
            I::I32Const(0),
            I::End,
            I::LocalSet(6),
            // block 0: the rules; a failed required or type rule leaves it
            I::Block(BlockType::Empty),
            // absent: no value, null, or empty text
            I::LocalGet(5),
            I::I32Eqz,
            I::LocalGet(6),
            if_i32(),
            I::LocalGet(6),
            load(0),
            I::I32Eqz,
            I::Else,
            I::I32Const(0),
            I::End,
            I::I32Or,
            if_empty(),
            I::LocalGet(0),
            load(4),
            if_empty(),
        ]);
        v.extend(report(vec![text(s.is_required)]));
        v.extend([I::End, I::Br(1), I::End]);

        // type, checked first
        v.extend([I::LocalGet(0), load(32), I::LocalTee(9), if_empty()]);
        let is_type = |name: u32| vec![I::LocalGet(9), text(name), I::Call(self.imports.str_eq)];
        let numeric_ok = |whole: i32| {
            vec![
                I::LocalGet(1),
                I::I32Const(whole),
                I::Call(self.helpers.numeric),
                I::LocalTee(16),
                I::LocalGet(16),
                I::F64Eq,
                I::LocalSet(8),
            ]
        };
        v.extend(is_type(s.type_string));
        v.extend([
            if_empty(),
            I::LocalGet(5),
            I::I32Const(ANY_TAG_STRING),
            I::I32Eq,
            I::LocalSet(8),
            text(s.must_be_string),
            I::LocalSet(7),
            I::Else,
        ]);
        v.extend(is_type(s.type_integer));
        v.push(if_empty());
        v.extend(numeric_ok(1));
        v.extend([text(s.must_be_whole), I::LocalSet(7), I::Else]);
        v.extend(is_type(s.type_number));
        v.push(if_empty());
        v.extend(numeric_ok(0));
        v.extend([text(s.must_be_number), I::LocalSet(7), I::Else]);
        v.extend(is_type(s.type_boolean));
        v.extend([
            if_empty(),
            I::LocalGet(5),
            I::I32Const(ANY_TAG_BOOLEAN),
            I::I32Eq,
            I::LocalGet(6),
            if_i32(),
            I::LocalGet(6),
            text(s.true_text),
            I::Call(self.imports.str_eq),
            I::LocalGet(6),
            text(s.false_text),
            I::Call(self.imports.str_eq),
            I::I32Or,
            I::Else,
            I::I32Const(0),
            I::End,
            I::I32Or,
            I::LocalSet(8),
            text(s.must_be_boolean),
            I::LocalSet(7),
            I::Else,
            // an unknown type name: no value has it
            I::I32Const(0),
            I::LocalSet(8),
            text(s.must_be_a),
            I::LocalGet(9),
            concat.clone(),
            I::LocalSet(7),
            I::End,
            I::End,
            I::End,
            I::End,
            I::LocalGet(8),
            I::I32Eqz,
            if_empty(),
        ]);
        v.extend(report(vec![I::LocalGet(7)]));
        v.extend([I::Br(2), I::End, I::End]);

        // minLength / maxLength (string values)
        v.extend([
            I::LocalGet(6),
            if_empty(),
            I::LocalGet(6),
            load(0),
            I::LocalSet(7),
            I::LocalGet(0),
            load(16),
            I::LocalTee(9),
            I::I32Const(0),
            I::I32GtS,
            I::LocalGet(7),
            I::LocalGet(9),
            I::I32LtS,
            I::I32And,
            if_empty(),
        ]);
        let length_message = |bound: u32| {
            vec![
                text(bound),
                I::LocalGet(9),
                I::Call(self.imports.int_to_string),
                I::Call(self.imports.concat),
                text(s.characters),
                I::Call(self.imports.concat),
            ]
        };
        v.extend(report(length_message(s.at_least)));
        v.extend([
            I::End,
            I::LocalGet(0),
            load(20),
            I::LocalTee(9),
            I::I32Const(0),
            I::I32GeS,
            I::LocalGet(7),
            I::LocalGet(9),
            I::I32GtS,
            I::I32And,
            if_empty(),
        ]);
        v.extend(report(length_message(s.at_most)));
        v.extend([I::End, I::End]);

        // range: a number within [lo, hi] (NaN — no number — fails)
        v.extend([
            I::LocalGet(0),
            load(12),
            I::LocalTee(9),
            if_empty(),
            I::LocalGet(1),
            I::I32Const(0),
            I::Call(self.helpers.numeric),
            I::LocalSet(16),
            I::LocalGet(16),
            I::LocalGet(9),
            f64_at(0),
            I::F64Ge,
            I::LocalGet(16),
            I::LocalGet(9),
            f64_at(8),
            I::F64Le,
            I::I32And,
            I::I32Eqz,
            if_empty(),
        ]);
        v.extend(report(vec![
            I::LocalGet(9),
            I::Call(self.helpers.range_message),
        ]));
        v.extend([I::End, I::End]);

        // match: the whole text matches a named pattern
        if let Some(string_matches) = self.imports.string_matches {
            v.extend([
                I::LocalGet(0),
                load(8),
                I::LocalTee(9),
                if_empty(),
                I::LocalGet(1),
                I::Call(self.helpers.value_text),
                I::LocalSet(13),
                I::LocalGet(9),
                I::Call(self.helpers.pattern_id),
                I::LocalSet(7),
                I::LocalGet(13),
                I::I32Const(0),
                I::I32Ne,
                I::LocalGet(7),
                I::I32Const(0),
                I::I32GeS,
                I::I32And,
                if_i32(),
                I::LocalGet(13),
                I::LocalGet(13),
                load(0),
                I::LocalGet(7),
                I::Call(string_matches),
                I::Else,
                I::I32Const(0),
                I::End,
                I::I32Eqz,
                if_empty(),
            ]);
            v.extend(report(vec![
                I::LocalGet(7),
                I::Call(self.helpers.pattern_message),
            ]));
            v.extend([I::End, I::End]);
        }

        // allowed values (`oneOf:`): the text is one of the listed texts
        v.extend([
            I::LocalGet(0),
            load(40),
            I::LocalTee(9),
            if_empty(),
            I::LocalGet(1),
            I::Call(self.helpers.value_text),
            I::LocalSet(13),
            I::I32Const(0),
            I::LocalSet(8),
            I::LocalGet(13),
            if_empty(),
            I::LocalGet(9),
            load(0),
            I::LocalSet(11),
            I::I32Const(0),
            I::LocalSet(10),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(10),
            I::LocalGet(11),
            I::I32GeS,
            I::BrIf(1),
            I::LocalGet(9),
            I::LocalGet(10),
            I::I32Const(4),
            I::I32Mul,
            I::I32Add,
            load(LIST_DATA_OFFSET),
            I::LocalGet(13),
            I::Call(self.imports.str_eq),
            if_empty(),
            I::I32Const(1),
            I::LocalSet(8),
            I::Br(2),
            I::End,
            I::LocalGet(10),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(10),
            I::Br(0),
            I::End,
            I::End,
            I::End,
            I::LocalGet(8),
            I::I32Eqz,
            if_empty(),
        ]);
        v.extend(report(vec![
            text(s.one_of),
            I::LocalGet(9),
            text(s.comma_space),
            I::Call(self.helpers.join),
            I::Call(self.imports.concat),
        ]));
        v.extend([I::End, I::End]);

        // custom: `boolean fn(string value)` through the function table
        if let Some(check_type) = self.imports.custom_check_type {
            v.extend([
                I::LocalGet(0),
                load(28),
                I::LocalTee(9),
                if_empty(),
                I::LocalGet(1),
                I::Call(self.helpers.value_text),
                I::LocalTee(13),
                if_i32(),
                I::LocalGet(13),
                I::LocalGet(9),
                I::CallIndirect {
                    ty: check_type,
                    table: 0,
                },
                I::Else,
                I::I32Const(0),
                I::End,
                I::I32Eqz,
                if_empty(),
            ]);
            v.extend(report(vec![text(s.is_invalid)]));
            v.extend([I::End, I::End]);
        }

        // shape: a nested object checked with the shape's rules
        v.extend([
            I::LocalGet(0),
            load(56),
            I::LocalTee(9),
            if_empty(),
            I::LocalGet(5),
            I::I32Const(ANY_TAG_OBJECT),
            I::I32Eq,
            if_i32(),
            I::LocalGet(1),
            load(4),
            I::Else,
            I::I32Const(0),
            I::End,
            I::LocalSet(14),
            I::LocalGet(9),
            I::LocalGet(14),
            I::LocalGet(2),
            text(s.dot),
            concat.clone(),
            I::LocalGet(3),
            I::Call(self.helpers.check_object),
            I::LocalSet(15),
            I::LocalGet(15),
            I::LocalGet(14),
            I::I32Ne,
            if_empty(),
        ]);
        v.extend(self.emit_new_box(ANY_TAG_OBJECT, 15, 1));
        v.extend([I::End, I::End]);

        // each: every element of a list checked with the element rules
        v.extend([
            I::LocalGet(0),
            load(60),
            I::LocalTee(9),
            if_empty(),
            I::LocalGet(5),
            I::I32Const(ANY_TAG_ARRAY),
            I::I32Ne,
            if_empty(),
        ]);
        v.extend(report(vec![text(s.must_be_list)]));
        v.extend([
            I::Else,
            // the JSON-tree array: [count][element box]...
            I::LocalGet(1),
            load(4),
            I::LocalTee(13),
            load(0),
            I::LocalSet(11),
            I::I32Const(0),
            I::LocalSet(12),
            I::I32Const(0),
            I::LocalSet(10),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(10),
            I::LocalGet(11),
            I::I32GeS,
            I::BrIf(1),
            // object = the element's JSON-tree object, 0 if it is not one
            I::LocalGet(13),
            I::LocalGet(10),
            I::I32Const(4),
            I::I32Mul,
            I::I32Add,
            load(4),
            I::LocalTee(7),
            if_i32(),
            I::LocalGet(7),
            load(0),
            I::I32Const(ANY_TAG_OBJECT),
            I::I32Eq,
            I::Else,
            I::I32Const(0),
            I::End,
            if_i32(),
            I::LocalGet(7),
            load(4),
            I::Else,
            I::I32Const(0),
            I::End,
            I::LocalSet(14),
            // check it; its errors are named `label[i].field`
            I::LocalGet(9),
            I::LocalGet(14),
            I::LocalGet(2),
            text(s.open_bracket),
            concat.clone(),
            I::LocalGet(10),
            I::Call(self.imports.int_to_string),
            concat.clone(),
            text(s.close_bracket_dot),
            concat.clone(),
            I::LocalGet(3),
            I::Call(self.helpers.check_object),
            I::LocalSet(15),
            I::LocalGet(15),
            I::LocalGet(14),
            I::I32Ne,
            if_empty(),
            // a changed element goes into a copy of the array
            I::LocalGet(12),
            I::I32Eqz,
            if_empty(),
            I::LocalGet(11),
            I::I32Const(4),
            I::I32Mul,
            I::I32Const(4),
            I::I32Add,
            I::LocalSet(8),
            I::LocalGet(8),
            I::Call(self.imports.malloc),
            I::LocalSet(12),
            I::LocalGet(12),
            I::LocalGet(13),
            I::LocalGet(8),
            I::Call(self.imports.memcpy),
            I::End,
        ]);
        v.extend(self.emit_new_box(ANY_TAG_OBJECT, 15, 7));
        v.extend([
            I::LocalGet(12),
            I::LocalGet(10),
            I::I32Const(4),
            I::I32Mul,
            I::I32Add,
            I::LocalGet(7),
            store(4),
            I::End,
            I::LocalGet(10),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(10),
            I::Br(0),
            I::End,
            I::End,
            I::LocalGet(12),
            if_empty(),
        ]);
        v.extend(self.emit_new_box(ANY_TAG_ARRAY, 12, 1));
        v.extend([I::End, I::End, I::End]);

        // end of the rules
        v.push(I::End);

        // message: one message replaces the field's errors
        v.extend([
            I::LocalGet(0),
            load(24),
            I::LocalTee(9),
            if_empty(),
            I::LocalGet(3),
            load(0),
            load(0),
            I::LocalGet(4),
            I::I32GtS,
            if_empty(),
            I::LocalGet(3),
            load(0),
            I::LocalGet(4),
            store(0),
            I::LocalGet(3),
            I::LocalGet(9),
            add_error,
            I::End,
            I::End,
            I::LocalGet(1),
        ]);
        v
    }

    // ==================== Code Generation: Result Construction ====================

    /// `validator.ok(value_ptr: i32) -> ValidationResult`
    ///
    /// Allocates a 24-byte ValidationResult with is_ok=1 and value_ptr set.
    /// Params: 0=value_ptr
    /// Extra locals: 1=result_ptr
    fn generate_validator_ok(&self) -> Vec<Instruction<'static>> {
        vec![
            Instruction::I32Const(24),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalTee(1), // result_ptr
            // is_ok = 1
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // value_ptr at offset 4
            Instruction::LocalGet(1),
            Instruction::LocalGet(0), // value_ptr param
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // errors_list_ptr = 0 at offset 8
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // field_errors_ptr = 0 at offset 12
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            // field_name_ptr = 0 at offset 16
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            // validator_name_ptr = 0 at offset 20
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 20,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1),
        ]
    }

    /// `validator.error(errors_list_ptr: i32) -> ValidationResult`
    ///
    /// Allocates a 24-byte ValidationResult with is_ok=0 and errors_list_ptr set.
    /// Params: 0=errors_list_ptr
    /// Extra locals: 1=result_ptr
    fn generate_validator_error(&self) -> Vec<Instruction<'static>> {
        vec![
            Instruction::I32Const(24),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalTee(1), // result_ptr
            // is_ok = 0
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // value_ptr = 0 at offset 4 (no value on error)
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // errors_list_ptr at offset 8
            Instruction::LocalGet(1),
            Instruction::LocalGet(0), // errors_list_ptr param
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // field_errors_ptr = 0 at offset 12
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            // field_name_ptr = 0 at offset 16
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            // validator_name_ptr = 0 at offset 20
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 20,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(1),
        ]
    }

    // ==================== Code Generation: Result Inspection ====================

    /// `validator.isOk(result_ptr: i32) -> boolean`
    fn generate_validator_is_ok(&self) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(0),
            Instruction::I32Ne, // 1 if is_ok != 0, else 0
        ]
    }

    /// `validator.isError(result_ptr: i32) -> boolean`
    fn generate_validator_is_error(&self) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Eqz, // 1 if is_ok == 0, else 0
        ]
    }

    /// `validator.getValue(result_ptr: i32) -> value_ptr`
    ///
    /// Returns value_ptr from offset 4 of result. Per spec, returns 0 (null)
    /// if the result is an error — callers must check result.ok first.
    fn generate_validator_get_value(&self) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// `validator.getErrors(result_ptr: i32) -> list_ptr`
    ///
    /// The error list at offset 8 of the result; an empty list for a result
    /// without one (`validator.ok`).
    fn generate_validator_get_errors(&self) -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(0),
            load(8),
            if_i32(),
            Instruction::LocalGet(0),
            load(8),
            Instruction::Else,
            Instruction::I32Const(0),
            Instruction::Call(self.imports.list_allocate),
            Instruction::End,
        ]
    }

    /// `validator.getFirstError(result_ptr: i32) -> string_ptr`
    ///
    /// The first element of the error list (list layout:
    /// [length:4][capacity:4][type_id:4][flags:4][data...]), or the empty
    /// string when there is none.
    ///
    /// Locals: 1 errors list.
    fn generate_validator_get_first_error(&self) -> Vec<Instruction<'static>> {
        let empty = self.imports.strings.empty;
        vec![
            Instruction::LocalGet(0),
            load(8),
            Instruction::LocalTee(1),
            if_i32(),
            Instruction::LocalGet(1),
            load(0),
            if_i32(),
            Instruction::LocalGet(1),
            load(LIST_DATA_OFFSET),
            Instruction::Else,
            text(empty),
            Instruction::End,
            Instruction::Else,
            text(empty),
            Instruction::End,
        ]
    }

    /// `validator.fieldErrors(result_ptr: i32, field_name_ptr: i32) -> list_ptr`
    ///
    /// Returns the error list for the named field. Reads field_errors_ptr at result
    /// offset 12. If 0, returns 0 (no per-field errors stored). Per-field error storage
    /// is populated by validator.run when field-level error tracking is enabled.
    fn generate_validator_field_errors(&self) -> Vec<Instruction<'static>> {
        // Params: 0=result_ptr, 1=field_name_ptr
        // For this iteration: if field_errors_ptr == 0, return 0.
        // When field_errors_ptr is non-zero it would be a pairs structure mapping
        // field names to error lists; that iteration is deferred pending pairs.get
        // call-index resolution.
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }), // field_errors_ptr
                // Return the ptr (0 means no field-level errors stored)
        ]
    }

    // ==================== Code Generation: Rule Builders ====================

    /// `validator.field(rules_ptr, field_name_ptr) -> rules_ptr`
    ///
    /// Appends a new field entry — no rules (no maximum length, -1), named
    /// `field_name_ptr` — and increments field_count; the rule calls that
    /// follow apply to this field. A full field array is moved to one twice
    /// as large first.
    ///
    /// Params: 0=rules_ptr, 1=field_name_ptr
    /// Extra locals: 2=field_count, 3=field_entry_ptr, 4=capacity, 5=new_fields_ptr
    fn generate_validator_field(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let mut instrs: Vec<Instruction<'static>> = vec![
            I::LocalGet(0),
            load(0),
            I::LocalSet(2),
            I::LocalGet(0),
            load(4),
            I::LocalSet(4),
            // grow a full field array
            I::LocalGet(2),
            I::LocalGet(4),
            I::I32GeS,
            if_empty(),
            I::LocalGet(4),
            I::I32Const(1),
            I::I32Shl,
            I::I32Const(INITIAL_FIELD_CAPACITY),
            I::LocalGet(4),
            I::Select,
            I::LocalSet(4),
            I::LocalGet(4),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::I32Mul,
            I::Call(self.imports.malloc),
            I::LocalSet(5),
            I::LocalGet(5),
            I::LocalGet(0),
            load(8),
            I::LocalGet(2),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::I32Mul,
            I::Call(self.imports.memcpy),
            I::LocalGet(0),
            I::LocalGet(5),
            store(8),
            I::LocalGet(0),
            I::LocalGet(4),
            store(4),
            I::End,
            // field_entry_ptr = fields_ptr + field_count * FIELD_ENTRY_SIZE
            I::LocalGet(0),
            load(8),
            I::LocalGet(2),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::I32Mul,
            I::I32Add,
            I::LocalSet(3),
        ];

        // Zero out the entry word by word
        for word in 0..FIELD_ENTRY_WORDS {
            instrs.extend([I::LocalGet(3), I::I32Const(0), store((word * 4) as u64)]);
        }

        instrs.extend([
            // name, and no maximum length
            I::LocalGet(3),
            I::LocalGet(1),
            store(0),
            I::LocalGet(3),
            I::I32Const(-1),
            store(20),
            // rules.field_count = field_count + 1
            I::LocalGet(0),
            I::LocalGet(2),
            I::I32Const(1),
            I::I32Add,
            store(0),
            I::LocalGet(0), // return rules_ptr
        ]);
        instrs
    }

    /// Shared helper: emit instructions to compute the address of the last field entry
    /// and store it into `dest_local`. Uses `rules_local` (0-indexed param/local).
    ///
    /// Requires the calling function to have already set up the rules_local.
    fn last_field_entry_instrs(rules_local: u32, dest_local: u32) -> Vec<Instruction<'static>> {
        Self::emit_get_last_field_entry(rules_local, dest_local)
    }

    /// `validator.required(rules_ptr) -> rules_ptr`
    ///
    /// Stores 1 into the required flag (offset 4) of the last field entry.
    /// Params: 0=rules_ptr
    /// Extra locals: 1=field_entry_ptr
    fn generate_validator_required(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 1);
        instrs.extend([
            Instruction::LocalGet(1),
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.optional(rules_ptr) -> rules_ptr`
    ///
    /// Stores 0 into required flag (offset 4) of last field entry.
    /// Params: 0=rules_ptr
    /// Extra locals: 1=field_entry_ptr
    fn generate_validator_optional(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 1);
        instrs.extend([
            Instruction::LocalGet(1),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.match(rules_ptr, pattern_ptr) -> rules_ptr`
    ///
    /// Stores pattern_ptr at offset 8 of last field entry.
    /// Params: 0=rules_ptr, 1=pattern_ptr
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_match(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // pattern_ptr
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.range(rules_ptr, lo: f64, hi: f64) -> rules_ptr`
    ///
    /// Allocates a 16-byte range structure {lo: f64, hi: f64} and stores its
    /// pointer at offset 12 of the last field entry.
    ///
    /// Params: 0=rules_ptr, 1=lo, 2=hi
    /// Extra locals: 3=range_rule_ptr, 4=field_entry_ptr
    fn generate_validator_range(&self) -> Vec<Instruction<'static>> {
        let bound = |offset: u64| {
            Instruction::F64Store(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            })
        };
        let mut instrs = vec![
            Instruction::I32Const(16),
            Instruction::Call(self.imports.malloc),
            Instruction::LocalTee(3),
            Instruction::LocalGet(1),
            bound(0),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2),
            bound(8),
        ];
        instrs.extend(Self::last_field_entry_instrs(0, 4));
        instrs.extend([
            Instruction::LocalGet(4),
            Instruction::LocalGet(3),
            store(12),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.minLength(rules_ptr, min) -> rules_ptr`
    ///
    /// Stores min at offset 16 of last field entry.
    /// Params: 0=rules_ptr, 1=min
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_min_length(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // min
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.maxLength(rules_ptr, max) -> rules_ptr`
    ///
    /// Stores max at offset 20 of last field entry.
    /// Params: 0=rules_ptr, 1=max
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_max_length(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // max
            Instruction::I32Store(MemArg {
                offset: 20,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.message(rules_ptr, message_ptr) -> rules_ptr`
    ///
    /// Stores message_ptr at offset 24 of last field entry.
    /// Params: 0=rules_ptr, 1=message_ptr
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_message(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // message_ptr
            Instruction::I32Store(MemArg {
                offset: 24,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.custom(rules_ptr, fn_ptr) -> rules_ptr`
    ///
    /// Stores fn_ptr at offset 28 of last field entry.
    /// Params: 0=rules_ptr, 1=fn_ptr
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_custom(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // fn_ptr
            Instruction::I32Store(MemArg {
                offset: 28,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.type(rules_ptr, type_name_ptr) -> rules_ptr`
    ///
    /// Stores type_name_ptr at offset 32 of last field entry. The runtime reads this
    /// pointer and compares the string to "string", "integer", "number", "boolean"
    /// to determine the type constraint to apply.
    ///
    /// Params: 0=rules_ptr, 1=type_name_ptr
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_type(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // type_name_ptr
            Instruction::I32Store(MemArg {
                offset: 32,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.trim(rules_ptr) -> rules_ptr`
    ///
    /// Sets trim_flag = 1 at offset 36 of last field entry.
    /// Params: 0=rules_ptr
    /// Extra locals: 1=field_entry_ptr
    fn generate_validator_trim(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 1);
        instrs.extend([
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // trim_flag = 1
            Instruction::I32Store(MemArg {
                offset: 36,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.allowedValues(rules_ptr, values_list_ptr) -> rules_ptr`
    ///
    /// Stores values_list_ptr at offset 40 of last field entry.
    /// Params: 0=rules_ptr, 1=values_list_ptr
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_allowed_values(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // values_list_ptr
            Instruction::I32Store(MemArg {
                offset: 40,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.default(rules_ptr, default_ptr) -> rules_ptr`
    ///
    /// Stores default_ptr at offset 44 of last field entry.
    /// Params: 0=rules_ptr, 1=default_ptr
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_default(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // default_ptr
            Instruction::I32Store(MemArg {
                offset: 44,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.requiredIf(rules_ptr, field_name_ptr, expected_value_ptr) -> rules_ptr`
    ///
    /// Stores field_name_ptr at offset 48 and expected_value_ptr at offset 52 of
    /// the last field entry.
    ///
    /// Params: 0=rules_ptr, 1=field_name_ptr, 2=expected_value_ptr
    /// Extra locals: 3=field_entry_ptr
    fn generate_validator_required_if(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 3);
        instrs.extend([
            Instruction::LocalGet(3),
            Instruction::LocalGet(1), // field_name_ptr
            Instruction::I32Store(MemArg {
                offset: 48,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3),
            Instruction::LocalGet(2), // expected_value_ptr
            Instruction::I32Store(MemArg {
                offset: 52,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.shape(rules_ptr, shape_rules_ptr) -> rules_ptr`
    ///
    /// Stores shape_rules_ptr at offset 56 of last field entry.
    /// Params: 0=rules_ptr, 1=shape_rules_ptr
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_shape(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // shape_rules_ptr
            Instruction::I32Store(MemArg {
                offset: 56,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.each(rules_ptr, each_rules_ptr) -> rules_ptr`
    ///
    /// Stores each_rules_ptr at offset 60 of last field entry.
    /// Params: 0=rules_ptr, 1=each_rules_ptr
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_each(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // each_rules_ptr
            Instruction::I32Store(MemArg {
                offset: 60,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.minValue(rules_ptr, min_i32) -> rules_ptr`
    ///
    /// Stores min_i32 at offset 64 and 1 (active) at offset 72 of last field entry.
    /// Params: 0=rules_ptr, 1=min_i32
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_min_value(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // min_i32
            Instruction::I32Store(MemArg {
                offset: 64,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(2),
            Instruction::I32Const(1), // min_value_active = 1
            Instruction::I32Store(MemArg {
                offset: 72,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.maxValue(rules_ptr, max_i32) -> rules_ptr`
    ///
    /// Stores max_i32 at offset 68 and 1 (active) at offset 76 of last field entry.
    /// Params: 0=rules_ptr, 1=max_i32
    /// Extra locals: 2=field_entry_ptr
    fn generate_validator_max_value(&self) -> Vec<Instruction<'static>> {
        let mut instrs = Self::last_field_entry_instrs(0, 2);
        instrs.extend([
            Instruction::LocalGet(2),
            Instruction::LocalGet(1), // max_i32
            Instruction::I32Store(MemArg {
                offset: 68,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(2),
            Instruction::I32Const(1), // max_value_active = 1
            Instruction::I32Store(MemArg {
                offset: 76,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0),
        ]);
        instrs
    }

    /// `validator.merge(rules_a_ptr, rules_b_ptr) -> new_rules_ptr`
    ///
    /// A new ValidationRules with the field entries of `rules_a`, then those
    /// of `rules_b`; an entry of `rules_b` replaces the entry with the same
    /// name (§15: `b`'s field wins). `rules_a` and `rules_b` are unchanged.
    ///
    /// Params: 0=rules_a_ptr, 1=rules_b_ptr
    /// Extra locals: 2=new_rules_ptr, 3=count_a, 4=count_b, 5=capacity,
    /// 6=new_fields_ptr, 7=copy_index, 8=src_entry_ptr, 9=dst_entry_ptr,
    /// 10=existing index, 11=field_count
    fn generate_validator_merge(&self) -> Vec<Instruction<'static>> {
        use Instruction as I;
        let entry = |fields: u32, index: u32| {
            [
                I::LocalGet(fields),
                I::LocalGet(index),
                I::I32Const(FIELD_ENTRY_SIZE),
                I::I32Mul,
                I::I32Add,
            ]
        };
        let mut v = vec![
            I::LocalGet(0),
            load(0),
            I::LocalSet(3),
            I::LocalGet(1),
            load(0),
            I::LocalSet(4),
            // capacity = max(count_a + count_b, INITIAL_FIELD_CAPACITY)
            I::LocalGet(3),
            I::LocalGet(4),
            I::I32Add,
            I::LocalTee(5),
            I::I32Const(INITIAL_FIELD_CAPACITY),
            I::LocalGet(5),
            I::I32Const(INITIAL_FIELD_CAPACITY),
            I::I32GtS,
            I::Select,
            I::LocalSet(5),
            // header: field_count 0, capacity, fields, no name
            I::I32Const(16),
            I::Call(self.imports.malloc),
            I::LocalTee(2),
            I::I32Const(0),
            store(0),
            I::LocalGet(2),
            I::LocalGet(5),
            store(4),
            I::LocalGet(2),
            I::I32Const(0),
            store(12),
            I::LocalGet(5),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::I32Mul,
            I::Call(self.imports.malloc),
            I::LocalSet(6),
            I::LocalGet(2),
            I::LocalGet(6),
            store(8),
            // the entries of rules_a
            I::I32Const(0),
            I::LocalSet(7),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(7),
            I::LocalGet(3),
            I::I32GeS,
            I::BrIf(1),
        ];
        v.extend(entry(6, 7));
        v.extend([I::LocalGet(0), load(8), I::LocalSet(8)]);
        v.extend(entry(8, 7));
        v.extend([
            I::I32Const(FIELD_ENTRY_SIZE),
            I::Call(self.imports.memcpy),
            I::LocalGet(7),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(7),
            I::Br(0),
            I::End,
            I::End,
            I::LocalGet(3),
            I::LocalSet(11),
            // the entries of rules_b, each replacing the entry with its name
            I::I32Const(0),
            I::LocalSet(7),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(7),
            I::LocalGet(4),
            I::I32GeS,
            I::BrIf(1),
            I::LocalGet(1),
            load(8),
            I::LocalSet(8),
        ]);
        v.extend(entry(8, 7));
        v.extend([
            I::LocalSet(8),
            I::LocalGet(2),
            I::LocalGet(11),
            store(0),
            I::LocalGet(2),
            I::LocalGet(8),
            load(0),
            I::Call(self.helpers.field_index),
            I::LocalTee(10),
            I::I32Const(0),
            I::I32LtS,
            if_empty(),
            I::LocalGet(11),
            I::LocalSet(10),
            I::LocalGet(11),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(11),
            I::End,
        ]);
        v.extend(entry(6, 10));
        v.extend([
            I::LocalSet(9),
            I::LocalGet(9),
            I::LocalGet(8),
            I::I32Const(FIELD_ENTRY_SIZE),
            I::Call(self.imports.memcpy),
            I::LocalGet(7),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(7),
            I::Br(0),
            I::End,
            I::End,
            I::LocalGet(2),
            I::LocalGet(11),
            store(0),
            I::LocalGet(2), // return new_rules_ptr
        ]);
        v
    }
}

/// Number of i32 locals `validator.run` declares after its two params
/// (indices 2..=16, see `generate_validator_run`).
const RUN_EXTRA_LOCALS: usize = 15;

/// Number of i32 locals `validator.runField` declares after its three params
/// (indices 3..=9, see `generate_validator_run_field`).
const RUN_FIELD_EXTRA_LOCALS: usize = 7;

/// Number of i32 locals `__validator_check_object` declares after its four
/// params (indices 4..=13).
const CHECK_FIELDS_LOCALS: usize = 10;

/// Number of i32 locals `validator.merge` declares after its two params.
const MERGE_EXTRA_LOCALS: usize = 10;

/// The locals of the field loop `emit_check_fields` emits.
#[derive(Debug, Clone, Copy)]
struct FieldLoop {
    rules: u32,
    /// The JSON-tree object, 0 for none.
    object: u32,
    /// The error-name prefix (a nested object), if any.
    prefix: Option<u32>,
    cell: u32,
    count: u32,
    index: u32,
    entry: u32,
    name: u32,
    object_count: u32,
    entry_index: u32,
    value: u32,
    checked: u32,
    copy: u32,
    label: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Function indices for building instruction sequences outside a module.
    const TEST_IMPORTS: ValidatorImports = ValidatorImports {
        malloc: 7,
        str_eq: 8,
        memcpy: 9,
        concat: 10,
        trim: 11,
        list_allocate: 12,
        list_add: 13,
        int_to_string: 14,
        int64_to_string: 15,
        float_to_string: 16,
        string_matches: Some(17),
        custom_check_type: Some(1),
        first_helper: 100,
        strings: ValidatorStrings {
            empty: 1000,
            dot: 1004,
            open_bracket: 1008,
            close_bracket_dot: 1012,
            comma_space: 1016,
            is_required: 1020,
            must_be_string: 1040,
            must_be_whole: 1064,
            must_be_number: 1088,
            must_be_boolean: 1112,
            must_be_a: 1136,
            at_least: 1152,
            at_most: 1176,
            characters: 1200,
            between: 1216,
            and: 1240,
            one_of: 1248,
            is_invalid: 1272,
            must_be_list: 1288,
            no_match: 1308,
            true_text: 1348,
            false_text: 1356,
            type_string: 1364,
            type_integer: 1376,
            type_number: 1388,
            type_boolean: 1400,
            patterns: [1412, 1424, 1432, 1440, 1452, 1460, 1472, 1484],
            pattern_messages: [1500, 1536, 1560, 1584, 1616, 1656, 1688, 1708],
        },
    };

    #[test]
    fn test_validator_manager_creation() {
        let _manager = ValidatorManager::new(TEST_IMPORTS);
    }

    #[test]
    fn test_field_entry_size_constant() {
        // Verify FIELD_ENTRY_SIZE covers all 20 fields at 4 bytes each
        assert_eq!(FIELD_ENTRY_SIZE, 80);
    }

    #[test]
    fn test_initial_capacity_bytes() {
        // Verify field array allocation size
        let expected_bytes = INITIAL_FIELD_CAPACITY * FIELD_ENTRY_SIZE;
        assert_eq!(expected_bytes, 640);
    }

    #[test]
    fn test_generate_validator_create_not_empty() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_create();
        // Must produce a non-trivial instruction sequence
        assert!(instrs.len() > 5);
    }

    #[test]
    fn test_generate_validator_run_produces_loop() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_run();
        // The run function must contain loop and block instructions
        let has_loop = instrs.iter().any(|i| matches!(i, Instruction::Loop(_)));
        let has_block = instrs.iter().any(|i| matches!(i, Instruction::Block(_)));
        assert!(has_loop, "validator.run must contain a Loop instruction");
        assert!(has_block, "validator.run must contain a Block instruction");
    }

    #[test]
    fn test_generate_validator_field_zeros_entry() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_field();
        // Must have exactly 20 zero-store instructions (one per word in 80-byte entry)
        let zero_stores = instrs
            .iter()
            .filter(|i| matches!(i, Instruction::I32Const(0)))
            .count();
        // 20 zeros for the entry fields plus initializations
        assert!(
            zero_stores >= 20,
            "validator.field must zero all 20 words of the field entry"
        );
    }

    #[test]
    fn test_min_length_stores_at_offset_16() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_min_length();
        // Must contain an I32Store with offset 16
        let has_offset_16 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 16, .. })));
        assert!(
            has_offset_16,
            "validator.minLength must store at field entry offset 16"
        );
    }

    #[test]
    fn test_max_length_stores_at_offset_20() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_max_length();
        let has_offset_20 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 20, .. })));
        assert!(
            has_offset_20,
            "validator.maxLength must store at field entry offset 20"
        );
    }

    #[test]
    fn test_message_stores_at_offset_24() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_message();
        let has_offset_24 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 24, .. })));
        assert!(
            has_offset_24,
            "validator.message must store at field entry offset 24"
        );
    }

    #[test]
    fn test_custom_stores_at_offset_28() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_custom();
        let has_offset_28 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 28, .. })));
        assert!(
            has_offset_28,
            "validator.custom must store at field entry offset 28"
        );
    }

    #[test]
    fn test_trim_stores_at_offset_36() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_trim();
        let has_offset_36 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 36, .. })));
        assert!(
            has_offset_36,
            "validator.trim must store at field entry offset 36"
        );
    }

    #[test]
    fn test_min_value_stores_at_offset_64_and_72() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_min_value();
        let has_offset_64 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 64, .. })));
        let has_offset_72 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 72, .. })));
        assert!(
            has_offset_64,
            "validator.minValue must store value at offset 64"
        );
        assert!(
            has_offset_72,
            "validator.minValue must set active flag at offset 72"
        );
    }

    #[test]
    fn test_max_value_stores_at_offset_68_and_76() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_max_value();
        let has_offset_68 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 68, .. })));
        let has_offset_76 = instrs
            .iter()
            .any(|i| matches!(i, Instruction::I32Store(MemArg { offset: 76, .. })));
        assert!(
            has_offset_68,
            "validator.maxValue must store value at offset 68"
        );
        assert!(
            has_offset_76,
            "validator.maxValue must set active flag at offset 76"
        );
    }

    #[test]
    fn test_merge_produces_two_copy_loops() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_merge();
        let loop_count = instrs
            .iter()
            .filter(|i| matches!(i, Instruction::Loop(_)))
            .count();
        assert_eq!(
            loop_count, 2,
            "validator.merge must contain exactly 2 copy loops"
        );
    }

    #[test]
    fn test_field_entry_holds_no_value() {
        // A field rule holds rules only: validator.field writes nothing past
        // the 80-byte entry and stores no value.
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_field();
        let past_entry = instrs.iter().any(|i| {
            matches!(i, Instruction::I32Store(MemArg { offset, .. }) if *offset >= FIELD_ENTRY_SIZE as u64)
        });
        assert!(!past_entry, "validator.field must stay inside its entry");
    }

    #[test]
    fn test_run_looks_fields_up_in_the_input() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_run();
        let compares_keys = instrs
            .iter()
            .any(|i| matches!(i, Instruction::Call(c) if *c == TEST_IMPORTS.str_eq));
        let reads_object = instrs.windows(2).any(|w| {
            matches!(
                w,
                [Instruction::I32Const(ANY_TAG_OBJECT), Instruction::I32Eq]
            )
        });
        assert!(
            compares_keys && reads_object,
            "validator.run must look each field up by name in the input object"
        );
    }

    #[test]
    fn test_run_field_compares_names_by_content() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_run_field();
        let compares = instrs
            .iter()
            .any(|i| matches!(i, Instruction::Call(c) if *c == TEST_IMPORTS.str_eq));
        assert!(
            compares,
            "validator.runField must compare field names by content"
        );
    }

    #[test]
    fn test_required_sets_flag_to_one() {
        let manager = ValidatorManager::new(TEST_IMPORTS);
        let instrs = manager.generate_validator_required();
        let sets_one = instrs.windows(2).any(|w| {
            matches!(
                w,
                [
                    Instruction::I32Const(1),
                    Instruction::I32Store(MemArg { offset: 4, .. })
                ]
            )
        });
        assert!(sets_one, "validator.required must store 1 at offset 4");
    }
}
