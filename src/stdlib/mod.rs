// Core standard library modules
pub mod basic_ops;
pub mod console_ops;
pub mod error;
pub mod list_ops;
pub mod matrix_ops;
pub mod memory;
pub mod numeric_ops;
pub mod string_ops;
pub mod type_conv;

// Class-based standard library modules
pub mod async_programming;
pub mod conditional;
pub mod console_input;
pub mod default_parameters;
pub mod error_handling;
pub mod file_advanced;
pub mod file_class;
pub mod http_advanced;
pub mod http_class;
pub mod import_system;
pub mod list_advanced;
pub mod list_behavior;
pub mod list_class;
pub mod math_advanced;
pub mod math_class;
pub mod matrix_literals;
pub mod method_style;
pub mod multiline;
pub mod numeric_literals;
pub mod pairs_type;
pub mod string_advanced;
pub mod string_class;
pub mod string_interpolation;
pub mod test_framework;

// Re-exports for convenience
pub use async_programming::AsyncProgrammingManager;
pub use basic_ops::basic_ops::*;
pub use conditional::ConditionalManager;
pub use console_input::ConsoleInputManager;
pub use console_ops::{ConsoleClass, ConsoleOperations};
pub use default_parameters::DefaultParameterManager;
pub use error::StdlibError;
pub use error_handling::ErrorHandlingManager;
pub use file_advanced::FileAdvancedManager;
pub use file_class::FileClass;
pub use http_advanced::HttpAdvancedManager;
pub use http_class::HttpClass;
pub use import_system::ImportSystemManager;
pub use list_advanced::ListAdvancedManager;
pub use list_behavior::ListBehaviorManager;
pub use list_class::ListClass;
pub use list_ops::ListManager;
pub use math_advanced::MathAdvancedManager;
pub use math_class::MathClass;
pub use matrix_literals::MatrixLiteralsManager;
pub use matrix_ops::MatrixOperations;
pub use memory::MemoryManager;
pub use method_style::MethodStyleManager;
pub use multiline::MultilineManager;
pub use numeric_literals::NumericLiteralsManager;
pub use numeric_ops::NumericOperations;
pub use pairs_type::PairsTypeManager;
pub use string_advanced::StringAdvancedManager;
pub use string_class::StringClass;
pub use string_interpolation::StringInterpolationManager;
pub use string_ops::{StringManager, StringOperations};
pub use test_framework::TestFrameworkManager;
pub use type_conv::TypeConvOperations;

use crate::codegen::{CodeGenerator, HEAP_START};
use crate::error::CompilerError;
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::Instruction;

/// Standard library implementation for the Clean Language
pub struct StandardLibrary {
    string_ops: StringOperations,
    numeric_ops: NumericOperations,
    matrix_ops: MatrixOperations,
    type_conv: TypeConvOperations,
    math_class: MathClass,
    #[allow(dead_code)]
    string_class: StringClass,
    #[allow(dead_code)]
    list_class: ListClass,
    #[allow(dead_code)]
    file_class: FileClass,
    #[allow(dead_code)]
    http_class: HttpClass,
    #[allow(dead_code)]
    list_behavior: ListBehaviorManager,
    #[allow(dead_code)]
    method_style: MethodStyleManager,
    #[allow(dead_code)]
    conditional: ConditionalManager,
    #[allow(dead_code)]
    multiline: MultilineManager,
    #[allow(dead_code)]
    error_handling: ErrorHandlingManager,
    #[allow(dead_code)]
    test_framework: TestFrameworkManager,
    #[allow(dead_code)]
    string_interpolation: StringInterpolationManager,
    #[allow(dead_code)]
    default_parameters: DefaultParameterManager,
    #[allow(dead_code)]
    numeric_literals: NumericLiteralsManager,
    #[allow(dead_code)]
    matrix_literals: MatrixLiteralsManager,
    #[allow(dead_code)]
    pairs_type: PairsTypeManager,
    #[allow(dead_code)]
    string_advanced: StringAdvancedManager,
    #[allow(dead_code)]
    list_advanced: ListAdvancedManager,
    #[allow(dead_code)]
    math_advanced: MathAdvancedManager,
    #[allow(dead_code)]
    file_advanced: FileAdvancedManager,
    #[allow(dead_code)]
    http_advanced: HttpAdvancedManager,
    #[allow(dead_code)]
    console_input: ConsoleInputManager,
    #[allow(dead_code)]
    async_programming: AsyncProgrammingManager,
    #[allow(dead_code)]
    import_system: ImportSystemManager,
    #[allow(dead_code)]
    memory_manager: Rc<RefCell<MemoryManager>>,
    console_ops: ConsoleOperations,
    console_class: ConsoleClass,
}

