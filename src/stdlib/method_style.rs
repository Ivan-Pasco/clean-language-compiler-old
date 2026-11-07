use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Method-style syntax implementation for Clean Language
/// Enables value.method() calls instead of only function(value) calls
#[derive(Default)]
pub struct MethodStyleManager;

impl MethodStyleManager {
    pub fn new() -> Self {
        Self
    }

    /// Register all method-style functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Register all method-style functions systematically
        self.register_utility_methods(codegen)?;
        self.register_conversion_methods(codegen)?;
        // All method-style functions registered successfully
        Ok(())
    }

    /// Register utility functions that work on any value
    fn register_utility_methods(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Register utility methods for all types
        let types = ["integer", "number", "string", "boolean", "value"];

        for type_name in &types {
            // NOTE: Skip string.length - it has a dedicated native implementation
            // The polymorphic version expects boxed values with type IDs at offset 8,
            // but raw string literals have [length|data] format with no type ID
            // Registering a polymorphic string.length overwrites the correct native function
            if *type_name != "string" {
                // Length methods for non-string types
                let length_name = format!("{type_name}.length");
                register_stdlib_function(
                    codegen,
                    &length_name,
                    &[WasmType::I32],    // value pointer
                    Some(WasmType::I32), // length
                    self.generate_value_length(),
                )?;
            }

            // Check if value is defined (not null/undefined)
            register_stdlib_function(
                codegen,
                &format!("{type_name}.isDefined"),
                &[WasmType::I32],    // value pointer
                Some(WasmType::I32), // boolean
                self.generate_value_is_defined(),
            )?;

            register_stdlib_function(
                codegen,
                &format!("{type_name}.isNotDefined"),
                &[WasmType::I32],    // value pointer
                Some(WasmType::I32), // boolean
                self.generate_value_is_not_defined(),
            )?;

            // Check if value is empty
            register_stdlib_function(
                codegen,
                &format!("{type_name}.isEmpty"),
                &[WasmType::I32],    // value pointer
                Some(WasmType::I32), // boolean
                self.generate_value_is_empty(),
            )?;

            register_stdlib_function(
                codegen,
                &format!("{type_name}.isNotEmpty"),
                &[WasmType::I32],    // value pointer
                Some(WasmType::I32), // boolean
                self.generate_value_is_not_empty(),
            )?;
        }

        Ok(())
    }

    /// Register type conversion methods (stdlib-reference §3).
    ///
    /// Each body works on the receiver's own representation: integers and
    /// booleans arrive as i32 values, numbers as f64. `integer.toString`,
    /// `boolean.toString`, `string.toInteger` and `string.toNumber` are
    /// aliases to the native converters, registered with the memory
    /// operations.
    fn register_conversion_methods(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        use WasmType::{F64, I32};
        let conversions: [(&str, WasmType, WasmType, Vec<Instruction<'static>>); 9] = [
            // A string is already a string.
            ("string.toString", I32, I32, vec![Instruction::LocalGet(0)]),
            (
                "integer.toInteger",
                I32,
                I32,
                vec![Instruction::LocalGet(0)],
            ),
            (
                "integer.toNumber",
                I32,
                F64,
                vec![Instruction::LocalGet(0), Instruction::F64ConvertI32S],
            ),
            ("integer.toBoolean", I32, I32, Self::nonzero_i32()),
            // Truncates toward zero; NaN and out-of-range values saturate.
            (
                "number.toInteger",
                F64,
                I32,
                vec![Instruction::LocalGet(0), Instruction::I32TruncSatF64S],
            ),
            ("number.toNumber", F64, F64, vec![Instruction::LocalGet(0)]),
            ("boolean.toBoolean", I32, I32, Self::nonzero_i32()),
            ("boolean.toInteger", I32, I32, Self::nonzero_i32()),
            ("boolean.toNumber", I32, F64, {
                let mut body = Self::nonzero_i32();
                body.push(Instruction::F64ConvertI32U);
                body
            }),
        ];
        for (name, param, result, body) in conversions {
            register_stdlib_function(codegen, name, &[param], Some(result), body)?;
        }

        // string.toBoolean: "true" and "1" are true; "false", "0" and any
        // other text are false (stdlib-reference.md, conversion error semantics).
        register_stdlib_function(
            codegen,
            "string.toBoolean",
            &[WasmType::I32],    // string pointer (length-prefixed)
            Some(WasmType::I32), // boolean value
            self.generate_string_to_boolean(),
        )?;

        // number.toString is the host float formatter.
        if let Some(float_to_string) = codegen.get_function_index("float_to_string") {
            codegen.add_function_alias("number.toString", float_to_string);
        }

        Ok(())
    }

    /// `local 0 != 0` as an i32 boolean.
    fn nonzero_i32() -> Vec<Instruction<'static>> {
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Const(0),
            Instruction::I32Ne,
        ]
    }

    /// Generate WASM for value.length() method
    fn generate_value_length(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: length of value

            // Load type ID from value header
            Instruction::LocalGet(0), // value_ptr
            Instruction::I32Load(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }), // Type ID at offset 8
            Instruction::LocalSet(1), // type_id
            // Check if it's a string (type ID 3)
            Instruction::LocalGet(1),
            Instruction::I32Const(3), // String type ID
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // String length - load from string header
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Check if it's a list (type ID 4)
            Instruction::LocalGet(1),
            Instruction::I32Const(4), // List type ID
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // List length - load from list header
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Default - return 1 for simple values
            Instruction::I32Const(1),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Generate WASM for value.isDefined() method
    /// Handles boxed Any values from JSON parsing
    /// Memory layout: [type_tag: i32][value: f64 or i32 pointer]
    fn generate_value_is_defined(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: 1 if defined (type tag != 0), 0 if null (type tag == 0)
            //
            // Boxed Any format from JSON:
            // - Offset 0: type tag (i32)
            //   - 0 = null → isNotDefined
            //   - 1 = false → isDefined
            //   - 2 = true → isDefined
            //   - 3 = number → isDefined
            //   - 4 = string → isDefined
            //   - 5 = array → isDefined
            //   - 6 = object → isDefined
            //
            // Check for null pointer first
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0), // null pointer = not defined
            Instruction::Else,
            // Load type tag from offset 0
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Check if type tag is NOT 0 (null)
            Instruction::I32Const(0),
            Instruction::I32Ne, // type_tag != 0 means defined
            Instruction::End,
        ]
    }

    /// Generate WASM for value.isNotDefined() method
    /// Handles boxed Any values from JSON parsing
    /// Memory layout: [type_tag: i32][value: f64 or i32 pointer]
    fn generate_value_is_not_defined(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: 1 if null (type tag == 0), 0 if defined
            //
            // Check for null pointer first
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(1), // null pointer = not defined
            Instruction::Else,
            // Load type tag from offset 0
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Check if type tag is 0 (null)
            Instruction::I32Eqz, // type_tag == 0 means not defined
            Instruction::End,
        ]
    }

    /// Generate WASM for value.isEmpty() method
    fn generate_value_is_empty(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: 1 if empty, 0 if not empty

            // Check if pointer is null
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(1), // Null is empty
            Instruction::Else,
            // Check length using direct memory access (length is at offset 0)
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0, // Length field is at offset 0 in Clean Language objects
                align: 2,  // 4-byte alignment for i32
                memory_index: 0,
            }),
            Instruction::I32Eqz, // Check if length is 0
            Instruction::End,
        ]
    }

    /// Generate WASM for value.isNotEmpty() method
    fn generate_value_is_not_empty(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value_ptr (0)
            // Returns: 1 if not empty, 0 if empty

            // Check if pointer is null
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0), // Null is empty, so not-empty is false
            Instruction::Else,
            // Check length using direct memory access and compare with 0
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                offset: 0, // Length field is at offset 0 in Clean Language objects
                align: 2,  // 4-byte alignment for i32
                memory_index: 0,
            }),
            Instruction::I32Const(0),
            Instruction::I32GtU, // Check if length > 0
            Instruction::End,
        ]
    }

    /// Generate WASM for string.toBoolean(): compares the length-prefixed
    /// text with "true" (4 bytes) and "1" (1 byte).
    fn generate_string_to_boolean(&self) -> Vec<Instruction> {
        let len_at = MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        };
        let bytes_at = MemArg {
            offset: 4,
            align: 0,
            memory_index: 0,
        };
        // Little-endian i32 of the bytes "true".
        let true_bytes = i32::from_le_bytes(*b"true");
        vec![
            Instruction::LocalGet(0),
            Instruction::I32Eqz,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::I32Const(0),
            Instruction::Else,
            Instruction::LocalGet(0),
            Instruction::I32Load(len_at),
            Instruction::I32Const(4),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load(bytes_at),
            Instruction::I32Const(true_bytes),
            Instruction::I32Eq,
            Instruction::Else,
            Instruction::LocalGet(0),
            Instruction::I32Load(len_at),
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            Instruction::LocalGet(0),
            Instruction::I32Load8U(bytes_at),
            Instruction::I32Const(b'1' as i32),
            Instruction::I32Eq,
            Instruction::Else,
            Instruction::I32Const(0),
            Instruction::End,
            Instruction::End,
            Instruction::End,
        ]
    }
}
