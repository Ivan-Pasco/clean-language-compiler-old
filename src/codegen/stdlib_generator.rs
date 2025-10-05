//! Standard Library Generator for the Clean Language compiler.
//!
//! This module provides functionality to register all standard library
//! functions including numeric operations, string operations, array/list operations,
//! matrix operations, file I/O, HTTP operations, and type conversions.
//!
//! Note: Function registry and metadata methods are infrastructure for IDE integration.

#![allow(dead_code)]

use crate::error::CompilerError;
use std::collections::HashMap;

/// Registry for standard library functions.
/// This is a lightweight registry that tracks which stdlib functions have been registered.
pub struct StdlibGenerator {
    registered_functions: HashMap<String, StdlibFunctionMeta>,
    registration_order: Vec<String>,
}

/// Metadata for a standard library function.
#[derive(Debug, Clone)]
pub struct StdlibFunctionMeta {
    pub name: String,
    pub category: StdlibCategory,
    pub description: String,
    pub is_registered: bool,
}

/// Categories of standard library functions.
#[derive(Debug, Clone, PartialEq)]
pub enum StdlibCategory {
    Math,
    String,
    List,
    Matrix,
    File,
    Http,
    Console,
    TypeConversion,
    Conditional,
    MethodStyle,
}

impl StdlibGenerator {
    /// Create a new standard library generator.
    pub fn new() -> Self {
        Self {
            registered_functions: HashMap::new(),
            registration_order: Vec::new(),
        }
    }

    /// Register a stdlib function by name and category.
    fn register_function_meta(&mut self, name: &str, category: StdlibCategory, description: &str) {
        let meta = StdlibFunctionMeta {
            name: name.to_string(),
            category,
            description: description.to_string(),
            is_registered: true,
        };

        self.registered_functions.insert(name.to_string(), meta);
        self.registration_order.push(name.to_string());
    }

    /// Register all standard library functions.
    pub fn register_stdlib_functions(&mut self) -> Result<(), CompilerError> {
        // Register core operations in order of dependency
        self.register_matrix_operations()?;
        self.register_numeric_operations()?;
        self.register_list_operations()?;
        self.register_type_conversion_operations()?;
        self.register_file_operations()?;
        self.register_basic_array_get_fallback()?;
        self.pre_allocate_conversion_strings()?;
        self.register_console_operations()?;
        self.register_http_operations()?;
        self.register_math_operations()?;
        self.register_string_class_operations()?;
        self.register_list_class_operations()?;
        self.register_conditional_operations()?;
        self.register_method_style_operations()?;

        Ok(())
    }