impl Default for StandardLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl StandardLibrary {
    pub fn new() -> Self {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(
            16,
            Some(HEAP_START as u32),
        )));
        Self {
            string_ops: StringOperations::new(HEAP_START),
            numeric_ops: NumericOperations::new(),
            matrix_ops: MatrixOperations::new(),
            type_conv: TypeConvOperations::new(HEAP_START),
            math_class: MathClass::new(),
            string_class: StringClass::new(),
            list_class: ListClass::new(),
            file_class: FileClass::new(),
            http_class: HttpClass::new(),
            list_behavior: ListBehaviorManager::new(memory_manager.clone()),
            method_style: MethodStyleManager::new(memory_manager.clone()),
            conditional: ConditionalManager::new(memory_manager.clone()),
            multiline: MultilineManager::new(memory_manager.clone()),
            error_handling: ErrorHandlingManager::new(memory_manager.clone()),
            test_framework: TestFrameworkManager::new(memory_manager.clone()),
            string_interpolation: StringInterpolationManager::new(memory_manager.clone()),
            default_parameters: DefaultParameterManager::new(memory_manager.clone()),
            numeric_literals: NumericLiteralsManager::new(memory_manager.clone()),
            matrix_literals: MatrixLiteralsManager::new(memory_manager.clone()),
            pairs_type: PairsTypeManager::new(memory_manager.clone()),
            string_advanced: StringAdvancedManager::new(memory_manager.clone()),
            list_advanced: ListAdvancedManager::new(memory_manager.clone()),
            math_advanced: MathAdvancedManager::new(memory_manager.clone()),
            file_advanced: FileAdvancedManager::new(memory_manager.clone()),
            http_advanced: HttpAdvancedManager::new(memory_manager.clone()),
            console_input: ConsoleInputManager::new(memory_manager.clone()),
            async_programming: AsyncProgrammingManager::new(memory_manager.clone()),
            import_system: ImportSystemManager::new(memory_manager.clone()),
            memory_manager,
            console_ops: ConsoleOperations::new(HEAP_START),
            console_class: ConsoleClass::new(),
        }
    }

    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Register all standard library functions systematically
        self.string_ops.register_functions(codegen)?;
        self.numeric_ops.register_functions(codegen)?;
        self.matrix_ops.register_functions(codegen)?;
        self.type_conv.register_functions(codegen)?;
        self.math_class.register_functions(codegen)?;
        self.string_class.register_functions(codegen)?;
        self.list_class.register_functions(codegen)?;
        self.file_class.register_functions(codegen)?;
        self.http_class.register_functions(codegen)?;
        self.list_behavior.register_functions(codegen)?;
        self.method_style.register_functions(codegen)?;
        self.conditional.register_functions(codegen)?;
        self.multiline.register_functions(codegen)?;
        self.error_handling.register_functions(codegen)?;
        self.test_framework.register_functions(codegen)?;
        self.string_interpolation.register_functions(codegen)?;
        self.default_parameters.register_functions(codegen)?;
        self.numeric_literals.register_functions(codegen)?;
        self.matrix_literals.register_functions(codegen)?;
        self.pairs_type.register_functions(codegen)?;
        self.string_advanced.register_functions(codegen)?;
        self.list_advanced.register_functions(codegen)?;
        self.math_advanced.register_functions(codegen)?;
        self.file_advanced.register_functions(codegen)?;
        self.http_advanced.register_functions(codegen)?;
        self.console_input.register_functions(codegen)?;
        self.async_programming.register_functions(codegen)?;
        self.import_system.register_functions(codegen)?;
        self.console_ops.register_functions(codegen)?;
        self.console_class.register_functions(codegen)?;
        // All standard library functions registered successfully
        Ok(())
    }
}

