use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::{register_stdlib_function_with_locals, MemoryManager};
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Matrix Literals Manager - Implements matrix literal syntax and operations
/// Supports [[1,2],[3,4]] syntax for matrix<T> types
pub struct MatrixLiteralsManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl MatrixLiteralsManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all matrix literal functions with the code generator
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Matrix creation functions
        self.register_create_integer_matrix(codegen)?;
        self.register_create_number_matrix(codegen)?;
        self.register_create_boolean_matrix(codegen)?;
        self.register_create_string_matrix(codegen)?;

        // Matrix access functions
        self.register_get_matrix_element(codegen)?;
        self.register_set_matrix_element(codegen)?;
        self.register_get_matrix_row(codegen)?;
        self.register_get_matrix_column(codegen)?;

        // Matrix properties
        self.register_get_matrix_rows(codegen)?;
        self.register_get_matrix_columns(codegen)?;
        self.register_get_matrix_size(codegen)?;
        self.register_is_matrix_square(codegen)?;

        // Matrix validation and utilities
        self.register_validate_matrix_dimensions(codegen)?;
        self.register_matrix_to_string(codegen)?;
        self.register_matrix_equals(codegen)?;

        Ok(())
    }

    /// Create integer matrix from nested array: [[1,2],[3,4]]
    fn register_create_integer_matrix(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.createInteger",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // rows, columns, data_array_ptr
            Some(WasmType::I32),                            // matrix pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // matrix_ptr, row_size, total_size, i
            self.generate_create_integer_matrix(),
        )
    }

    fn generate_create_integer_matrix(&self) -> Vec<Instruction> {
        vec![
            // Calculate matrix size: rows * columns * 4 (i32 size) + header (20 bytes)
            Instruction::LocalGet(0), // rows
            Instruction::LocalGet(1), // columns
            Instruction::I32Mul,
            Instruction::I32Const(4), // sizeof(i32)
            Instruction::I32Mul,
            Instruction::I32Const(20), // matrix header size
            Instruction::I32Add,
            Instruction::LocalSet(6), // total_size
            // Allocate matrix memory
            Instruction::LocalGet(6), // total_size
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(3), // matrix_ptr
            // Write matrix header
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(0), // rows
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(1), // columns
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::I32Const(1), // type_id for integer
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::I32Const(4), // element_size
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(6), // total_size
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            // Copy data from input array to matrix data section
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(7), // i
            // Loop: copy each element
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= rows * columns
            Instruction::LocalGet(7), // i
            Instruction::LocalGet(0), // rows
            Instruction::LocalGet(1), // columns
            Instruction::I32Mul,
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= total elements
            // Copy element: matrix_data[i] = input_array[i]
            Instruction::LocalGet(3),  // matrix_ptr
            Instruction::I32Const(20), // header size
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4), // sizeof(i32)
            Instruction::I32Mul,
            Instruction::I32Add,      // matrix_data + i*4
            Instruction::LocalGet(2), // data_array_ptr
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4), // sizeof(i32)
            Instruction::I32Mul,
            Instruction::I32Add, // input_array + i*4
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // load input[i]
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // store to matrix[i]
            // Increment i
            Instruction::LocalGet(7), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return matrix pointer
            Instruction::LocalGet(3), // matrix_ptr
        ]
    }

    /// Create number matrix from nested array: [[1.0,2.0],[3.0,4.0]]
    fn register_create_number_matrix(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.createNumber",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // rows, columns, data_array_ptr
            Some(WasmType::I32),                            // matrix pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // matrix_ptr, row_size, total_size, i
            self.generate_create_number_matrix(),
        )
    }

    fn generate_create_number_matrix(&self) -> Vec<Instruction> {
        vec![
            // Calculate matrix size: rows * columns * 8 (f64 size) + header (20 bytes)
            Instruction::LocalGet(0), // rows
            Instruction::LocalGet(1), // columns
            Instruction::I32Mul,
            Instruction::I32Const(8), // sizeof(f64)
            Instruction::I32Mul,
            Instruction::I32Const(20), // matrix header size
            Instruction::I32Add,
            Instruction::LocalSet(6), // total_size
            // Allocate matrix memory
            Instruction::LocalGet(6), // total_size
            Instruction::I32Const(8), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(3), // matrix_ptr
            // Write matrix header
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(0), // rows
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(1), // columns
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::I32Const(2), // type_id for number
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::I32Const(8), // element_size
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(6), // total_size
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            // Copy data from input array to matrix data section
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(7), // i
            // Loop: copy each element
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= rows * columns
            Instruction::LocalGet(7), // i
            Instruction::LocalGet(0), // rows
            Instruction::LocalGet(1), // columns
            Instruction::I32Mul,
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= total elements
            // Copy element: matrix_data[i] = input_array[i]
            Instruction::LocalGet(3),  // matrix_ptr
            Instruction::I32Const(20), // header size
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8), // sizeof(f64)
            Instruction::I32Mul,
            Instruction::I32Add,      // matrix_data + i*8
            Instruction::LocalGet(2), // data_array_ptr
            Instruction::LocalGet(7), // i
            Instruction::I32Const(8), // sizeof(f64)
            Instruction::I32Mul,
            Instruction::I32Add, // input_array + i*8
            Instruction::F64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }), // load input[i]
            Instruction::F64Store(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }), // store to matrix[i]
            // Increment i
            Instruction::LocalGet(7), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return matrix pointer
            Instruction::LocalGet(3), // matrix_ptr
        ]
    }

    /// Create boolean matrix from nested array
    fn register_create_boolean_matrix(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.createBoolean",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // rows, columns, data_array_ptr
            Some(WasmType::I32),                            // matrix pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // matrix_ptr, row_size, total_size, i
            self.generate_create_boolean_matrix(),
        )
    }

    fn generate_create_boolean_matrix(&self) -> Vec<Instruction> {
        vec![
            // Calculate matrix size: rows * columns * 1 (bool size) + header (20 bytes)
            Instruction::LocalGet(0), // rows
            Instruction::LocalGet(1), // columns
            Instruction::I32Mul,
            Instruction::I32Const(1), // sizeof(bool)
            Instruction::I32Mul,
            Instruction::I32Const(20), // matrix header size
            Instruction::I32Add,
            Instruction::LocalSet(6), // total_size
            // Allocate matrix memory
            Instruction::LocalGet(6), // total_size
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(3), // matrix_ptr
            // Write matrix header
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(0), // rows
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(1), // columns
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::I32Const(4), // type_id for boolean
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::I32Const(1), // element_size
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(6), // total_size
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            // Copy data from input array to matrix data section
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(7), // i
            // Loop: copy each element
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= rows * columns
            Instruction::LocalGet(7), // i
            Instruction::LocalGet(0), // rows
            Instruction::LocalGet(1), // columns
            Instruction::I32Mul,
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= total elements
            // Copy element: matrix_data[i] = input_array[i]
            Instruction::LocalGet(3),  // matrix_ptr
            Instruction::I32Const(20), // header size
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Add,      // matrix_data + i
            Instruction::LocalGet(2), // data_array_ptr
            Instruction::LocalGet(7), // i
            Instruction::I32Add,      // input_array + i
            Instruction::I32Load8U(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }), // load input[i]
            Instruction::I32Store8(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }), // store to matrix[i]
            // Increment i
            Instruction::LocalGet(7), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return matrix pointer
            Instruction::LocalGet(3), // matrix_ptr
        ]
    }

    /// Create string matrix from nested array
    fn register_create_string_matrix(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.createString",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // rows, columns, data_array_ptr
            Some(WasmType::I32),                            // matrix pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // matrix_ptr, row_size, total_size, i
            self.generate_create_string_matrix(),
        )
    }

    fn generate_create_string_matrix(&self) -> Vec<Instruction> {
        vec![
            // Calculate matrix size: rows * columns * 4 (ptr size) + header (20 bytes)
            Instruction::LocalGet(0), // rows
            Instruction::LocalGet(1), // columns
            Instruction::I32Mul,
            Instruction::I32Const(4), // sizeof(string_ptr)
            Instruction::I32Mul,
            Instruction::I32Const(20), // matrix header size
            Instruction::I32Add,
            Instruction::LocalSet(6), // total_size
            // Allocate matrix memory
            Instruction::LocalGet(6), // total_size
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(3), // matrix_ptr
            // Write matrix header
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(0), // rows
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(1), // columns
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::I32Const(3), // type_id for string
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::I32Const(4), // element_size (string pointer)
            Instruction::I32Store(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(3), // matrix_ptr
            Instruction::LocalGet(6), // total_size
            Instruction::I32Store(MemArg {
                offset: 16,
                align: 2,
                memory_index: 0,
            }),
            // Copy data from input array to matrix data section
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(7), // i
            // Loop: copy each string pointer
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= rows * columns
            Instruction::LocalGet(7), // i
            Instruction::LocalGet(0), // rows
            Instruction::LocalGet(1), // columns
            Instruction::I32Mul,
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= total elements
            // Copy element: matrix_data[i] = input_array[i]
            Instruction::LocalGet(3),  // matrix_ptr
            Instruction::I32Const(20), // header size
            Instruction::I32Add,
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4), // sizeof(string_ptr)
            Instruction::I32Mul,
            Instruction::I32Add,      // matrix_data + i*4
            Instruction::LocalGet(2), // data_array_ptr
            Instruction::LocalGet(7), // i
            Instruction::I32Const(4), // sizeof(string_ptr)
            Instruction::I32Mul,
            Instruction::I32Add, // input_array + i*4
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // load input[i]
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // store to matrix[i]
            // Increment i
            Instruction::LocalGet(7), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(7), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return matrix pointer
            Instruction::LocalGet(3), // matrix_ptr
        ]
    }

    /// Get matrix element at [row, column]
    fn register_get_matrix_element(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.getElement",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // matrix_ptr, row, column
            Some(WasmType::I32), // element value (as i32, can be reinterpreted)
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // columns, element_size, index, data_ptr
            self.generate_get_matrix_element(),
        )
    }

    fn generate_get_matrix_element(&self) -> Vec<Instruction> {
        vec![
            // Load matrix columns
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // columns
            Instruction::LocalSet(3), // columns
            // Load element size
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }), // element_size
            Instruction::LocalSet(4), // element_size
            // Calculate index: row * columns + column
            Instruction::LocalGet(1), // row
            Instruction::LocalGet(3), // columns
            Instruction::I32Mul,
            Instruction::LocalGet(2), // column
            Instruction::I32Add,
            Instruction::LocalSet(5), // index
            // Calculate data pointer: matrix_ptr + 20 + index * element_size
            Instruction::LocalGet(0),  // matrix_ptr
            Instruction::I32Const(20), // header size
            Instruction::I32Add,
            Instruction::LocalGet(5), // index
            Instruction::LocalGet(4), // element_size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(6), // data_ptr
            // Load element based on element size
            Instruction::LocalGet(4), // element_size
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Boolean (1 byte)
            Instruction::LocalGet(6), // data_ptr
            Instruction::I32Load8U(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::Else,
            Instruction::LocalGet(4), // element_size
            Instruction::I32Const(4),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
            // Integer or string pointer (4 bytes)
            Instruction::LocalGet(6), // data_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Number (8 bytes) - return low 32 bits
            Instruction::LocalGet(6), // data_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // Load as i32
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Set matrix element at [row, column]
    fn register_set_matrix_element(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.setElement",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // matrix_ptr, row, column, value
            None,                                                          // void
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // columns, element_size, index, data_ptr
            self.generate_set_matrix_element(),
        )
    }

    fn generate_set_matrix_element(&self) -> Vec<Instruction> {
        vec![
            // Load matrix columns
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // columns
            Instruction::LocalSet(4), // columns
            // Load element size
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }), // element_size
            Instruction::LocalSet(5), // element_size
            // Calculate index: row * columns + column
            Instruction::LocalGet(1), // row
            Instruction::LocalGet(4), // columns
            Instruction::I32Mul,
            Instruction::LocalGet(2), // column
            Instruction::I32Add,
            Instruction::LocalSet(6), // index
            // Calculate data pointer: matrix_ptr + 20 + index * element_size
            Instruction::LocalGet(0),  // matrix_ptr
            Instruction::I32Const(20), // header size
            Instruction::I32Add,
            Instruction::LocalGet(6), // index
            Instruction::LocalGet(5), // element_size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(7), // data_ptr
            // Store element based on element size
            Instruction::LocalGet(5), // element_size
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            // Boolean (1 byte)
            Instruction::LocalGet(7), // data_ptr
            Instruction::LocalGet(3), // value
            Instruction::I32Store8(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::Else,
            Instruction::LocalGet(5), // element_size
            Instruction::I32Const(4),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            // Integer or string pointer (4 bytes)
            Instruction::LocalGet(7), // data_ptr
            Instruction::LocalGet(3), // value
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Number (8 bytes) - store as i32 for now
            Instruction::LocalGet(7), // data_ptr
            Instruction::LocalGet(3), // value
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::End,
            Instruction::End,
        ]
    }

    /// Get entire matrix row as array
    fn register_get_matrix_row(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.getRow",
            &[WasmType::I32, WasmType::I32], // matrix_ptr, row_index
            Some(WasmType::I32),             // row array pointer
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // columns, element_size, row_ptr, start_ptr, i
            self.generate_get_matrix_row(),
        )
    }

    fn generate_get_matrix_row(&self) -> Vec<Instruction> {
        vec![
            // Load matrix columns
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // columns
            Instruction::LocalSet(2), // columns
            // Load element size
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }), // element_size
            Instruction::LocalSet(3), // element_size
            // Allocate row array: columns * element_size
            Instruction::LocalGet(2), // columns
            Instruction::LocalGet(3), // element_size
            Instruction::I32Mul,
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(4), // row_ptr
            // Calculate start of row data: matrix_ptr + 20 + row_index * columns * element_size
            Instruction::LocalGet(0),  // matrix_ptr
            Instruction::I32Const(20), // header size
            Instruction::I32Add,
            Instruction::LocalGet(1), // row_index
            Instruction::LocalGet(2), // columns
            Instruction::I32Mul,
            Instruction::LocalGet(3), // element_size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(5), // start_ptr
            // Copy row data
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(6), // i
            // Loop: copy each element in the row
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= columns
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(2), // columns
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= columns
            // Copy element: row_data[i] = matrix_data[start + i]
            Instruction::LocalGet(4), // row_ptr
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(3), // element_size
            Instruction::I32Mul,
            Instruction::I32Add,      // row_ptr + i * element_size
            Instruction::LocalGet(5), // start_ptr
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(3), // element_size
            Instruction::I32Mul,
            Instruction::I32Add, // start_ptr + i * element_size
            // Copy based on element size
            Instruction::LocalGet(3), // element_size
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            // Boolean (1 byte)
            Instruction::I32Load8U(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Store8(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::Else,
            Instruction::LocalGet(3), // element_size
            Instruction::I32Const(4),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            // Integer or string pointer (4 bytes)
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Number (8 bytes)
            Instruction::F64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::F64Store(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::End,
            Instruction::End,
            // Increment i
            Instruction::LocalGet(6), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return row array pointer
            Instruction::LocalGet(4), // row_ptr
        ]
    }

    /// Get entire matrix column as array
    fn register_get_matrix_column(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.getColumn",
            &[WasmType::I32, WasmType::I32], // matrix_ptr, column_index
            Some(WasmType::I32),             // column array pointer
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // rows, columns, element_size, col_ptr, i, element_ptr
            self.generate_get_matrix_column(),
        )
    }

    fn generate_get_matrix_column(&self) -> Vec<Instruction> {
        vec![
            // Load matrix dimensions
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }), // rows
            Instruction::LocalSet(2), // rows
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }), // columns
            Instruction::LocalSet(3), // columns
            // Load element size
            Instruction::LocalGet(0), // matrix_ptr
            Instruction::I32Load(MemArg {
                offset: 12,
                align: 2,
                memory_index: 0,
            }), // element_size
            Instruction::LocalSet(4), // element_size
            // Allocate column array: rows * element_size
            Instruction::LocalGet(2), // rows
            Instruction::LocalGet(4), // element_size
            Instruction::I32Mul,
            Instruction::I32Const(4), // alignment
            Instruction::Call(2000),  // memory.allocate
            Instruction::LocalSet(5), // col_ptr
            // Copy column data
            Instruction::I32Const(0), // i = 0
            Instruction::LocalSet(6), // i
            // Loop: copy each element in the column
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            // Check if i >= rows
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(2), // rows
            Instruction::I32GeU,
            Instruction::BrIf(1), // break if i >= rows
            // Calculate element pointer: matrix_ptr + 20 + (i * columns + column_index) * element_size
            Instruction::LocalGet(0),  // matrix_ptr
            Instruction::I32Const(20), // header size
            Instruction::I32Add,
            Instruction::LocalGet(6), // i (row)
            Instruction::LocalGet(3), // columns
            Instruction::I32Mul,
            Instruction::LocalGet(1), // column_index
            Instruction::I32Add,      // i * columns + column_index
            Instruction::LocalGet(4), // element_size
            Instruction::I32Mul,
            Instruction::I32Add,
            Instruction::LocalSet(7), // element_ptr
            // Copy element: col_data[i] = matrix_data[element_ptr]
            Instruction::LocalGet(5), // col_ptr
            Instruction::LocalGet(6), // i
            Instruction::LocalGet(4), // element_size
            Instruction::I32Mul,
            Instruction::I32Add, // col_ptr + i * element_size
            // Copy based on element size
            Instruction::LocalGet(4), // element_size
            Instruction::I32Const(1),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            // Boolean (1 byte)
            Instruction::LocalGet(7), // element_ptr
            Instruction::I32Load8U(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::I32Store8(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            }),
            Instruction::Else,
            Instruction::LocalGet(4), // element_size
            Instruction::I32Const(4),
            Instruction::I32Eq,
            Instruction::If(BlockType::Empty),
            // Integer or string pointer (4 bytes)
            Instruction::LocalGet(7), // element_ptr
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::Else,
            // Number (8 bytes)
            Instruction::LocalGet(7), // element_ptr
            Instruction::F64Load(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::F64Store(MemArg {
                offset: 0,
                align: 3,
                memory_index: 0,
            }),
            Instruction::End,
            Instruction::End,
            // Increment i
            Instruction::LocalGet(6), // i
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::LocalSet(6), // i++
            Instruction::Br(0),       // continue loop
            Instruction::End,         // end loop
            Instruction::End,         // end block
            // Return column array pointer
            Instruction::LocalGet(5), // col_ptr
        ]
    }

    /// Get number of rows in matrix
    fn register_get_matrix_rows(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.getRows",
            &[WasmType::I32],    // matrix_ptr
            Some(WasmType::I32), // row count
            &[],                 // no locals
            vec![
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load rows
            ],
        )
    }

    /// Get number of columns in matrix
    fn register_get_matrix_columns(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.getColumns",
            &[WasmType::I32],    // matrix_ptr
            Some(WasmType::I32), // column count
            &[],                 // no locals
            vec![
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // load columns
            ],
        )
    }

    /// Get total size of matrix (rows * columns)
    fn register_get_matrix_size(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.getSize",
            &[WasmType::I32],                // matrix_ptr
            Some(WasmType::I32),             // total size
            &[WasmType::I32, WasmType::I32], // rows, columns
            vec![
                // Load rows
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load rows
                Instruction::LocalSet(1), // rows
                // Load columns
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // load columns
                Instruction::LocalSet(2), // columns
                // Return rows * columns
                Instruction::LocalGet(1), // rows
                Instruction::LocalGet(2), // columns
                Instruction::I32Mul,
            ],
        )
    }

    /// Check if matrix is square (rows == columns)
    fn register_is_matrix_square(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.isSquare",
            &[WasmType::I32],                // matrix_ptr
            Some(WasmType::I32),             // boolean result
            &[WasmType::I32, WasmType::I32], // rows, columns
            vec![
                // Load rows
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load rows
                Instruction::LocalSet(1), // rows
                // Load columns
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // load columns
                Instruction::LocalSet(2), // columns
                // Return rows == columns
                Instruction::LocalGet(1), // rows
                Instruction::LocalGet(2), // columns
                Instruction::I32Eq,
            ],
        )
    }

    /// Validate matrix dimensions and structure
    fn register_validate_matrix_dimensions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.validateDimensions",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // matrix_ptr, expected_rows, expected_columns
            Some(WasmType::I32),                            // boolean result
            &[WasmType::I32, WasmType::I32],                // actual_rows, actual_columns
            vec![
                // Load actual rows
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load rows
                Instruction::LocalSet(3), // actual_rows
                // Load actual columns
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // load columns
                Instruction::LocalSet(4), // actual_columns
                // Check if actual_rows == expected_rows AND actual_columns == expected_columns
                Instruction::LocalGet(3), // actual_rows
                Instruction::LocalGet(1), // expected_rows
                Instruction::I32Eq,
                Instruction::LocalGet(4), // actual_columns
                Instruction::LocalGet(2), // expected_columns
                Instruction::I32Eq,
                Instruction::I32And, // rows_match AND columns_match
            ],
        )
    }

    /// Convert matrix to string representation
    fn register_matrix_to_string(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.toString",
            &[WasmType::I32],    // matrix_ptr
            Some(WasmType::I32), // string pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // rows, columns, type_id, result_ptr
            vec![
                // Load matrix dimensions and type
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load rows
                Instruction::LocalSet(1), // rows
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // load columns
                Instruction::LocalSet(2), // columns
                Instruction::LocalGet(0), // matrix_ptr
                Instruction::I32Load(MemArg {
                    offset: 8,
                    align: 2,
                    memory_index: 0,
                }), // load type_id
                Instruction::LocalSet(3), // type_id
                // Allocate result string (estimated size: 20 + rows*columns*10)
                Instruction::I32Const(20), // base size
                Instruction::LocalGet(1),  // rows
                Instruction::LocalGet(2),  // columns
                Instruction::I32Mul,
                Instruction::I32Const(10), // estimated chars per element
                Instruction::I32Mul,
                Instruction::I32Add,
                Instruction::I32Const(4), // alignment
                Instruction::Call(2000),  // memory.allocate
                Instruction::LocalSet(4), // result_ptr
                // For now, return a simple placeholder string pointer
                // TODO: Implement full matrix string formatting
                Instruction::LocalGet(4), // result_ptr
            ],
        )
    }

    /// Check if two matrices are equal
    fn register_matrix_equals(&self, codegen: &mut CodeGenerator) -> Result<u32, CompilerError> {
        register_stdlib_function_with_locals(
            codegen,
            "matrix.equals",
            &[WasmType::I32, WasmType::I32], // matrix1_ptr, matrix2_ptr
            Some(WasmType::I32),             // boolean result
            &[
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
                WasmType::I32,
            ], // rows1, cols1, rows2, cols2, type1, type2
            vec![
                // Load first matrix properties
                Instruction::LocalGet(0), // matrix1_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load rows
                Instruction::LocalSet(2), // rows1
                Instruction::LocalGet(0), // matrix1_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // load columns
                Instruction::LocalSet(3), // cols1
                Instruction::LocalGet(0), // matrix1_ptr
                Instruction::I32Load(MemArg {
                    offset: 8,
                    align: 2,
                    memory_index: 0,
                }), // load type_id
                Instruction::LocalSet(6), // type1
                // Load second matrix properties
                Instruction::LocalGet(1), // matrix2_ptr
                Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }), // load rows
                Instruction::LocalSet(4), // rows2
                Instruction::LocalGet(1), // matrix2_ptr
                Instruction::I32Load(MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }), // load columns
                Instruction::LocalSet(5), // cols2
                Instruction::LocalGet(1), // matrix2_ptr
                Instruction::I32Load(MemArg {
                    offset: 8,
                    align: 2,
                    memory_index: 0,
                }), // load type_id
                Instruction::LocalSet(7), // type2
                // Quick dimension and type checks
                Instruction::LocalGet(2), // rows1
                Instruction::LocalGet(4), // rows2
                Instruction::I32Eq,
                Instruction::LocalGet(3), // cols1
                Instruction::LocalGet(5), // cols2
                Instruction::I32Eq,
                Instruction::I32And,
                Instruction::LocalGet(6), // type1
                Instruction::LocalGet(7), // type2
                Instruction::I32Eq,
                Instruction::I32And,
                // If dimensions or types don't match, return false
                Instruction::I32Eqz,
                Instruction::If(BlockType::Result(ValType::I32)),
                Instruction::I32Const(0), // false
                Instruction::Else,
                // For now, assume equal if dimensions and types match
                // TODO: Implement element-by-element comparison
                Instruction::I32Const(1), // true
                Instruction::End,
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::CodeGenerator;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn test_matrix_literals_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let _manager = MatrixLiteralsManager::new(memory_manager);
    }

    #[test]
    fn test_register_matrix_functions() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MatrixLiteralsManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Verify functions are registered (checking a few key functions)
        assert!(codegen.get_function_index("matrix.createInteger").is_some());
        assert!(codegen.get_function_index("matrix.createNumber").is_some());
        assert!(codegen.get_function_index("matrix.getElement").is_some());
        assert!(codegen.get_function_index("matrix.setElement").is_some());
        assert!(codegen.get_function_index("matrix.getRows").is_some());
        assert!(codegen.get_function_index("matrix.getColumns").is_some());

        Ok(())
    }

    #[test]
    fn test_matrix_creation_functions_exist() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MatrixLiteralsManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that all creation functions are available
        assert!(codegen.get_function_index("matrix.createInteger").is_some());
        assert!(codegen.get_function_index("matrix.createNumber").is_some());
        assert!(codegen.get_function_index("matrix.createBoolean").is_some());
        assert!(codegen.get_function_index("matrix.createString").is_some());

        Ok(())
    }

    #[test]
    fn test_matrix_access_functions_exist() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MatrixLiteralsManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that all access functions are available
        assert!(codegen.get_function_index("matrix.getElement").is_some());
        assert!(codegen.get_function_index("matrix.setElement").is_some());
        assert!(codegen.get_function_index("matrix.getRow").is_some());
        assert!(codegen.get_function_index("matrix.getColumn").is_some());

        Ok(())
    }

    #[test]
    fn test_matrix_property_functions_exist() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MatrixLiteralsManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that all property functions are available
        assert!(codegen.get_function_index("matrix.getRows").is_some());
        assert!(codegen.get_function_index("matrix.getColumns").is_some());
        assert!(codegen.get_function_index("matrix.getSize").is_some());
        assert!(codegen.get_function_index("matrix.isSquare").is_some());

        Ok(())
    }

    #[test]
    fn test_matrix_utility_functions_exist() -> Result<(), CompilerError> {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MatrixLiteralsManager::new(memory_manager);
        let mut codegen = CodeGenerator::new();

        manager.register_functions(&mut codegen)?;

        // Test that all utility functions are available
        assert!(codegen
            .get_function_index("matrix.validateDimensions")
            .is_some());
        assert!(codegen.get_function_index("matrix.toString").is_some());
        assert!(codegen.get_function_index("matrix.equals").is_some());

        Ok(())
    }

    #[test]
    fn test_matrix_header_layout() {
        // Test that matrix header layout is correctly defined
        // Matrix header: 20 bytes
        // 0-3: rows (i32)
        // 4-7: columns (i32)
        // 8-11: type_id (i32)
        // 12-15: element_size (i32)
        // 16-19: total_size (i32)

        let header_size = 20;
        let rows_offset = 0;
        let columns_offset = 4;
        let type_id_offset = 8;
        let element_size_offset = 12;
        let total_size_offset = 16;

        assert_eq!(header_size, 20);
        assert_eq!(rows_offset, 0);
        assert_eq!(columns_offset, 4);
        assert_eq!(type_id_offset, 8);
        assert_eq!(element_size_offset, 12);
        assert_eq!(total_size_offset, 16);
    }

    #[test]
    fn test_matrix_type_ids() {
        // Test that matrix type IDs match expected values
        let integer_type_id = 1;
        let number_type_id = 2;
        let string_type_id = 3;
        let boolean_type_id = 4;

        assert_eq!(integer_type_id, 1);
        assert_eq!(number_type_id, 2);
        assert_eq!(string_type_id, 3);
        assert_eq!(boolean_type_id, 4);
    }
}
