use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use crate::stdlib::register_stdlib_function_with_locals;
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, MemArg, ValType};

/// Test framework implementation for Clean Language
/// Enables tests: blocks with test execution and reporting
pub struct TestFrameworkManager {
    #[allow(dead_code)]
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl TestFrameworkManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all test framework functions as stdlib functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.register_test_execution_functions(codegen)?;
        self.register_assertion_functions(codegen)?;
        self.register_test_reporting_functions(codegen)?;
        self.register_test_runner_functions(codegen)?;
        Ok(())
    }

    /// Register test execution functions
    fn register_test_execution_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Execute a single test case
        register_stdlib_function_with_locals(
            codegen,
            "test.executeTest",
            &[WasmType::I32, WasmType::I32], // test_name_ptr, test_function_ptr
            Some(WasmType::I32),             // test result (pass/fail)
            &[WasmType::I32, WasmType::I32, WasmType::I32], // result, error_occurred, start_time
            self.generate_execute_test(),
        )?;

        // Initialize test suite
        register_stdlib_function_with_locals(
            codegen,
            "test.initializeSuite",
            &[WasmType::I32], // suite_name_ptr
            None,             // void
            &[WasmType::I32], // suite_ptr
            self.generate_initialize_suite(),
        )?;

        // Finalize test suite and generate report
        register_stdlib_function_with_locals(
            codegen,
            "test.finalizeSuite",
            &[],                             // no parameters
            Some(WasmType::I32),             // report_ptr
            &[WasmType::I32, WasmType::I32], // report_ptr, summary_ptr
            self.generate_finalize_suite(),
        )?;

        // Run all tests in a suite
        register_stdlib_function_with_locals(
            codegen,
            "test.runSuite",
            &[WasmType::I32],                               // test_list_ptr
            Some(WasmType::I32),                            // overall result
            &[WasmType::I32, WasmType::I32, WasmType::I32], // passed, failed, total
            self.generate_run_suite(),
        )?;

        Ok(())
    }

    /// Register assertion functions for tests
    fn register_assertion_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Assert that condition is true
        register_stdlib_function_with_locals(
            codegen,
            "test.assertTrue",
            &[WasmType::I32, WasmType::I32], // condition, message_ptr
            None,                            // void (throws on failure)
            &[WasmType::I32],                // assertion_result
            self.generate_assert_true(),
        )?;

        // Assert that condition is false
        register_stdlib_function_with_locals(
            codegen,
            "test.assertFalse",
            &[WasmType::I32, WasmType::I32], // condition, message_ptr
            None,                            // void (throws on failure)
            &[WasmType::I32],                // assertion_result
            self.generate_assert_false(),
        )?;

        // Assert that two values are equal
        register_stdlib_function_with_locals(
            codegen,
            "test.assertEqual",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // expected, actual, message_ptr
            None,                                           // void (throws on failure)
            &[WasmType::I32],                               // comparison_result
            self.generate_assert_equal(),
        )?;

        // Assert that two values are not equal
        register_stdlib_function_with_locals(
            codegen,
            "test.assertNotEqual",
            &[WasmType::I32, WasmType::I32, WasmType::I32], // expected, actual, message_ptr
            None,                                           // void (throws on failure)
            &[WasmType::I32],                               // comparison_result
            self.generate_assert_not_equal(),
        )?;

        // Assert that value is null
        register_stdlib_function_with_locals(
            codegen,
            "test.assertNull",
            &[WasmType::I32, WasmType::I32], // value, message_ptr
            None,                            // void (throws on failure)
            &[WasmType::I32],                // null_check
            self.generate_assert_null(),
        )?;

        // Assert that value is not null
        register_stdlib_function_with_locals(
            codegen,
            "test.assertNotNull",
            &[WasmType::I32, WasmType::I32], // value, message_ptr
            None,                            // void (throws on failure)
            &[WasmType::I32],                // null_check
            self.generate_assert_not_null(),
        )?;

        Ok(())
    }

    /// Register test reporting functions
    fn register_test_reporting_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Report test pass
        register_stdlib_function_with_locals(
            codegen,
            "test.reportPass",
            &[WasmType::I32], // test_name_ptr
            None,             // void
            &[WasmType::I32], // report_operation
            self.generate_report_pass(),
        )?;

        // Report test failure
        register_stdlib_function_with_locals(
            codegen,
            "test.reportFail",
            &[WasmType::I32, WasmType::I32], // test_name_ptr, error_message_ptr
            None,                            // void
            &[WasmType::I32],                // report_operation
            self.generate_report_fail(),
        )?;

        // Get test statistics
        register_stdlib_function_with_locals(
            codegen,
            "test.getStatistics",
            &[],                                            // no parameters
            Some(WasmType::I32),                            // statistics_ptr
            &[WasmType::I32, WasmType::I32, WasmType::I32], // stats_ptr, passed, failed
            self.generate_get_statistics(),
        )?;

        // Print test summary
        register_stdlib_function_with_locals(
            codegen,
            "test.printSummary",
            &[],                                            // no parameters
            None,                                           // void
            &[WasmType::I32, WasmType::I32, WasmType::I32], // total, passed, failed
            self.generate_print_summary(),
        )?;

        Ok(())
    }

    /// Register test runner utility functions
    fn register_test_runner_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Setup test environment
        register_stdlib_function_with_locals(
            codegen,
            "test.setup",
            &[],              // no parameters
            None,             // void
            &[WasmType::I32], // setup_operation
            self.generate_test_setup(),
        )?;

        // Cleanup test environment
        register_stdlib_function_with_locals(
            codegen,
            "test.cleanup",
            &[],              // no parameters
            None,             // void
            &[WasmType::I32], // cleanup_operation
            self.generate_test_cleanup(),
        )?;

        // Mock function creation
        register_stdlib_function_with_locals(
            codegen,
            "test.createMock",
            &[WasmType::I32, WasmType::I32], // function_name_ptr, mock_behavior_ptr
            Some(WasmType::I32),             // mock_function_ptr
            &[WasmType::I32],                // mock_ptr
            self.generate_create_mock(),
        )?;

        // Time measurement for performance tests
        register_stdlib_function_with_locals(
            codegen,
            "test.measureTime",
            &[WasmType::I32],                // test_function_ptr
            Some(WasmType::I32),             // elapsed_time_ms
            &[WasmType::I32, WasmType::I32], // start_time, end_time
            self.generate_measure_time(),
        )?;

        Ok(())
    }

    /// Generate WASM for executing a single test
    fn generate_execute_test(&self) -> Vec<Instruction> {
        vec![
            // Parameters: test_name_ptr (0), test_function_ptr (1)
            // Locals: result (2), error_occurred (3), start_time (4)

            // Get start time for test execution
            Instruction::Call(self.get_current_time_function_index()),
            Instruction::LocalSet(4), // start_time
            // Clear any previous error state
            Instruction::Call(self.get_clear_error_function_index()),
            // Execute the test function
            Instruction::LocalGet(1), // test_function_ptr
            Instruction::CallIndirect {
                ty: 0, // Function type index for void() functions
                table: 0,
            },
            // Check if test failed due to error
            Instruction::Call(self.get_has_error_function_index()),
            Instruction::LocalSet(3), // error_occurred
            Instruction::LocalGet(3),
            Instruction::If(BlockType::Result(ValType::I32)),
            // Test failed - report failure
            Instruction::LocalGet(0), // test_name
            Instruction::Call(self.get_error_message_function_index()),
            Instruction::Call(self.get_report_fail_function_index()),
            Instruction::I32Const(0), // Failed
            Instruction::Else,
            // Test passed - report success
            Instruction::LocalGet(0), // test_name
            Instruction::Call(self.get_report_pass_function_index()),
            Instruction::I32Const(1), // Passed
            Instruction::End,
        ]
    }

    /// Generate WASM for initializing test suite
    fn generate_initialize_suite(&self) -> Vec<Instruction> {
        vec![
            // Parameters: suite_name_ptr (0)
            // Local: suite_ptr (1)

            // Initialize test counters to zero
            Instruction::I32Const(self.get_test_passed_address()),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(self.get_test_failed_address()),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(self.get_test_total_address()),
            Instruction::I32Const(0),
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Print suite start message
            Instruction::LocalGet(0), // suite_name
            Instruction::Call(self.get_print_suite_start_function_index()),
        ]
    }

    /// Generate WASM for finalizing test suite
    fn generate_finalize_suite(&self) -> Vec<Instruction> {
        vec![
            // Locals: report_ptr (0), summary_ptr (1)

            // Create test report
            Instruction::Call(self.get_create_report_function_index()),
            Instruction::LocalSet(0), // report_ptr
            // Print final summary
            Instruction::Call(self.get_print_summary_function_index()),
            // Return report
            Instruction::LocalGet(0),
        ]
    }

    /// Generate WASM for running entire test suite
    fn generate_run_suite(&self) -> Vec<Instruction> {
        vec![
            // Parameters: test_list_ptr (0)
            // Locals: passed (1), failed (2), total (3)

            // Initialize counters
            Instruction::I32Const(0),
            Instruction::LocalSet(1), // passed = 0
            Instruction::I32Const(0),
            Instruction::LocalSet(2), // failed = 0
            Instruction::I32Const(0),
            Instruction::LocalSet(3), // total = 0
            // Run all tests in the list (simplified loop)
            Instruction::LocalGet(0), // test_list
            Instruction::Call(self.get_run_test_list_function_index()),
            // Get final results
            Instruction::I32Const(self.get_test_passed_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // passed
            Instruction::I32Const(self.get_test_failed_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // failed
            // Calculate overall result (1 if all passed, 0 if any failed)
            Instruction::LocalGet(2), // failed
            Instruction::I32Eqz,      // failed == 0
        ]
    }

    /// Generate WASM for assertTrue assertion
    fn generate_assert_true(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition (0), message_ptr (1)
            // Local: assertion_result (2)
            Instruction::LocalGet(0), // condition
            Instruction::LocalSet(2), // Store for checking
            Instruction::LocalGet(2),
            Instruction::I32Eqz, // condition is false
            Instruction::If(BlockType::Empty),
            // Assertion failed - throw test error
            Instruction::I32Const(2001), // Assertion error code
            Instruction::LocalGet(1),    // message
            Instruction::Call(self.get_throw_test_error_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for assertFalse assertion
    fn generate_assert_false(&self) -> Vec<Instruction> {
        vec![
            // Parameters: condition (0), message_ptr (1)
            // Local: assertion_result (2)
            Instruction::LocalGet(0), // condition
            Instruction::LocalSet(2), // Store for checking
            Instruction::LocalGet(2),
            Instruction::If(BlockType::Empty),
            // Assertion failed - condition is true but should be false
            Instruction::I32Const(2002), // Assertion error code
            Instruction::LocalGet(1),    // message
            Instruction::Call(self.get_throw_test_error_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for assertEqual assertion
    fn generate_assert_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expected (0), actual (1), message_ptr (2)
            // Local: comparison_result (3)
            Instruction::LocalGet(0), // expected
            Instruction::LocalGet(1), // actual
            Instruction::I32Eq,
            Instruction::LocalSet(3), // comparison_result
            Instruction::LocalGet(3),
            Instruction::I32Eqz, // Values are not equal
            Instruction::If(BlockType::Empty),
            // Assertion failed
            Instruction::I32Const(2003), // Assertion error code
            Instruction::LocalGet(2),    // message
            Instruction::Call(self.get_throw_test_error_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for assertNotEqual assertion
    fn generate_assert_not_equal(&self) -> Vec<Instruction> {
        vec![
            // Parameters: expected (0), actual (1), message_ptr (2)
            // Local: comparison_result (3)
            Instruction::LocalGet(0), // expected
            Instruction::LocalGet(1), // actual
            Instruction::I32Eq,
            Instruction::LocalSet(3), // comparison_result
            Instruction::LocalGet(3),
            Instruction::If(BlockType::Empty),
            // Assertion failed - values are equal but shouldn't be
            Instruction::I32Const(2004), // Assertion error code
            Instruction::LocalGet(2),    // message
            Instruction::Call(self.get_throw_test_error_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for assertNull assertion
    fn generate_assert_null(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), message_ptr (1)
            // Local: null_check (2)
            Instruction::LocalGet(0), // value
            Instruction::I32Eqz,      // value == 0 (null)
            Instruction::LocalSet(2), // null_check
            Instruction::LocalGet(2),
            Instruction::I32Eqz, // Value is not null
            Instruction::If(BlockType::Empty),
            // Assertion failed
            Instruction::I32Const(2005), // Assertion error code
            Instruction::LocalGet(1),    // message
            Instruction::Call(self.get_throw_test_error_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for assertNotNull assertion
    fn generate_assert_not_null(&self) -> Vec<Instruction> {
        vec![
            // Parameters: value (0), message_ptr (1)
            // Local: null_check (2)
            Instruction::LocalGet(0), // value
            Instruction::I32Eqz,      // value == 0 (null)
            Instruction::LocalSet(2), // null_check
            Instruction::LocalGet(2),
            Instruction::If(BlockType::Empty),
            // Assertion failed - value is null
            Instruction::I32Const(2006), // Assertion error code
            Instruction::LocalGet(1),    // message
            Instruction::Call(self.get_throw_test_error_function_index()),
            Instruction::End,
        ]
    }

    /// Generate WASM for reporting test pass
    fn generate_report_pass(&self) -> Vec<Instruction> {
        vec![
            // Parameters: test_name_ptr (0)
            // Local: report_operation (1)

            // Increment passed test counter
            Instruction::I32Const(self.get_test_passed_address()),
            Instruction::I32Const(self.get_test_passed_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Print pass message
            Instruction::LocalGet(0), // test_name
            Instruction::Call(self.get_print_test_pass_function_index()),
        ]
    }

    /// Generate WASM for reporting test failure
    fn generate_report_fail(&self) -> Vec<Instruction> {
        vec![
            // Parameters: test_name_ptr (0), error_message_ptr (1)
            // Local: report_operation (2)

            // Increment failed test counter
            Instruction::I32Const(self.get_test_failed_address()),
            Instruction::I32Const(self.get_test_failed_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::I32Const(1),
            Instruction::I32Add,
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            // Print failure message
            Instruction::LocalGet(0), // test_name
            Instruction::LocalGet(1), // error_message
            Instruction::Call(self.get_print_test_fail_function_index()),
        ]
    }

    /// Generate WASM for getting test statistics
    fn generate_get_statistics(&self) -> Vec<Instruction> {
        vec![
            // Locals: stats_ptr (0), passed (1), failed (2)

            // Allocate statistics structure
            Instruction::I32Const(12), // Stats structure size (3 integers)
            Instruction::Call(self.get_allocate_function_index()),
            Instruction::LocalSet(0), // stats_ptr
            // Load passed count
            Instruction::I32Const(self.get_test_passed_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // passed
            // Load failed count
            Instruction::I32Const(self.get_test_failed_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // failed
            // Store statistics in structure
            Instruction::LocalGet(0), // stats_ptr
            Instruction::LocalGet(1), // passed
            Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0), // stats_ptr
            Instruction::LocalGet(2), // failed
            Instruction::I32Store(MemArg {
                offset: 4,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalGet(0), // stats_ptr
            Instruction::LocalGet(1), // passed
            Instruction::LocalGet(2), // failed
            Instruction::I32Add,      // total = passed + failed
            Instruction::I32Store(MemArg {
                offset: 8,
                align: 2,
                memory_index: 0,
            }),
            // Return statistics pointer
            Instruction::LocalGet(0),
        ]
    }

    /// Generate WASM for printing test summary
    fn generate_print_summary(&self) -> Vec<Instruction> {
        vec![
            // Locals: total (0), passed (1), failed (2)

            // Load test counts
            Instruction::I32Const(self.get_test_passed_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(1), // passed
            Instruction::I32Const(self.get_test_failed_address()),
            Instruction::I32Load(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }),
            Instruction::LocalSet(2), // failed
            Instruction::LocalGet(1), // passed
            Instruction::LocalGet(2), // failed
            Instruction::I32Add,
            Instruction::LocalSet(0), // total
            // Print summary
            Instruction::LocalGet(0), // total
            Instruction::LocalGet(1), // passed
            Instruction::LocalGet(2), // failed
            Instruction::Call(self.get_print_test_summary_function_index()),
        ]
    }

    /// Generate WASM for test setup
    fn generate_test_setup(&self) -> Vec<Instruction> {
        vec![
            // Local: setup_operation (0)

            // Initialize test environment
            Instruction::Call(self.get_init_test_environment_function_index()),
        ]
    }

    /// Generate WASM for test cleanup
    fn generate_test_cleanup(&self) -> Vec<Instruction> {
        vec![
            // Local: cleanup_operation (0)

            // Clean up test environment
            Instruction::Call(self.get_cleanup_test_environment_function_index()),
        ]
    }

    /// Generate WASM for creating mock functions
    fn generate_create_mock(&self) -> Vec<Instruction> {
        vec![
            // Parameters: function_name_ptr (0), mock_behavior_ptr (1)
            // Local: mock_ptr (2)

            // Create mock function (simplified)
            Instruction::LocalGet(0), // function_name
            Instruction::LocalGet(1), // mock_behavior
            Instruction::Call(self.get_create_mock_function_index()),
        ]
    }

    /// Generate WASM for measuring execution time
    fn generate_measure_time(&self) -> Vec<Instruction> {
        vec![
            // Parameters: test_function_ptr (0)
            // Locals: start_time (1), end_time (2)

            // Get start time
            Instruction::Call(self.get_current_time_function_index()),
            Instruction::LocalSet(1), // start_time
            // Execute function
            Instruction::LocalGet(0), // test_function_ptr
            Instruction::CallIndirect { ty: 0, table: 0 },
            // Get end time
            Instruction::Call(self.get_current_time_function_index()),
            Instruction::LocalSet(2), // end_time
            // Calculate elapsed time
            Instruction::LocalGet(2), // end_time
            Instruction::LocalGet(1), // start_time
            Instruction::I32Sub,      // elapsed = end - start
        ]
    }

    // Helper function indices and memory addresses
    fn get_current_time_function_index(&self) -> u32 {
        800
    }
    fn get_clear_error_function_index(&self) -> u32 {
        801
    }
    fn get_has_error_function_index(&self) -> u32 {
        802
    }
    fn get_error_message_function_index(&self) -> u32 {
        803
    }
    fn get_report_pass_function_index(&self) -> u32 {
        804
    }
    fn get_report_fail_function_index(&self) -> u32 {
        805
    }
    fn get_print_suite_start_function_index(&self) -> u32 {
        806
    }
    fn get_create_report_function_index(&self) -> u32 {
        807
    }
    fn get_print_summary_function_index(&self) -> u32 {
        808
    }
    fn get_run_test_list_function_index(&self) -> u32 {
        809
    }
    fn get_throw_test_error_function_index(&self) -> u32 {
        810
    }
    fn get_allocate_function_index(&self) -> u32 {
        811
    }
    fn get_print_test_pass_function_index(&self) -> u32 {
        812
    }
    fn get_print_test_fail_function_index(&self) -> u32 {
        813
    }
    fn get_print_test_summary_function_index(&self) -> u32 {
        814
    }
    fn get_init_test_environment_function_index(&self) -> u32 {
        815
    }
    fn get_cleanup_test_environment_function_index(&self) -> u32 {
        816
    }
    fn get_create_mock_function_index(&self) -> u32 {
        817
    }

    fn get_test_passed_address(&self) -> i32 {
        0x3000
    }
    fn get_test_failed_address(&self) -> i32 {
        0x3004
    }
    fn get_test_total_address(&self) -> i32 {
        0x3008
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_framework_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let test_manager = TestFrameworkManager::new(memory_manager.clone());

        // Test that manager is created successfully
        assert!(test_manager.memory_manager.borrow().data.is_empty());
    }

    #[test]
    fn test_execute_test_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let test_manager = TestFrameworkManager::new(memory_manager);

        let instructions = test_manager.generate_execute_test();
        assert!(!instructions.is_empty());

        // Should contain CallIndirect for test function execution
        assert!(matches!(instructions[4], Instruction::CallIndirect { .. }));
    }

    #[test]
    fn test_assertion_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let test_manager = TestFrameworkManager::new(memory_manager);

        let assert_true = test_manager.generate_assert_true();
        assert!(!assert_true.is_empty());
        // After: LocalGet(0), LocalSet(2), LocalGet(2), I32Eqz, If should be at index 4
        assert!(matches!(assert_true[4], Instruction::If(_)));

        let assert_equal = test_manager.generate_assert_equal();
        assert!(!assert_equal.is_empty());
        // After: LocalGet(0), LocalGet(1), I32Eq should be at index 2
        assert!(matches!(assert_equal[2], Instruction::I32Eq));
    }

    #[test]
    fn test_reporting_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let test_manager = TestFrameworkManager::new(memory_manager);

        let report_pass = test_manager.generate_report_pass();
        assert!(!report_pass.is_empty());

        let report_fail = test_manager.generate_report_fail();
        assert!(!report_fail.is_empty());
        // Should have more instructions for error handling
        assert!(report_fail.len() >= report_pass.len());
    }

    #[test]
    fn test_suite_management() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let test_manager = TestFrameworkManager::new(memory_manager);

        let init_suite = test_manager.generate_initialize_suite();
        assert!(!init_suite.is_empty());

        let run_suite = test_manager.generate_run_suite();
        assert!(!run_suite.is_empty());
        // Should end with comparison for overall result
        assert!(matches!(
            run_suite[run_suite.len() - 1],
            Instruction::I32Eqz
        ));
    }

    #[test]
    fn test_statistics_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let test_manager = TestFrameworkManager::new(memory_manager);

        let stats = test_manager.generate_get_statistics();
        assert!(!stats.is_empty());

        // Should allocate memory for statistics structure
        assert!(matches!(stats[1], Instruction::Call(_)));
        // Should end by returning the stats pointer
        assert!(matches!(stats[stats.len() - 1], Instruction::LocalGet(0)));
    }
}