/// Runtime environment for Clean Language execution
pub struct Runtime {
    pub memory: MemoryManager,
    pub strings: StringManager,
    pub lists: ListManager,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            memory: MemoryManager::new(16, Some(HEAP_START as u32)),
            strings: StringManager::new(MemoryManager::new(16, Some(HEAP_START as u32))),
            lists: ListManager::new(Rc::new(RefCell::new(MemoryManager::new(
                16,
                Some(HEAP_START as u32),
            )))),
        }
    }

    pub fn reset(&mut self) {
        self.memory = MemoryManager::new(16, Some(HEAP_START as u32));
        self.strings = StringManager::new(MemoryManager::new(16, Some(HEAP_START as u32)));
        self.lists = ListManager::new(Rc::new(RefCell::new(MemoryManager::new(
            16,
            Some(HEAP_START as u32),
        ))));
    }
}

/// Simplified standard library interface
pub struct StdLib {
    pub runtime: Runtime,
    pub numeric_ops: NumericOperations,
    pub type_conv: TypeConvOperations,
}

impl Default for StdLib {
    fn default() -> Self {
        Self::new()
    }
}

impl StdLib {
    pub fn new() -> Self {
        Self {
            runtime: Runtime::new(),
            numeric_ops: NumericOperations::new(),
            type_conv: TypeConvOperations::new(HEAP_START),
        }
    }

    pub fn reset(&mut self) {
        self.runtime.reset();
    }

    pub fn register_functions(&mut self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        self.runtime.strings.register_functions(codegen)?;
        self.runtime.lists.register_functions(codegen)?;
        self.numeric_ops.register_functions(codegen)?;
        self.type_conv.register_functions(codegen)?;
        Ok(())
    }

    pub fn allocate_string(&mut self, data: &[u8]) -> Result<usize, CompilerError> {
        // Simplified string allocation
        self.runtime
            .memory
            .allocate(data.len() + 4, 3)
            .map_err(|e| {
                CompilerError::runtime_error(format!("String allocation failed: {e}"), None, None)
            })
    }

    pub fn get_string_from_memory(&self, _ptr: usize) -> Result<String, CompilerError> {
        // Simplified string retrieval - return empty string for now
        Ok(String::new())
    }
}

/// Helper function to register stdlib functions
pub(crate) fn register_stdlib_function(
    codegen: &mut CodeGenerator,
    name: &str,
    params: &[WasmType],
    return_type: Option<WasmType>,
    instructions: Vec<Instruction>,
) -> Result<u32, CompilerError> {
    // Function registration completed successfully
    let function_index = codegen.register_function(name, params, return_type, &instructions)?;

    // Function registration completed successfully

    Ok(function_index)
}

/// Enhanced helper function to register stdlib functions with explicit local variable types
pub(crate) fn register_stdlib_function_with_locals(
    codegen: &mut CodeGenerator,
    name: &str,
    params: &[WasmType],
    return_type: Option<WasmType>,
    local_types: &[WasmType], // Additional local variable types beyond parameters
    instructions: Vec<Instruction>,
) -> Result<u32, CompilerError> {
    // Function registration with locals completed successfully
    let function_index = codegen.register_function_with_locals(
        name,
        params,
        return_type,
        local_types,
        &instructions,
    )?;

    // Function registration with locals completed successfully

    Ok(function_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdlib_creation() {
        let _stdlib = StdLib::new();
        // Basic test to ensure creation works
    }

    #[test]
    fn test_runtime_creation() {
        let _runtime = Runtime::new();
        // Basic test to ensure creation works
    }

    #[test]
    fn test_memory_allocation() -> Result<(), CompilerError> {
        let mut memory = MemoryManager::new(16, Some(1024));
        assert!(memory.allocate(100, 1).is_ok());
        Ok(())
    }
}
