use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::memory::MemoryManager;
use std::cell::RefCell;
use std::rc::Rc;

/// Conditional expressions implementation for Clean Language
/// NOTE: The compare.*, conditional.*, and logical.* namespace functions have been
/// removed from the language. This manager is kept for backward compatibility with
/// existing code that instantiates it, but no longer registers any functions.
pub struct ConditionalManager {
    #[allow(dead_code)]
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl ConditionalManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register functions (currently does nothing as all conditional/compare/logical
    /// namespaces have been removed from the language)
    pub fn register_functions(&self, _codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // No functions to register - compare.*, conditional.*, and logical.* namespaces removed
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conditional_manager_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(1, Some(10))));
        let conditional_manager = ConditionalManager::new(memory_manager.clone());

        // Test that manager is created successfully
        assert_eq!(
            conditional_manager
                .memory_manager
                .borrow()
                .get_total_allocated(),
            0
        );
    }
}
