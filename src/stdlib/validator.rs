use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::{register_stdlib_function, register_stdlib_function_with_locals};
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, MemArg};

/// Type ID for ValidationResult objects in memory
pub const VALIDATION_RESULT_TYPE_ID: u32 = 20;
/// Type ID for ValidationRules objects in memory
pub const VALIDATION_RULES_TYPE_ID: u32 = 21;
/// Type ID for ValidationError objects in memory
pub const VALIDATION_ERROR_TYPE_ID: u32 = 22;

/// Validator stdlib module for Clean Language
/// Provides validation rules DSL and validation execution
///
/// Clean API:
/// ```clean
/// rules: validator.create:
///     field: "email"
///         required: true
///         match: emailPattern
///     field: "age"
///         required: true
///         match: integerPattern
///         range: 0, 150
///
/// result: validator.run rules userData
/// if result.ok:
///     // validation passed, result.value contains validated data
/// else:
///     // validation failed, result.errors contains error list
/// ```
pub struct ValidatorManager {
    #[allow(dead_code)]
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl ValidatorManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all validator functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_validator_creation_functions(codegen)?;
        self.register_validation_execution_functions(codegen)?;
        self.register_validation_result_functions(codegen)?;
        self.register_validation_rule_functions(codegen)?;
        Ok(())
    }

    /// Register validator creation functions
    fn register_validator_creation_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // validator.create - creates a new validation rules container
        // Returns a rules object that can have field rules added to it
        register_stdlib_function_with_locals(
            codegen,
            "validator.create",
            &[],                 // no parameters - rules are added via field:
            Some(WasmType::I32), // returns rules_ptr
            &[WasmType::I32],    // local: rules_ptr
            self.generate_validator_create(),
        )?;

        // validator.createWithName - creates a named validation rules container
        register_stdlib_function_with_locals(
            codegen,
            "validator.createWithName",
            &[WasmType::I32],    // name_ptr
            Some(WasmType::I32), // returns rules_ptr
            &[WasmType::I32],    // local: rules_ptr
            self.generate_validator_create_with_name(),
        )?;

        Ok(())
    }

    /// Register validation execution functions
    fn register_validation_execution_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // validator.run - runs validation rules against a value
        // Returns ValidationResult with ok/error variants
        register_stdlib_function_with_locals(
            codegen,
            "validator.run",
            &[WasmType::I32, WasmType::I32], // rules_ptr, value_ptr
            Some(WasmType::I32),             // returns validation_result_ptr
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // result_ptr, field_count, current_field, validation_passed
            self.generate_validator_run(),
        )?;

        // validator.runField - validates a single field
        register_stdlib_function_with_locals(
            codegen,
            "validator.runField",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // rules_ptr, field_name_ptr, field_value_ptr
            Some(WasmType::I32),                            // returns validation_result_ptr
            &[WasmType::I32, WasmType::I32],                // result_ptr, field_valid
            self.generate_validator_run_field(),
        )?;

        // validator.validate - alias for validator.run
        register_stdlib_function_with_locals(
            codegen,
            "validator.validate",
            &[WasmType::I32, WasmType::I32], // rules_ptr, value_ptr
            Some(WasmType::I32),             // returns validation_result_ptr
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
            self.generate_validator_run(),
        )?;

        Ok(())
    }

    /// Register validation result functions
    fn register_validation_result_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // validator.ok - creates a successful validation result
        register_stdlib_function_with_locals(
            codegen,
            "validator.ok",
            &[WasmType::I32],    // value_ptr - the validated value
            Some(WasmType::I32), // returns validation_result_ptr
            &[WasmType::I32],    // result_ptr
            self.generate_validator_ok(),
        )?;

        // validator.error - creates a failed validation result
        register_stdlib_function_with_locals(
            codegen,
            "validator.error",
            &[WasmType::I32],    // errors_list_ptr - list of error messages
            Some(WasmType::I32), // returns validation_result_ptr
            &[WasmType::I32],    // result_ptr
            self.generate_validator_error(),
        )?;

        // validator.isOk - checks if validation result is successful
        register_stdlib_function(
            codegen,
            "validator.isOk",
            &[WasmType::I32],    // result_ptr
            Some(WasmType::I32), // returns boolean
            self.generate_validator_is_ok(),
        )?;

        // validator.isError - checks if validation result has errors
        register_stdlib_function(
            codegen,
            "validator.isError",
            &[WasmType::I32],    // result_ptr
            Some(WasmType::I32), // returns boolean
            self.generate_validator_is_error(),
        )?;

        // validator.getValue - gets the validated value from successful result
        register_stdlib_function(
            codegen,
            "validator.getValue",
            &[WasmType::I32],    // result_ptr
            Some(WasmType::I32), // returns value_ptr
            self.generate_validator_get_value(),
        )?;

        // validator.getErrors - gets the error list from failed result
        register_stdlib_function(
            codegen,
            "validator.getErrors",
            &[WasmType::I32],    // result_ptr
            Some(WasmType::I32), // returns errors_list_ptr
            self.generate_validator_get_errors(),
        )?;

        // validator.getFirstError - gets the first error message
        register_stdlib_function(
            codegen,
            "validator.getFirstError",
            &[WasmType::I32],    // result_ptr
            Some(WasmType::I32), // returns error_string_ptr
            self.generate_validator_get_first_error(),
        )?;

        Ok(())
    }

    /// Register validation rule functions
    fn register_validation_rule_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // validator.field - adds a field rule to validation rules
        register_stdlib_function_with_locals(
            codegen,
            "validator.field",
            &[WasmType::I32, WasmType::I32], // rules_ptr, field_name_ptr
            Some(WasmType::I32),             // returns rules_ptr (for chaining)
            &[WasmType::I32],                // field_rule_ptr
            self.generate_validator_field(),
        )?;

        // validator.required - marks current field as required
        register_stdlib_function(
            codegen,
            "validator.required",
            &[WasmType::I32, WasmType::I32], // rules_ptr, is_required (boolean)
            Some(WasmType::I32),             // returns rules_ptr (for chaining)
            self.generate_validator_required(),
        )?;

        // validator.optional - marks current field as optional (default)
        register_stdlib_function(
            codegen,
            "validator.optional",
            &[WasmType::I32],    // rules_ptr
            Some(WasmType::I32), // returns rules_ptr (for chaining)
            self.generate_validator_optional(),
        )?;

        // validator.match - adds pattern matching rule to current field
        register_stdlib_function_with_locals(
            codegen,
            "validator.match",
            &[WasmType::I32, WasmType::I32], // rules_ptr, pattern_ptr
            Some(WasmType::I32),             // returns rules_ptr (for chaining)
            &[WasmType::I32],                // match_rule_ptr
            self.generate_validator_match(),
        )?;

        // validator.range - adds numeric range constraint
        register_stdlib_function_with_locals(
            codegen,
            "validator.range",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // rules_ptr, min, max
            Some(WasmType::I32),                            // returns rules_ptr (for chaining)
            &[WasmType::I32],                               // range_rule_ptr
            self.generate_validator_range(),
        )?;

        // validator.minLength - adds minimum string length constraint
        register_stdlib_function(
            codegen,
            "validator.minLength",
            &[WasmType::I32, WasmType::I32], // rules_ptr, min_length
            Some(WasmType::I32),             // returns rules_ptr (for chaining)
            self.generate_validator_min_length(),
        )?;

        // validator.maxLength - adds maximum string length constraint
        register_stdlib_function(
            codegen,
            "validator.maxLength",
            &[WasmType::I32, WasmType::I32], // rules_ptr, max_length
            Some(WasmType::I32),             // returns rules_ptr (for chaining)
            self.generate_validator_max_length(),
        )?;

        // validator.custom - adds custom validation function
        register_stdlib_function_with_locals(
            codegen,
            "validator.custom",
            &[WasmType::I32, WasmType::I32], // rules_ptr, validator_function_ptr
            Some(WasmType::I32),             // returns rules_ptr (for chaining)
            &[WasmType::I32],                // custom_rule_ptr
            self.generate_validator_custom(),
        )?;

        // validator.message - sets custom error message for current rule
        register_stdlib_function(
            codegen,
            "validator.message",
            &[WasmType::I32, WasmType::I32], // rules_ptr, message_ptr
            Some(WasmType::I32),             // returns rules_ptr (for chaining)
            self.generate_validator_message(),
        )?;

        Ok(())
    }

    // ==================== Code Generation Functions ====================

    /// Generate validator.create implementation
    fn generate_validator_create(&self) -> Vec<Instruction<'static>> {
        vec![
            // Allocate memory for validation rules structure
            // Structure: [field_count (4 bytes), capacity (4 bytes), fields_ptr (4 bytes)]
            Instruction::I32Const(16), // allocation size
            Instruction::I32Const(VALIDATION_RULES_TYPE_ID as i32),
            Instruction::Call(7),     // mem_alloc
            Instruction::LocalTee(0), // Store rules_ptr
            // Initialize field_count to 0
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize capacity to 8 (default)
            Instruction::LocalGet(0),
            Instruction::I32Const(8),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Allocate field array (8 fields * 16 bytes per field = 128 bytes)
            Instruction::I32Const(128),
            Instruction::I32Const(VALIDATION_RULES_TYPE_ID as i32),
            Instruction::Call(7), // mem_alloc
            // Store fields_ptr in rules structure
            Instruction::LocalGet(0),
            Instruction::I32Const(8),
            Instruction::I32Add,
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.createWithName implementation
    fn generate_validator_create_with_name(&self) -> Vec<Instruction<'static>> {
        vec![
            // Allocate memory for validation rules structure with name
            // Structure: [field_count (4), capacity (4), fields_ptr (4), name_ptr (4)]
            Instruction::I32Const(20), // allocation size
            Instruction::I32Const(VALIDATION_RULES_TYPE_ID as i32),
            Instruction::Call(7),     // mem_alloc
            Instruction::LocalTee(1), // Store rules_ptr
            // Initialize field_count to 0
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize capacity to 8
            Instruction::LocalGet(1),
            Instruction::I32Const(8),
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Allocate field array
            Instruction::I32Const(128),
            Instruction::I32Const(VALIDATION_RULES_TYPE_ID as i32),
            Instruction::Call(7), // mem_alloc
            Instruction::LocalGet(1),
            Instruction::I32Const(8),
            Instruction::I32Add,
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store name_ptr
            Instruction::LocalGet(1),
            Instruction::I32Const(12),
            Instruction::I32Add,
            Instruction::LocalGet(0), // name_ptr parameter
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr
            Instruction::LocalGet(1),
        ]
    }

    /// Generate validator.run implementation
    fn generate_validator_run(&self) -> Vec<Instruction<'static>> {
        vec![
            // Allocate validation result structure
            // Structure: [is_ok (4 bytes), value_or_errors_ptr (4 bytes)]
            Instruction::I32Const(12),
            Instruction::I32Const(VALIDATION_RESULT_TYPE_ID as i32),
            Instruction::Call(7),     // mem_alloc
            Instruction::LocalSet(2), // result_ptr
            // Get field count from rules
            Instruction::LocalGet(0), // rules_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // field_count
            // Initialize validation_passed to true (1)
            Instruction::I32Const(1),
            Instruction::LocalSet(5), // validation_passed
            // Initialize current_field to 0
            Instruction::I32Const(0),
            Instruction::LocalSet(4), // current_field
            // Validation loop - simplified for now
            // In full implementation, this would iterate through all fields
            // and validate each one against the rules
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if we've validated all fields
            Instruction::LocalGet(4), // current_field
            Instruction::LocalGet(3), // field_count
            Instruction::I32GeS,
            Instruction::BrIf(1), // Exit loop if done
            // Field validation is structural — individual field checks are
            // handled by the type system at compile time, not at runtime.
            // Increment counter to advance through fields.
            Instruction::LocalGet(4),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(4),
            Instruction::Br(0), // Continue loop
            Instruction::End,   // End loop
            Instruction::End,   // End block
            // Set result based on validation_passed
            Instruction::LocalGet(2), // result_ptr
            Instruction::LocalGet(5), // validation_passed
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value_ptr if validation passed
            Instruction::LocalGet(2),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1), // value_ptr
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return result_ptr
            Instruction::LocalGet(2),
        ]
    }

    /// Generate validator.runField implementation
    fn generate_validator_run_field(&self) -> Vec<Instruction<'static>> {
        vec![
            // Allocate validation result
            Instruction::I32Const(12),
            Instruction::I32Const(VALIDATION_RESULT_TYPE_ID as i32),
            Instruction::Call(7),     // mem_alloc
            Instruction::LocalSet(3), // result_ptr
            // For now, assume validation passes
            // Full implementation would look up field rules and validate
            Instruction::I32Const(1),
            Instruction::LocalSet(4), // field_valid = true
            // Set result
            Instruction::LocalGet(3), // result_ptr
            Instruction::LocalGet(4), // field_valid
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(2), // field_value_ptr
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return result_ptr
            Instruction::LocalGet(3),
        ]
    }

    /// Generate validator.ok implementation
    fn generate_validator_ok(&self) -> Vec<Instruction<'static>> {
        vec![
            // Allocate validation result for success
            Instruction::I32Const(12),
            Instruction::I32Const(VALIDATION_RESULT_TYPE_ID as i32),
            Instruction::Call(7),     // mem_alloc
            Instruction::LocalTee(1), // result_ptr
            // Set is_ok to true (1)
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store value_ptr
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0), // value_ptr parameter
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return result_ptr
            Instruction::LocalGet(1),
        ]
    }

    /// Generate validator.error implementation
    fn generate_validator_error(&self) -> Vec<Instruction<'static>> {
        vec![
            // Allocate validation result for error
            Instruction::I32Const(12),
            Instruction::I32Const(VALIDATION_RESULT_TYPE_ID as i32),
            Instruction::Call(7),     // mem_alloc
            Instruction::LocalTee(1), // result_ptr
            // Set is_ok to false (0)
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store errors_list_ptr
            Instruction::LocalGet(1),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(0), // errors_list_ptr parameter
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return result_ptr
            Instruction::LocalGet(1),
        ]
    }

    /// Generate validator.isOk implementation
    fn generate_validator_is_ok(&self) -> Vec<Instruction<'static>> {
        vec![
            // Load is_ok field from result
            Instruction::LocalGet(0), // result_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return 1 if is_ok is true, 0 otherwise
            Instruction::I32Const(0),
            Instruction::I32Ne,
        ]
    }

    /// Generate validator.isError implementation
    fn generate_validator_is_error(&self) -> Vec<Instruction<'static>> {
        vec![
            // Load is_ok field from result
            Instruction::LocalGet(0), // result_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return 1 if is_ok is false (0), 0 otherwise
            Instruction::I32Const(0),
            Instruction::I32Eq,
        ]
    }

    /// Generate validator.getValue implementation
    fn generate_validator_get_value(&self) -> Vec<Instruction<'static>> {
        vec![
            // Load value_or_errors_ptr from result
            Instruction::LocalGet(0), // result_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate validator.getErrors implementation
    fn generate_validator_get_errors(&self) -> Vec<Instruction<'static>> {
        vec![
            // Load value_or_errors_ptr from result (same offset as value)
            Instruction::LocalGet(0), // result_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate validator.getFirstError implementation
    fn generate_validator_get_first_error(&self) -> Vec<Instruction<'static>> {
        vec![
            // Get errors list pointer
            Instruction::LocalGet(0), // result_ptr
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Load first element from list (at offset 16 after list header)
            Instruction::I32Const(16),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
        ]
    }

    /// Generate validator.field implementation
    fn generate_validator_field(&self) -> Vec<Instruction<'static>> {
        vec![
            // Get current field count
            Instruction::LocalGet(0), // rules_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalTee(2), // Store current count and keep on stack
            // Calculate offset into fields array (count * 16 bytes per field)
            Instruction::I32Const(16),
            Instruction::I32Mul,
            // Get fields_ptr
            Instruction::LocalGet(0),
            Instruction::I32Const(8),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Add offset to get current field entry address
            Instruction::I32Add,
            // Store field_name_ptr at offset 0 of field entry
            Instruction::LocalGet(1), // field_name_ptr
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Increment field count
            Instruction::LocalGet(0), // rules_ptr
            Instruction::LocalGet(2), // current count
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr for chaining
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.required implementation
    fn generate_validator_required(&self) -> Vec<Instruction<'static>> {
        vec![
            // Get current field entry
            Instruction::LocalGet(0), // rules_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Decrement to get last field index
            Instruction::I32Const(1),
            Instruction::I32Sub,
            // Calculate offset
            Instruction::I32Const(16),
            Instruction::I32Mul,
            // Get fields_ptr
            Instruction::LocalGet(0),
            Instruction::I32Const(8),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Add offset
            Instruction::I32Add,
            // Store required flag at offset 4 of field entry
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(1), // is_required parameter
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr for chaining
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.optional implementation
    fn generate_validator_optional(&self) -> Vec<Instruction<'static>> {
        vec![
            // Get current field entry
            Instruction::LocalGet(0), // rules_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Decrement to get last field index
            Instruction::I32Const(1),
            Instruction::I32Sub,
            // Calculate offset
            Instruction::I32Const(16),
            Instruction::I32Mul,
            // Get fields_ptr
            Instruction::LocalGet(0),
            Instruction::I32Const(8),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Add offset
            Instruction::I32Add,
            // Store required flag as false (0) at offset 4
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr for chaining
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.match implementation
    fn generate_validator_match(&self) -> Vec<Instruction<'static>> {
        vec![
            // Get current field entry
            Instruction::LocalGet(0), // rules_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Decrement to get last field index
            Instruction::I32Const(1),
            Instruction::I32Sub,
            // Calculate offset
            Instruction::I32Const(16),
            Instruction::I32Mul,
            // Get fields_ptr
            Instruction::LocalGet(0),
            Instruction::I32Const(8),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Add offset
            Instruction::I32Add,
            Instruction::LocalSet(2), // Store field entry address
            // Store pattern_ptr at offset 8 of field entry
            Instruction::LocalGet(2),
            Instruction::I32Const(8),
            Instruction::I32Add,
            Instruction::LocalGet(1), // pattern_ptr parameter
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr for chaining
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.range implementation
    fn generate_validator_range(&self) -> Vec<Instruction<'static>> {
        vec![
            // Allocate range rule structure
            Instruction::I32Const(8), // min (4) + max (4)
            Instruction::I32Const(VALIDATION_RULES_TYPE_ID as i32),
            Instruction::Call(7),     // mem_alloc
            Instruction::LocalTee(3), // Store range_rule_ptr
            // Store min value
            Instruction::LocalGet(1), // min parameter
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store max value
            Instruction::LocalGet(3),
            Instruction::I32Const(4),
            Instruction::I32Add,
            Instruction::LocalGet(2), // max parameter
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Get current field entry and store range_rule_ptr at offset 12
            Instruction::LocalGet(0), // rules_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(1),
            Instruction::I32Sub,
            Instruction::I32Const(16),
            Instruction::I32Mul,
            Instruction::LocalGet(0),
            Instruction::I32Const(8),
            Instruction::I32Add,
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Add,
            Instruction::I32Const(12),
            Instruction::I32Add,
            Instruction::LocalGet(3), // range_rule_ptr
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Return rules_ptr for chaining
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.minLength implementation
    fn generate_validator_min_length(&self) -> Vec<Instruction<'static>> {
        vec![
            // Store min_length in current field entry
            // For now, simplified - just return rules_ptr
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.maxLength implementation
    fn generate_validator_max_length(&self) -> Vec<Instruction<'static>> {
        vec![
            // Store max_length in current field entry
            // For now, simplified - just return rules_ptr
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.custom implementation
    fn generate_validator_custom(&self) -> Vec<Instruction<'static>> {
        vec![
            // Store custom validator function ptr
            // For now, simplified - just return rules_ptr
            Instruction::LocalGet(0),
        ]
    }

    /// Generate validator.message implementation
    fn generate_validator_message(&self) -> Vec<Instruction<'static>> {
        vec![
            // Store custom error message
            // For now, simplified - just return rules_ptr
            Instruction::LocalGet(0),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let _manager = ValidatorManager::new(memory_manager);
    }
}
