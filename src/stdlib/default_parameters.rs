use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::register_stdlib_function_with_locals;
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Default parameter implementation for Clean Language
/// Enables default values in function parameters and input blocks
pub struct DefaultParameterManager {
    #[allow(dead_code)]
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl DefaultParameterManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all default parameter functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_parameter_default_functions(codegen)?;
        self.register_input_block_functions(codegen)?;
        self.register_default_evaluation_functions(codegen)?;
        self.register_parameter_utility_functions(codegen)?;
        Ok(())
    }

    /// Register parameter default value functions
    fn register_parameter_default_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Apply default value for integer parameter
        register_stdlib_function_with_locals(
            codegen,
            "default.integerParam",
            &[WasmType::I32, WasmType::I32], // provided_value, default_value
            Some(WasmType::I32),             // final value
            &[WasmType::I32],                // final_value
            self.generate_integer_param_default(),
        )?;

        // Apply default value for number parameter
        register_stdlib_function_with_locals(
            codegen,
            "default.numberParam",
            &[WasmType::F64, WasmType::F64], // provided_value, default_value
            Some(WasmType::F64),             // final value
            &[WasmType::F64],                // final_value
            self.generate_number_param_default(),
        )?;

        // Apply default value for string parameter
        register_stdlib_function_with_locals(
            codegen,
            "default.stringParam",
            &[WasmType::I32, WasmType::I32], // provided_ptr, default_ptr
            Some(WasmType::I32),             // final string pointer
            &[WasmType::I32],                // final_ptr
            self.generate_string_param_default(),
        )?;

        // Apply default value for boolean parameter
        register_stdlib_function_with_locals(
            codegen,
            "default.booleanParam",
            &[WasmType::I32, WasmType::I32], // provided_value, default_value
            Some(WasmType::I32),             // final value
            &[WasmType::I32],                // final_value
            self.generate_boolean_param_default(),
        )?;

        // Apply default value for list parameter
        register_stdlib_function_with_locals(
            codegen,
            "default.listParam",
            &[WasmType::I32, WasmType::I32], // provided_ptr, default_ptr
            Some(WasmType::I32),             // final list pointer
            &[WasmType::I32],                // final_ptr
            self.generate_list_param_default(),
        )?;

        Ok(())
    }

    /// Register input block default functions
    fn register_input_block_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Create input block with defaults
        register_stdlib_function_with_locals(
            codegen,
            "input.createWithDefaults",
            &[WasmType::I32],                // input_definition_ptr
            Some(WasmType::I32),             // input_block_ptr
            &[WasmType::I32, WasmType::I32], // input_ptr, field_count
            self.generate_create_input_with_defaults(),
        )?;

        // Apply defaults to input block
        register_stdlib_function_with_locals(
            codegen,
            "input.applyDefaults",
            &[WasmType::I32, WasmType::I32], // input_block_ptr, defaults_definition_ptr
            None,                            // void
            &[WasmType::I32, WasmType::I32, WasmType::I32], // field_index, field_ptr, default_ptr
            self.generate_apply_input_defaults(),
        )?;

        // Get field value with default fallback
        register_stdlib_function_with_locals(
            codegen,
            "input.getFieldWithDefault",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // input_block_ptr, field_name_ptr, default_value_ptr
            Some(WasmType::I32),                            // field value pointer
            &[WasmType::I32, WasmType::I32],                // field_ptr, default_applied
            self.generate_get_field_with_default(),
        )?;

        // Validate input block completeness
        register_stdlib_function_with_locals(
            codegen,
            "input.validateWithDefaults",
            &[WasmType::I32],                               // input_block_ptr
            Some(WasmType::I32), // 1 if valid, 0 if missing required fields
            &[WasmType::I32, WasmType::I32, WasmType::I32], // field_count, valid_count, is_valid
            self.generate_validate_input_with_defaults(),
        )?;

        Ok(())
    }

    /// Register default value evaluation functions
    fn register_default_evaluation_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Evaluate default expression (integer)
        register_stdlib_function_with_locals(
            codegen,
            "default.evaluateInteger",
            &[WasmType::I32],                // expression_ptr (function or constant)
            Some(WasmType::I32),             // evaluated value
            &[WasmType::I32, WasmType::I32], // expr_type, result
            self.generate_evaluate_integer_default(),
        )?;

        // Evaluate default expression (number)
        register_stdlib_function_with_locals(
            codegen,
            "default.evaluateNumber",
            &[WasmType::I32],                // expression_ptr
            Some(WasmType::F64),             // evaluated value
            &[WasmType::I32, WasmType::F64], // expr_type, result
            self.generate_evaluate_number_default(),
        )?;

        // Evaluate default expression (string)
        register_stdlib_function_with_locals(
            codegen,
            "default.evaluateString",
            &[WasmType::I32],                // expression_ptr
            Some(WasmType::I32),             // evaluated string pointer
            &[WasmType::I32, WasmType::I32], // expr_type, result_ptr
            self.generate_evaluate_string_default(),
        )?;

        // Evaluate default expression (boolean)
        register_stdlib_function_with_locals(
            codegen,
            "default.evaluateBoolean",
            &[WasmType::I32],                // expression_ptr
            Some(WasmType::I32),             // evaluated value
            &[WasmType::I32, WasmType::I32], // expr_type, result
            self.generate_evaluate_boolean_default(),
        )?;

        // Check if parameter was provided (not using default)
        register_stdlib_function_with_locals(
            codegen,
            "default.wasProvided",
            &[WasmType::I32],    // parameter_info_ptr
            Some(WasmType::I32), // 1 if provided, 0 if using default
            &[WasmType::I32],    // provided_flag
            self.generate_was_provided(),
        )?;

        Ok(())
    }

    /// Register parameter utility functions
    fn register_parameter_utility_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Create parameter definition with default
        register_stdlib_function_with_locals(
            codegen,
            "param.createWithDefault",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // name_ptr, type_id, default_ptr
            Some(WasmType::I32),                            // parameter definition pointer
            &[WasmType::I32],                               // param_ptr
            self.generate_create_param_with_default(),
        )?;

        // Set parameter defaults for function
        register_stdlib_function_with_locals(
            codegen,
            "param.setFunctionDefaults",
            &[WasmType::I32, WasmType::I32], // function_ptr, defaults_array_ptr
            None,                            // void
            &[WasmType::I32, WasmType::I32, WasmType::I32], // param_count, param_index, default_ptr
            self.generate_set_function_defaults(),
        )?;

        // Apply function call with default parameters
        register_stdlib_function_with_locals(
            codegen,
            "param.applyCallDefaults",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // function_ptr, provided_args_ptr, final_args_ptr
            Some(WasmType::I32),                            // final arguments pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // param_count, provided_count, final_count
            self.generate_apply_call_defaults(),
        )?;

        // Count required parameters (no defaults)
        register_stdlib_function_with_locals(
            codegen,
            "param.countRequired",
            &[WasmType::I32],                               // function_ptr
            Some(WasmType::I32),                            // required parameter count
            &[WasmType::I32, WasmType::I32, WasmType::I32], // param_count, required_count, param_index
            self.generate_count_required_params(),
        )?;

        Ok(())
    }

    /// Generate WASM for integer parameter default
    fn generate_integer_param_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: provided_value (0), default_value (1)
            // Local: final_value (2)

            // Use provided value if not zero (assuming 0 means not provided)
            Instruction::LocalGet(0), // provided_value
            Instruction::I32Eqz,      // provided_value == 0
            Instruction::If(BlockType::Result(ValType::I32)),
            // Use default value
            Instruction::LocalGet(1), // default_value
            Instruction::Else,
            // Use provided value
            Instruction::LocalGet(0), // provided_value
            Instruction::End,
        ]
    }

    /// Generate WASM for number parameter default
    fn generate_number_param_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: provided_value (0), default_value (1)
            // Local: final_value (2)

            // Check if provided value is NaN (indicates not provided)
            Instruction::LocalGet(0), // provided_value
            Instruction::LocalGet(0), // provided_value
            Instruction::F64Ne,       // provided_value != provided_value (NaN check)
            Instruction::If(BlockType::Result(ValType::F64)),
            // Use default value (value is NaN)
            Instruction::LocalGet(1), // default_value
            Instruction::Else,
            // Use provided value
            Instruction::LocalGet(0), // provided_value
            Instruction::End,
        ]
    }

    /// Generate WASM for string parameter default
    fn generate_string_param_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: provided_ptr (0), default_ptr (1)
            // Local: final_ptr (2)

            // Use default if provided pointer is null
            Instruction::LocalGet(0), // provided_ptr
            Instruction::I32Eqz,      // provided_ptr == 0 (null)
            Instruction::If(BlockType::Result(ValType::I32)),
            // Use default string
            Instruction::LocalGet(1), // default_ptr
            Instruction::Else,
            // Check if provided string is empty
            Instruction::LocalGet(0), // provided_ptr
            Instruction::Call(self.get_string_length_function_index()),
            Instruction::I32Eqz, // length == 0
            Instruction::If(BlockType::Result(ValType::I32)),
            // Use default for empty string
            Instruction::LocalGet(1), // default_ptr
            Instruction::Else,
            // Use provided string
            Instruction::LocalGet(0), // provided_ptr
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for boolean parameter default
    fn generate_boolean_param_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: provided_value (0), default_value (1)
            // Local: final_value (2)

            // For boolean, we need a special "not provided" value (-1)
            Instruction::LocalGet(0), // provided_value
            Instruction::I32Const(-1),
            Instruction::I32Eq, // provided_value == -1 (not provided)
            Instruction::If(BlockType::Result(ValType::I32)),
            // Use default value
            Instruction::LocalGet(1), // default_value
            Instruction::Else,
            // Use provided value
            Instruction::LocalGet(0), // provided_value
            Instruction::End,
        ]
    }

    /// Generate WASM for list parameter default
    fn generate_list_param_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: provided_ptr (0), default_ptr (1)
            // Local: final_ptr (2)

            // Use default if provided pointer is null
            Instruction::LocalGet(0), // provided_ptr
            Instruction::I32Eqz,      // provided_ptr == 0 (null)
            Instruction::If(BlockType::Result(ValType::I32)),
            // Use default list (create copy if needed)
            Instruction::LocalGet(1), // default_ptr
            Instruction::Call(self.get_list_copy_function_index()),
            Instruction::Else,
            // Use provided list
            Instruction::LocalGet(0), // provided_ptr
            Instruction::End,
        ]
    }

    /// Generate WASM for creating input block with defaults
    fn generate_create_input_with_defaults(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_definition_ptr (0)
            // Locals: input_ptr (1), field_count (2)

            // Get field count from definition
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // field_count
            // Allocate input block structure
            Instruction::LocalGet(2),  // field_count
            Instruction::I32Const(16), // Size per field
            Instruction::I32Mul,
            Instruction::I32Const(32), // Header size
            Instruction::I32Add,
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(1), // input_ptr
            // Initialize input block header
            Instruction::LocalGet(1),
            Instruction::LocalGet(2), // field_count
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize all fields to use defaults
            Instruction::LocalGet(1), // input_ptr
            Instruction::LocalGet(0), // input_definition_ptr
            Instruction::Call(self.get_initialize_input_defaults_function_index()),
            // Return input pointer
            Instruction::LocalGet(1),
        ]
    }

    /// Generate WASM for applying defaults to input block
    fn generate_apply_input_defaults(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_block_ptr (0), defaults_definition_ptr (1)
            // Locals: field_index (2), field_ptr (3), default_ptr (4)

            // Get field count
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(2), // field_index = 0
            // Loop through all fields (simplified - would implement proper loop)
            Instruction::LocalGet(0), // input_block_ptr
            Instruction::LocalGet(1), // defaults_definition_ptr
            Instruction::Call(self.get_apply_defaults_loop_function_index()),
        ]
    }

    /// Generate WASM for getting field with default fallback
    fn generate_get_field_with_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_block_ptr (0), field_name_ptr (1), default_value_ptr (2)
            // Locals: field_ptr (3), default_applied (4)

            // Find field in input block
            Instruction::LocalGet(0), // input_block_ptr
            Instruction::LocalGet(1), // field_name_ptr
            Instruction::Call(self.get_find_input_field_function_index()),
            Instruction::LocalSet(3), // field_ptr
            // Check if field exists and has value
            Instruction::LocalGet(3), // field_ptr
            Instruction::I32Eqz,      // field_ptr == 0 (not found)
            Instruction::If(BlockType::Result(ValType::I32)),
            // Field not found - use default
            Instruction::LocalGet(2), // default_value_ptr
            Instruction::Else,
            // Field found - check if it has value
            Instruction::LocalGet(3), // field_ptr
            Instruction::I32Load(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // Load value pointer
            Instruction::I32Eqz,      // value_ptr == 0 (no value)
            Instruction::If(BlockType::Result(ValType::I32)),
            // No value - use default
            Instruction::LocalGet(2), // default_value_ptr
            Instruction::Else,
            // Has value - use it
            Instruction::LocalGet(3), // field_ptr
            Instruction::I32Load(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // Load value pointer
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for validating input with defaults
    fn generate_validate_input_with_defaults(&self) -> Vec<Instruction> {
        vec![
            // Parameters: input_block_ptr (0)
            // Locals: field_count (1), valid_count (2), is_valid (3)

            // Get field count
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // field_count
            // Count valid fields (including defaults)
            Instruction::LocalGet(0), // input_block_ptr
            Instruction::Call(self.get_count_valid_fields_function_index()),
            Instruction::LocalSet(2), // valid_count
            // Check if all required fields are valid
            Instruction::LocalGet(2), // valid_count
            Instruction::LocalGet(1), // field_count
            Instruction::I32GeU,      // valid_count >= field_count
        ]
    }

    /// Generate WASM for evaluating integer default expression
    fn generate_evaluate_integer_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expression_ptr (0)
            // Locals: expr_type (1), result (2)

            // Load expression type
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // expr_type
            // Switch on expression type
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // Constant expression
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Load constant value
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            Instruction::LocalGet(1),
            Instruction::I32Const(2), // Function call expression
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Call function to get default value
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // Function index
            Instruction::CallIndirect {
                ty: 0, // Function type for () -> i32
                table: 0,
            },
            Instruction::Else,
            // Unknown expression type - return 0
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for evaluating number default expression
    fn generate_evaluate_number_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expression_ptr (0)
            // Locals: expr_type (1), result (2)

            // Load expression type
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // expr_type
            // Switch on expression type
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // Constant expression
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::F64)),
            // Load constant value
            Instruction::LocalGet(0),
            Instruction::F64Load(MemArg {
                offset: 4,
                align: 3,
                memory_index: 0,
            }),
            Instruction::Else,
            Instruction::LocalGet(1),
            Instruction::I32Const(2), // Function call expression
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::F64)),
            // Call function to get default value
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // Function index
            Instruction::CallIndirect {
                ty: 1, // Function type for () -> f64
                table: 0,
            },
            Instruction::Else,
            // Unknown expression type - return 0.0
            Instruction::F64Const(0.0),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for evaluating string default expression
    fn generate_evaluate_string_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expression_ptr (0)
            // Locals: expr_type (1), result_ptr (2)

            // Load expression type
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // expr_type
            // Switch on expression type
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // Constant expression
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Load constant string pointer
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            Instruction::LocalGet(1),
            Instruction::I32Const(2), // Function call expression
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Call function to get default string
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // Function index
            Instruction::CallIndirect {
                ty: 2, // Function type for () -> i32 (string pointer)
                table: 0,
            },
            Instruction::Else,
            // Unknown expression type - return empty string
            Instruction::Call(self.get_empty_string_function_index()),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for evaluating boolean default expression
    fn generate_evaluate_boolean_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expression_ptr (0)
            // Locals: expr_type (1), result (2)

            // Load expression type
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // expr_type
            // Switch on expression type
            Instruction::LocalGet(1),
            Instruction::I32Const(1), // Constant expression
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Load constant value
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            Instruction::LocalGet(1),
            Instruction::I32Const(2), // Function call expression
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Call function to get default value
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // Function index
            Instruction::CallIndirect {
                ty: 0, // Function type for () -> i32
                table: 0,
            },
            Instruction::Else,
            // Unknown expression type - return false
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for checking if parameter was provided
    fn generate_was_provided(&self) -> Vec<Instruction> {
        vec![
            // Parameters: parameter_info_ptr (0)
            // Local: provided_flag (1)

            // Load provided flag from parameter info
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }), // Provided flag at offset 12
        ]
    }

    /// Generate WASM for creating parameter with default
    fn generate_create_param_with_default(&self) -> Vec<Instruction> {
        vec![
            // Parameters: name_ptr (0), type_id (1), default_ptr (2)
            // Local: param_ptr (3)

            // Allocate parameter definition structure (20 bytes)
            Instruction::I32Const(20),
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(3), // param_ptr
            // Store parameter name
            Instruction::LocalGet(3),
            Instruction::LocalGet(0), // name_ptr
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Store type ID
            Instruction::LocalGet(3),
            Instruction::LocalGet(1), // type_id
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            // Store default value pointer
            Instruction::LocalGet(3),
            Instruction::LocalGet(2), // default_ptr
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Mark as having default (1 = has default, 0 = required)
            Instruction::LocalGet(3),
            Instruction::I32Const(1),
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            // Return parameter pointer
            Instruction::LocalGet(3),
        ]
    }

    /// Generate WASM for setting function defaults
    fn generate_set_function_defaults(&self) -> Vec<Instruction> {
        vec![
            // Parameters: function_ptr (0), defaults_array_ptr (1)
            // Locals: param_count (2), param_index (3), default_ptr (4)

            // Get parameter count from function
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }), // Param count at offset 16
            Instruction::LocalSet(2), // param_count
            // Apply defaults to function (simplified loop)
            Instruction::LocalGet(0), // function_ptr
            Instruction::LocalGet(1), // defaults_array_ptr
            Instruction::LocalGet(2), // param_count
            Instruction::Call(self.get_apply_function_defaults_function_index()),
        ]
    }

    /// Generate WASM for applying call defaults
    fn generate_apply_call_defaults(&self) -> Vec<Instruction> {
        vec![
            // Parameters: function_ptr (0), provided_args_ptr (1), final_args_ptr (2)
            // Locals: param_count (3), provided_count (4), final_count (5)

            // Get parameter count from function
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(3), // param_count
            // Get provided argument count
            Instruction::LocalGet(1),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(4), // provided_count
            // Apply defaults for missing arguments
            Instruction::LocalGet(0), // function_ptr
            Instruction::LocalGet(1), // provided_args_ptr
            Instruction::LocalGet(2), // final_args_ptr
            Instruction::LocalGet(3), // param_count
            Instruction::LocalGet(4), // provided_count
            Instruction::Call(self.get_merge_args_with_defaults_function_index()),
            // Return final arguments pointer
            Instruction::LocalGet(2),
        ]
    }

    /// Generate WASM for counting required parameters
    fn generate_count_required_params(&self) -> Vec<Instruction> {
        vec![
            // Parameters: function_ptr (0)
            // Locals: param_count (1), required_count (2), param_index (3)

            // Get parameter count
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // param_count
            // Count required parameters (no defaults)
            Instruction::LocalGet(0), // function_ptr
            Instruction::LocalGet(1), // param_count
            Instruction::Call(self.get_count_required_function_index()),
        ]
    }

    // Helper function indices - Default Parameters uses 1100-1130 range
    fn get_string_length_function_index(&self) -> u32 {
        1100
    }
    fn get_list_copy_function_index(&self) -> u32 {
        1101
    }
    fn get_allocate_function_index(&self) -> u32 {
        1102
    }
    fn get_initialize_input_defaults_function_index(&self) -> u32 {
        1103
    }
    fn get_apply_defaults_loop_function_index(&self) -> u32 {
        1104
    }
    fn get_find_input_field_function_index(&self) -> u32 {
        1105
    }
    fn get_count_valid_fields_function_index(&self) -> u32 {
        1106
    }
    fn get_empty_string_function_index(&self) -> u32 {
        1107
    }
    fn get_apply_function_defaults_function_index(&self) -> u32 {
        1108
    }
    fn get_merge_args_with_defaults_function_index(&self) -> u32 {
        1109
    }
    fn get_count_required_function_index(&self) -> u32 {
        1110
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_parameter_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let default_manager = DefaultParameterManager::new(memory_manager.clone());

        // Test that manager is created successfully
        assert_eq!(
            default_manager
                .memory_manager
                .borrow()
                .get_total_allocated(),
            0
        );
    }

    #[test]
    fn test_parameter_default_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let default_manager = DefaultParameterManager::new(memory_manager);

        let int_default = default_manager.generate_integer_param_default();
        assert!(!int_default.is_empty());
        // Should contain conditional logic for checking if value was provided
        assert!(int_default
            .iter()
            .any(|inst| matches!(inst, Instruction::If(_))));

        let num_default = default_manager.generate_number_param_default();
        assert!(!num_default.is_empty());
        // Should use NaN check for numbers
        assert!(num_default
            .iter()
            .any(|inst| matches!(inst, Instruction::F64Ne)));

        let str_default = default_manager.generate_string_param_default();
        assert!(!str_default.is_empty());
        // Should contain nested If blocks for null and empty checks
        let if_count = str_default
            .iter()
            .filter(|inst| matches!(inst, Instruction::If(_)))
            .count();
        assert!(if_count >= 2);

        let bool_default = default_manager.generate_boolean_param_default();
        assert!(!bool_default.is_empty());
        // Should check for special "not provided" value (-1)
        assert!(bool_default
            .iter()
            .any(|inst| matches!(inst, Instruction::I32Const(-1))));
    }

    #[test]
    fn test_input_block_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let default_manager = DefaultParameterManager::new(memory_manager);

        let create_input = default_manager.generate_create_input_with_defaults();
        assert!(!create_input.is_empty());
        // Should allocate memory for input block
        assert!(create_input
            .iter()
            .any(|inst| matches!(inst, Instruction::Call(_))));

        let get_field = default_manager.generate_get_field_with_default();
        assert!(!get_field.is_empty());
        // Should contain nested conditional logic for field existence and values
        let if_count = get_field
            .iter()
            .filter(|inst| matches!(inst, Instruction::If(_)))
            .count();
        assert!(if_count >= 2);

        let validate = default_manager.generate_validate_input_with_defaults();
        assert!(!validate.is_empty());
        // Should end with comparison for validation
        assert!(matches!(validate[validate.len() - 1], Instruction::I32GeU));
    }

    #[test]
    fn test_default_evaluation_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let default_manager = DefaultParameterManager::new(memory_manager);

        let eval_int = default_manager.generate_evaluate_integer_default();
        assert!(!eval_int.is_empty());
        // Should contain CallIndirect for function expressions
        assert!(eval_int
            .iter()
            .any(|inst| matches!(inst, Instruction::CallIndirect { .. })));

        let eval_num = default_manager.generate_evaluate_number_default();
        assert!(!eval_num.is_empty());
        // Should handle F64 values
        assert!(eval_num
            .iter()
            .any(|inst| matches!(inst, Instruction::F64Load(_))));
        assert!(eval_num
            .iter()
            .any(|inst| matches!(inst, Instruction::F64Const(_))));

        let eval_str = default_manager.generate_evaluate_string_default();
        assert!(!eval_str.is_empty());
        // Should call empty string function as fallback
        let call_count = eval_str
            .iter()
            .filter(|inst| matches!(inst, Instruction::Call(_)))
            .count();
        assert!(call_count >= 1);

        let was_provided = default_manager.generate_was_provided();
        assert!(!was_provided.is_empty());
        // Should be simple memory load
        assert_eq!(was_provided.len(), 2); // LocalGet + I32Load
    }

    #[test]
    fn test_parameter_utility_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let default_manager = DefaultParameterManager::new(memory_manager);

        let create_param = default_manager.generate_create_param_with_default();
        assert!(!create_param.is_empty());
        // Should store all parameter information
        let store_count = create_param
            .iter()
            .filter(|inst| matches!(inst, Instruction::I32Store(_)))
            .count();
        assert_eq!(store_count, 4); // name, type, default, has_default flag

        let apply_call = default_manager.generate_apply_call_defaults();
        assert!(!apply_call.is_empty());
        // Should load parameter counts and call merge function
        let load_count = apply_call
            .iter()
            .filter(|inst| matches!(inst, Instruction::I32Load(_)))
            .count();
        assert_eq!(load_count, 2); // param_count and provided_count

        let count_required = default_manager.generate_count_required_params();
        assert!(!count_required.is_empty());
        // Should call counting helper function
        assert!(matches!(
            count_required[count_required.len() - 1],
            Instruction::Call(_)
        ));
    }
}
