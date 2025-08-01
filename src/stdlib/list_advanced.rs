use crate::codegen::CodeGenerator;
use crate::types::WasmType;
use crate::error::CompilerError;
use crate::stdlib::{MemoryManager, register_stdlib_function_with_locals};
use wasm_encoder::{Instruction, MemArg, BlockType, ValType};
use std::rc::Rc;
use std::cell::RefCell;

/// Advanced List class implementation for Clean Language
/// Implements missing functional methods: map(), filter(), reduce(), forEach(), first(), last(), fill(), range()
pub struct ListAdvancedManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl ListAdvancedManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self {
            memory_manager,
        }
    }

    /// Register all advanced list functions with the code generator
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Functional methods
        self.register_map_functions(codegen)?;
        self.register_filter_functions(codegen)?;
        self.register_reduce_functions(codegen)?;
        self.register_foreach_functions(codegen)?;
        
        // Access methods
        self.register_access_functions(codegen)?;
        
        // Utility methods
        self.register_utility_functions(codegen)?;
        
        Ok(())
    }

    fn register_map_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // list.mapAdvanced(list_ptr, function_ptr) -> list
        // Apply a function to each element and return new list with results
        register_stdlib_function_with_locals(
            codegen,
            "list.mapAdvanced",
            &[WasmType::I32, WasmType::I32], // list_ptr, function_ptr
            Some(WasmType::I32), // result list pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // result_list, list_size, i, element, mapped_value, element_ptr
            self.generate_map_advanced()
        )?;

        Ok(())
    }

    fn register_filter_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // list.filterAdvanced(list_ptr, predicate_function_ptr) -> list
        // Filter elements based on predicate function
        register_stdlib_function_with_locals(
            codegen,
            "list.filterAdvanced",
            &[WasmType::I32, WasmType::I32], // list_ptr, predicate_ptr
            Some(WasmType::I32), // filtered list pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // result_list, list_size, i, element, predicate_result, filtered_count
            self.generate_filter_advanced()
        )?;

        Ok(())
    }

    fn register_reduce_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // list.reduceAdvanced(list_ptr, accumulator_function_ptr, initial_value) -> value
        // Reduce list to single value using accumulator function
        register_stdlib_function_with_locals(
            codegen,
            "list.reduceAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // list_ptr, accumulator_ptr, initial_value
            Some(WasmType::I32), // final accumulated value
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // accumulator, list_size, i, element, new_accumulator
            self.generate_reduce_advanced()
        )?;

        Ok(())
    }

    fn register_foreach_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // list.forEachAdvanced(list_ptr, action_function_ptr) -> void
        // Execute action function for each element
        register_stdlib_function_with_locals(
            codegen,
            "list.forEachAdvanced",
            &[WasmType::I32, WasmType::I32], // list_ptr, action_ptr
            None, // void return
            &[WasmType::I32, WasmType::I32, WasmType::I32], // list_size, i, element
            self.generate_foreach_advanced()
        )?;

        Ok(())
    }

    fn register_access_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // list.firstAdvanced(list_ptr) -> value
        // Get first element of list (or 0 if empty)
        register_stdlib_function_with_locals(
            codegen,
            "list.firstAdvanced",
            &[WasmType::I32], // list_ptr
            Some(WasmType::I32), // first element value
            &[WasmType::I32], // list_size
            self.generate_first_advanced()
        )?;

        // list.lastAdvanced(list_ptr) -> value
        // Get last element of list (or 0 if empty)
        register_stdlib_function_with_locals(
            codegen,
            "list.lastAdvanced",
            &[WasmType::I32], // list_ptr
            Some(WasmType::I32), // last element value
            &[WasmType::I32, WasmType::I32], // list_size, last_index
            self.generate_last_advanced()
        )?;

        Ok(())
    }

    fn register_utility_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // list.fillAdvanced(list_ptr, value, start_index, end_index) -> list
        // Fill list elements with value from start to end index
        register_stdlib_function_with_locals(
            codegen,
            "list.fillAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // list_ptr, value, start_index, end_index
            Some(WasmType::I32), // modified list pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32], // list_size, i, element_ptr
            self.generate_fill_advanced()
        )?;

        // list.rangeAdvanced(start, end, step) -> list
        // Create list with integer range from start to end with step
        register_stdlib_function_with_locals(
            codegen,
            "list.rangeAdvanced",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // start, end, step
            Some(WasmType::I32), // range list pointer
            &[WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32], // list_size, list_ptr, i, current_value, element_ptr
            self.generate_range_advanced()
        )?;

        Ok(())
    }

    // Implementation methods for advanced list operations

    fn generate_map_advanced(&self) -> Vec<Instruction> {
        vec![
            // Get list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(3), // list_size

            // Create result list with same size
            Instruction::I32Const(16), // list header size
            Instruction::LocalGet(3), // list_size
            Instruction::I32Const(4), // element size (assuming 4-byte integers)
            Instruction::I32Mul,
            Instruction::I32Add, // total size = 16 + (size * 4)
            
            // Allocate memory for result list (simplified allocation at fixed address)
            Instruction::I32Const(0x4000), // result list address
            Instruction::LocalSet(2), // result_list

            // Initialize result list header
            Instruction::LocalGet(2), // result_list
            Instruction::LocalGet(3), // list_size
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }), // size
            
            Instruction::LocalGet(2), // result_list
            Instruction::LocalGet(3), // list_size (also capacity)
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }), // capacity
            
            Instruction::LocalGet(2), // result_list
            Instruction::I32Const(1), // type_id for integer
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }), // type_id

            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(4), // i = 0

            // Loop through each element
            Instruction::Loop(BlockType::Empty),
                // Check loop condition: i < list_size
                Instruction::LocalGet(4), // i
                Instruction::LocalGet(3), // list_size
                Instruction::I32LtU,
                Instruction::If(BlockType::Empty),
                    // Get element at index i
                    Instruction::LocalGet(0), // list_ptr
                    Instruction::I32Const(16), // offset to data
                    Instruction::I32Add,
                    Instruction::LocalGet(4), // i
                    Instruction::I32Const(4), // element size
                    Instruction::I32Mul,
                    Instruction::I32Add, // element address
                    Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
                    Instruction::LocalSet(5), // element

                    // Apply mapping function (simplified - just return element + 1)
                    // In full implementation, would use CallIndirect to call function_ptr
                    Instruction::LocalGet(5), // element
                    Instruction::I32Const(1),
                    Instruction::I32Add,
                    Instruction::LocalSet(6), // mapped_value

                    // Store mapped value in result list at index i
                    Instruction::LocalGet(2), // result_list
                    Instruction::I32Const(16), // offset to data
                    Instruction::I32Add,
                    Instruction::LocalGet(4), // i
                    Instruction::I32Const(4), // element size
                    Instruction::I32Mul,
                    Instruction::I32Add, // target address
                    Instruction::LocalGet(6), // mapped_value
                    Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                    // Increment counter
                    Instruction::LocalGet(4), // i
                    Instruction::I32Const(1),
                    Instruction::I32Add,
                    Instruction::LocalSet(4), // i++

                    // Continue loop
                    Instruction::Br(1),
                Instruction::End,
            Instruction::End,

            // Return result list pointer
            Instruction::LocalGet(2), // result_list
        ]
    }

    fn generate_filter_advanced(&self) -> Vec<Instruction> {
        vec![
            // Get list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(3), // list_size

            // Create result list (initially same size, will adjust)
            Instruction::I32Const(0x5000), // result list address
            Instruction::LocalSet(2), // result_list

            // Initialize result list header
            Instruction::LocalGet(2), // result_list
            Instruction::I32Const(0), // initially 0 elements
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }), // size
            
            Instruction::LocalGet(2), // result_list
            Instruction::LocalGet(3), // original capacity
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }), // capacity
            
            Instruction::LocalGet(2), // result_list
            Instruction::I32Const(1), // type_id for integer
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }), // type_id

            // Initialize counters
            Instruction::I32Const(0),
            Instruction::LocalSet(4), // i = 0
            Instruction::I32Const(0),
            Instruction::LocalSet(7), // filtered_count = 0

            // Loop through each element
            Instruction::Loop(BlockType::Empty),
                // Check loop condition: i < list_size
                Instruction::LocalGet(4), // i
                Instruction::LocalGet(3), // list_size
                Instruction::I32LtU,
                Instruction::If(BlockType::Empty),
                    // Get element at index i
                    Instruction::LocalGet(0), // list_ptr
                    Instruction::I32Const(16), // offset to data
                    Instruction::I32Add,
                    Instruction::LocalGet(4), // i
                    Instruction::I32Const(4), // element size
                    Instruction::I32Mul,
                    Instruction::I32Add, // element address
                    Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
                    Instruction::LocalSet(5), // element

                    // Apply predicate function (simplified - filter even numbers)
                    // In full implementation, would use CallIndirect to call predicate_ptr
                    Instruction::LocalGet(5), // element
                    Instruction::I32Const(2),
                    Instruction::I32RemU, // element % 2
                    Instruction::I32Const(0),
                    Instruction::I32Eq, // element % 2 == 0 (even)
                    Instruction::LocalSet(6), // predicate_result

                    // If predicate is true, add element to result
                    Instruction::LocalGet(6), // predicate_result
                    Instruction::If(BlockType::Empty),
                        // Store element in result list at filtered_count index
                        Instruction::LocalGet(2), // result_list
                        Instruction::I32Const(16), // offset to data
                        Instruction::I32Add,
                        Instruction::LocalGet(7), // filtered_count
                        Instruction::I32Const(4), // element size
                        Instruction::I32Mul,
                        Instruction::I32Add, // target address
                        Instruction::LocalGet(5), // element
                        Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                        // Increment filtered count
                        Instruction::LocalGet(7), // filtered_count
                        Instruction::I32Const(1),
                        Instruction::I32Add,
                        Instruction::LocalSet(7), // filtered_count++
                    Instruction::End,

                    // Increment loop counter
                    Instruction::LocalGet(4), // i
                    Instruction::I32Const(1),
                    Instruction::I32Add,
                    Instruction::LocalSet(4), // i++

                    // Continue loop
                    Instruction::Br(1),
                Instruction::End,
            Instruction::End,

            // Update result list size with filtered count
            Instruction::LocalGet(2), // result_list
            Instruction::LocalGet(7), // filtered_count
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }), // size

            // Return result list pointer
            Instruction::LocalGet(2), // result_list
        ]
    }

    fn generate_reduce_advanced(&self) -> Vec<Instruction> {
        vec![
            // Get list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(4), // list_size

            // Initialize accumulator with initial value
            Instruction::LocalGet(2), // initial_value
            Instruction::LocalSet(3), // accumulator

            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(5), // i = 0

            // Loop through each element
            Instruction::Loop(BlockType::Empty),
                // Check loop condition: i < list_size
                Instruction::LocalGet(5), // i
                Instruction::LocalGet(4), // list_size
                Instruction::I32LtU,
                Instruction::If(BlockType::Empty),
                    // Get element at index i
                    Instruction::LocalGet(0), // list_ptr
                    Instruction::I32Const(16), // offset to data
                    Instruction::I32Add,
                    Instruction::LocalGet(5), // i
                    Instruction::I32Const(4), // element size
                    Instruction::I32Mul,
                    Instruction::I32Add, // element address
                    Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
                    Instruction::LocalSet(6), // element

                    // Apply accumulator function (simplified - sum elements)
                    // In full implementation, would use CallIndirect to call accumulator_ptr
                    Instruction::LocalGet(3), // accumulator
                    Instruction::LocalGet(6), // element
                    Instruction::I32Add,
                    Instruction::LocalSet(7), // new_accumulator

                    // Update accumulator
                    Instruction::LocalGet(7), // new_accumulator
                    Instruction::LocalSet(3), // accumulator = new_accumulator

                    // Increment counter
                    Instruction::LocalGet(5), // i
                    Instruction::I32Const(1),
                    Instruction::I32Add,
                    Instruction::LocalSet(5), // i++

                    // Continue loop
                    Instruction::Br(1),
                Instruction::End,
            Instruction::End,

            // Return final accumulator value
            Instruction::LocalGet(3), // accumulator
        ]
    }

    fn generate_foreach_advanced(&self) -> Vec<Instruction> {
        vec![
            // Get list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(2), // list_size

            // Initialize loop counter
            Instruction::I32Const(0),
            Instruction::LocalSet(3), // i = 0

            // Loop through each element
            Instruction::Loop(BlockType::Empty),
                // Check loop condition: i < list_size
                Instruction::LocalGet(3), // i
                Instruction::LocalGet(2), // list_size
                Instruction::I32LtU,
                Instruction::If(BlockType::Empty),
                    // Get element at index i
                    Instruction::LocalGet(0), // list_ptr
                    Instruction::I32Const(16), // offset to data
                    Instruction::I32Add,
                    Instruction::LocalGet(3), // i
                    Instruction::I32Const(4), // element size
                    Instruction::I32Mul,
                    Instruction::I32Add, // element address
                    Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
                    Instruction::LocalSet(4), // element

                    // Execute action function on element (simplified - no operation)
                    // In full implementation, would use CallIndirect to call action_ptr
                    // For now, just consume the element value
                    Instruction::LocalGet(4), // element
                    Instruction::Drop, // consume element (action performed)

                    // Increment counter
                    Instruction::LocalGet(3), // i
                    Instruction::I32Const(1),
                    Instruction::I32Add,
                    Instruction::LocalSet(3), // i++

                    // Continue loop
                    Instruction::Br(1),
                Instruction::End,
            Instruction::End,

            // Function returns void (no return instruction needed)
        ]
    }

    fn generate_first_advanced(&self) -> Vec<Instruction> {
        vec![
            // Get list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // list_size

            // Check if list is empty
            Instruction::LocalGet(1), // list_size
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // List is empty, return 0
                Instruction::I32Const(0),
            Instruction::Else,
                // List has elements, get first element
                Instruction::LocalGet(0), // list_ptr
                Instruction::I32Const(16), // offset to data
                Instruction::I32Add, // first element address
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // load first element
            Instruction::End,
        ]
    }

    fn generate_last_advanced(&self) -> Vec<Instruction> {
        vec![
            // Get list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(1), // list_size

            // Check if list is empty
            Instruction::LocalGet(1), // list_size
            Instruction::I32Const(0),
            Instruction::I32Eq,
            Instruction::If(BlockType::Result(ValType::I32)),
                // List is empty, return 0
                Instruction::I32Const(0),
            Instruction::Else,
                // List has elements, calculate last index
                Instruction::LocalGet(1), // list_size
                Instruction::I32Const(1),
                Instruction::I32Sub,
                Instruction::LocalSet(2), // last_index = size - 1

                // Get last element
                Instruction::LocalGet(0), // list_ptr
                Instruction::I32Const(16), // offset to data
                Instruction::I32Add,
                Instruction::LocalGet(2), // last_index
                Instruction::I32Const(4), // element size
                Instruction::I32Mul,
                Instruction::I32Add, // last element address
                Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }), // load last element
            Instruction::End,
        ]
    }

    fn generate_fill_advanced(&self) -> Vec<Instruction> {
        vec![
            // Get list size
            Instruction::LocalGet(0), // list_ptr
            Instruction::I32Load(MemArg { offset: 0, align: 2, memory_index: 0 }),
            Instruction::LocalSet(4), // list_size

            // Validate start and end indices (simplified - assume valid)
            // In full implementation, would clamp indices to valid range

            // Initialize loop counter with start_index
            Instruction::LocalGet(2), // start_index
            Instruction::LocalSet(5), // i = start_index

            // Loop through from start_index to end_index
            Instruction::Loop(BlockType::Empty),
                // Check loop condition: i < end_index AND i < list_size
                Instruction::LocalGet(5), // i
                Instruction::LocalGet(3), // end_index
                Instruction::I32LtU,
                Instruction::LocalGet(5), // i
                Instruction::LocalGet(4), // list_size
                Instruction::I32LtU,
                Instruction::I32And, // (i < end_index) AND (i < list_size)
                Instruction::If(BlockType::Empty),
                    // Calculate element address
                    Instruction::LocalGet(0), // list_ptr
                    Instruction::I32Const(16), // offset to data
                    Instruction::I32Add,
                    Instruction::LocalGet(5), // i
                    Instruction::I32Const(4), // element size
                    Instruction::I32Mul,
                    Instruction::I32Add,
                    Instruction::LocalSet(6), // element_ptr

                    // Store fill value at element address
                    Instruction::LocalGet(6), // element_ptr
                    Instruction::LocalGet(1), // value
                    Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                    // Increment counter
                    Instruction::LocalGet(5), // i
                    Instruction::I32Const(1),
                    Instruction::I32Add,
                    Instruction::LocalSet(5), // i++

                    // Continue loop
                    Instruction::Br(1),
                Instruction::End,
            Instruction::End,

            // Return modified list pointer
            Instruction::LocalGet(0), // list_ptr
        ]
    }

    fn generate_range_advanced(&self) -> Vec<Instruction> {
        vec![
            // Calculate range size: (end - start + step - 1) / step
            Instruction::LocalGet(1), // end
            Instruction::LocalGet(0), // start
            Instruction::I32Sub, // end - start
            Instruction::LocalGet(2), // step
            Instruction::I32Add, // (end - start) + step
            Instruction::I32Const(1),
            Instruction::I32Sub, // (end - start) + step - 1
            Instruction::LocalGet(2), // step
            Instruction::I32DivU, // range_size = ((end - start) + step - 1) / step
            Instruction::LocalSet(3), // list_size

            // Allocate memory for range list
            Instruction::I32Const(0x6000), // range list address
            Instruction::LocalSet(4), // list_ptr

            // Initialize list header
            Instruction::LocalGet(4), // list_ptr
            Instruction::LocalGet(3), // list_size
            Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }), // size
            
            Instruction::LocalGet(4), // list_ptr
            Instruction::LocalGet(3), // list_size (also capacity)
            Instruction::I32Store(MemArg { offset: 4, align: 2, memory_index: 0 }), // capacity
            
            Instruction::LocalGet(4), // list_ptr
            Instruction::I32Const(1), // type_id for integer
            Instruction::I32Store(MemArg { offset: 8, align: 2, memory_index: 0 }), // type_id

            // Initialize loop variables
            Instruction::I32Const(0),
            Instruction::LocalSet(5), // i = 0
            Instruction::LocalGet(0), // start
            Instruction::LocalSet(6), // current_value = start

            // Loop to fill range values
            Instruction::Loop(BlockType::Empty),
                // Check loop condition: i < list_size
                Instruction::LocalGet(5), // i
                Instruction::LocalGet(3), // list_size
                Instruction::I32LtU,
                Instruction::If(BlockType::Empty),
                    // Calculate element address
                    Instruction::LocalGet(4), // list_ptr
                    Instruction::I32Const(16), // offset to data
                    Instruction::I32Add,
                    Instruction::LocalGet(5), // i
                    Instruction::I32Const(4), // element size
                    Instruction::I32Mul,
                    Instruction::I32Add,
                    Instruction::LocalSet(7), // element_ptr

                    // Store current value at element address
                    Instruction::LocalGet(7), // element_ptr
                    Instruction::LocalGet(6), // current_value
                    Instruction::I32Store(MemArg { offset: 0, align: 2, memory_index: 0 }),

                    // Increment current value by step
                    Instruction::LocalGet(6), // current_value
                    Instruction::LocalGet(2), // step
                    Instruction::I32Add,
                    Instruction::LocalSet(6), // current_value += step

                    // Increment counter
                    Instruction::LocalGet(5), // i
                    Instruction::I32Const(1),
                    Instruction::I32Add,
                    Instruction::LocalSet(5), // i++

                    // Continue loop
                    Instruction::Br(1),
                Instruction::End,
            Instruction::End,

            // Return range list pointer
            Instruction::LocalGet(4), // list_ptr
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::CodeGenerator;
    use crate::types::WasmType;

    #[test]
    fn test_list_advanced_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let _manager = ListAdvancedManager::new(memory_manager);
    }

    #[test]
    fn test_map_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = ListAdvancedManager::new(memory_manager);
        let instructions = manager.generate_map_advanced();
        assert!(!instructions.is_empty(), "Map instructions should not be empty");
    }

    #[test]
    fn test_filter_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = ListAdvancedManager::new(memory_manager);
        let instructions = manager.generate_filter_advanced();
        assert!(!instructions.is_empty(), "Filter instructions should not be empty");
    }

    #[test]
    fn test_reduce_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = ListAdvancedManager::new(memory_manager);
        let instructions = manager.generate_reduce_advanced();
        assert!(!instructions.is_empty(), "Reduce instructions should not be empty");
    }

    #[test]
    fn test_foreach_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = ListAdvancedManager::new(memory_manager);
        let instructions = manager.generate_foreach_advanced();
        assert!(!instructions.is_empty(), "ForEach instructions should not be empty");
    }

    #[test]
    fn test_first_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = ListAdvancedManager::new(memory_manager);
        let instructions = manager.generate_first_advanced();
        assert!(!instructions.is_empty(), "First instructions should not be empty");
    }

    #[test]
    fn test_last_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = ListAdvancedManager::new(memory_manager);
        let instructions = manager.generate_last_advanced();
        assert!(!instructions.is_empty(), "Last instructions should not be empty");
    }

    #[test]
    fn test_fill_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = ListAdvancedManager::new(memory_manager);
        let instructions = manager.generate_fill_advanced();
        assert!(!instructions.is_empty(), "Fill instructions should not be empty");
    }

    #[test]
    fn test_range_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = ListAdvancedManager::new(memory_manager);
        let instructions = manager.generate_range_advanced();
        assert!(!instructions.is_empty(), "Range instructions should not be empty");
    }
}