    /// Register matrix operations (matrix_add, matrix_multiply, etc.)
    pub fn register_matrix_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta("matrix_add", StdlibCategory::Matrix, "Add two matrices");
        self.register_function_meta(
            "matrix_multiply",
            StdlibCategory::Matrix,
            "Multiply two matrices",
        );
        self.register_function_meta(
            "matrix_transpose",
            StdlibCategory::Matrix,
            "Transpose a matrix",
        );
        Ok(())
    }

    /// Register file I/O operations (file_read, file_write, etc.)
    pub fn register_file_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta("file_read", StdlibCategory::File, "Read file contents");
        self.register_function_meta("file_write", StdlibCategory::File, "Write to file");
        Ok(())
    }

    /// Register numeric operations (add, subtract, multiply, divide, etc.)
    pub fn register_numeric_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta("abs", StdlibCategory::Math, "Absolute value");
        self.register_function_meta("min", StdlibCategory::Math, "Minimum of two numbers");
        self.register_function_meta("max", StdlibCategory::Math, "Maximum of two numbers");
        Ok(())
    }

    /// Register list/array operations (push, pop, length, etc.)
    pub fn register_list_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta("list_push", StdlibCategory::List, "Add element to list");
        self.register_function_meta(
            "list_pop",
            StdlibCategory::List,
            "Remove last element from list",
        );
        self.register_function_meta("list_length", StdlibCategory::List, "Get list length");
        Ok(())
    }

    /// Register type conversion operations (toString, toNumber, etc.)
    pub fn register_type_conversion_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta(
            "toString",
            StdlibCategory::TypeConversion,
            "Convert to string",
        );
        self.register_function_meta(
            "toNumber",
            StdlibCategory::TypeConversion,
            "Convert to number",
        );
        self.register_function_meta(
            "toBoolean",
            StdlibCategory::TypeConversion,
            "Convert to boolean",
        );
        Ok(())
    }

    /// Register console operations (print, println, input, etc.)
    pub fn register_console_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta("print", StdlibCategory::Console, "Print to console");
        self.register_function_meta("println", StdlibCategory::Console, "Print line to console");
        Ok(())
    }

    /// Register HTTP operations (http_get, http_post, etc.)
    pub fn register_http_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta("http_get", StdlibCategory::Http, "HTTP GET request");
        self.register_function_meta("http_post", StdlibCategory::Http, "HTTP POST request");
        Ok(())
    }

    /// Register mathematical operations (sin, cos, sqrt, etc.)
    pub fn register_math_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta("sin", StdlibCategory::Math, "Sine function");
        self.register_function_meta("cos", StdlibCategory::Math, "Cosine function");
        self.register_function_meta("sqrt", StdlibCategory::Math, "Square root");
        Ok(())
    }

    /// Register string class operations (charAt, substring, etc.)
    pub fn register_string_class_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta(
            "string_charAt",
            StdlibCategory::String,
            "Get character at index",
        );
        self.register_function_meta(
            "string_substring",
            StdlibCategory::String,
            "Extract substring",
        );
        Ok(())
    }

    /// Register list class operations (add, remove, contains, etc.)
    pub fn register_list_class_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta(
            "list_contains",
            StdlibCategory::List,
            "Check if list contains value",
        );
        self.register_function_meta(
            "list_remove",
            StdlibCategory::List,
            "Remove element from list",
        );
        Ok(())
    }

    /// Register conditional operations (if-else helpers, etc.)
    pub fn register_conditional_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta(
            "conditional_select",
            StdlibCategory::Conditional,
            "Select value based on condition",
        );
        Ok(())
    }

    /// Register method-style operations (obj.method() syntax support)
    pub fn register_method_style_operations(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta(
            "method_call",
            StdlibCategory::MethodStyle,
            "Support method-style calls",
        );
        Ok(())
    }

    /// Register basic array access fallback functions
    pub fn register_basic_array_get_fallback(&mut self) -> Result<(), CompilerError> {
        self.register_function_meta(
            "array_get",
            StdlibCategory::List,
            "Array element access fallback",
        );
        Ok(())
    }

    /// Pre-allocate strings used in type conversion
    pub fn pre_allocate_conversion_strings(&mut self) -> Result<(), CompilerError> {
        // Pre-allocation is handled by the main compiler now
        Ok(())
    }

    /// Get the number of registered functions.
    pub fn get_function_count(&self) -> u32 {
        self.registered_functions.len() as u32
    }

    /// Get metadata for a specific function.
    pub fn get_function_meta(&self, name: &str) -> Option<&StdlibFunctionMeta> {
        self.registered_functions.get(name)
    }

    /// Get all registered function names.
    pub fn get_registered_functions(&self) -> &[String] {
        &self.registration_order
    }

    /// Get functions by category.
    pub fn get_functions_by_category(&self, category: StdlibCategory) -> Vec<&StdlibFunctionMeta> {
        self.registered_functions
            .values()
            .filter(|meta| meta.category == category)
            .collect()
    }

    /// Check if a function is registered.
    pub fn is_function_registered(&self, name: &str) -> bool {
        self.registered_functions.contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdlib_generator_creation() {
        let stdlib_gen = StdlibGenerator::new();
        assert_eq!(stdlib_gen.get_function_count(), 0);
        assert!(stdlib_gen.get_registered_functions().is_empty());
    }

    #[test]
    fn test_stdlib_function_registration() {
        let mut stdlib_gen = StdlibGenerator::new();

        // Test that registration doesn't fail
        assert!(stdlib_gen.register_stdlib_functions().is_ok());

        // Verify some functions are registered
        assert!(stdlib_gen.is_function_registered("sin"));
        assert!(stdlib_gen.is_function_registered("print"));
        assert!(stdlib_gen.is_function_registered("toString"));

        // Test function count
        assert!(stdlib_gen.get_function_count() > 0);
    }

    #[test]
    fn test_function_categories() {
        let mut stdlib_gen = StdlibGenerator::new();
        stdlib_gen.register_math_operations().unwrap();

        let math_functions = stdlib_gen.get_functions_by_category(StdlibCategory::Math);
        assert!(!math_functions.is_empty());

        // Verify specific math functions
        assert!(math_functions.iter().any(|f| f.name == "sin"));
        assert!(math_functions.iter().any(|f| f.name == "cos"));
    }

    #[test]
    fn test_function_metadata() {
        let mut stdlib_gen = StdlibGenerator::new();
        stdlib_gen.register_function_meta("test_func", StdlibCategory::Math, "Test function");

        let meta = stdlib_gen.get_function_meta("test_func").unwrap();
        assert_eq!(meta.name, "test_func");
        assert_eq!(meta.category, StdlibCategory::Math);
        assert_eq!(meta.description, "Test function");
        assert!(meta.is_registered);
    }
}
