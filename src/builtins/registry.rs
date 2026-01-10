//! Builtin Registry - Single Source of Truth for Builtin Functions
//!
//! This module defines all builtin functions, classes, methods, and namespaces
//! used by the Clean Language compiler. It provides conversion methods for
//! each compiler stage (Resolver, TypeChecker, CodeGen).

use std::collections::HashMap;

/// Simplified type representation for builtins
/// Can be converted to HirType, ConcreteType, or WasmType as needed
#[derive(Debug, Clone, PartialEq)]
pub enum BuiltinType {
    Integer,
    Number,
    String,
    Boolean,
    Void,
    List(Box<BuiltinType>),
    Matrix(Box<BuiltinType>),
    Pairs(Box<BuiltinType>, Box<BuiltinType>),
    Namespace,
    Any, // For generic functions
}

impl BuiltinType {
    /// Convert to HirType for resolver stage
    pub fn to_hir_type(&self) -> crate::hir::HirType {
        match self {
            BuiltinType::Integer => crate::hir::HirType::Integer,
            BuiltinType::Number => crate::hir::HirType::Number,
            BuiltinType::String => crate::hir::HirType::String,
            BuiltinType::Boolean => crate::hir::HirType::Boolean,
            BuiltinType::Void => crate::hir::HirType::Void,
            BuiltinType::List(inner) => crate::hir::HirType::List(Box::new(inner.to_hir_type())),
            BuiltinType::Matrix(inner) => {
                crate::hir::HirType::Matrix(Box::new(inner.to_hir_type()))
            }
            BuiltinType::Pairs(k, v) => {
                crate::hir::HirType::Pairs(Box::new(k.to_hir_type()), Box::new(v.to_hir_type()))
            }
            BuiltinType::Namespace => crate::hir::HirType::Void, // Namespace is a special case
            BuiltinType::Any => crate::hir::HirType::Any,        // Dynamic type for JSON values
        }
    }

    /// Convert to ConcreteType for type checker stage
    pub fn to_concrete_type(&self) -> crate::typechecker::ConcreteType {
        use crate::typechecker::ConcreteType;
        match self {
            BuiltinType::Integer => ConcreteType::Integer,
            BuiltinType::Number => ConcreteType::Number,
            BuiltinType::String => ConcreteType::String,
            BuiltinType::Boolean => ConcreteType::Boolean,
            BuiltinType::Void => ConcreteType::Null,
            BuiltinType::List(inner) => ConcreteType::Array(Box::new(inner.to_concrete_type())),
            BuiltinType::Matrix(inner) => ConcreteType::Matrix(Box::new(inner.to_concrete_type())),
            BuiltinType::Pairs(k, v) => ConcreteType::Pairs(
                Box::new(k.to_concrete_type()),
                Box::new(v.to_concrete_type()),
            ),
            BuiltinType::Namespace => ConcreteType::Namespace,
            BuiltinType::Any => ConcreteType::Any, // Dynamic type with runtime type tag
        }
    }
}

/// Categories for organizing builtin functions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinCategory {
    IO,        // print, println, input
    Math,      // abs, sqrt, pow, sin, cos, etc.
    String,    // string operations
    List,      // list operations
    Type,      // type conversion
    Compare,   // comparison operations
    Logical,   // logical operations
    File,      // file operations
    Http,      // HTTP operations
    Memory,    // memory management
    Condition, // conditional operations
    Json,      // JSON operations  // BOOK: json-module
}

/// A builtin function definition
#[derive(Debug, Clone)]
pub struct BuiltinFunction {
    pub name: String,
    pub parameters: Vec<BuiltinType>,
    pub return_type: BuiltinType,
    pub category: BuiltinCategory,
    /// Optional WASM import module.function name
    pub wasm_import: Option<(String, String)>,
}

impl BuiltinFunction {
    pub fn new(
        name: &str,
        parameters: Vec<BuiltinType>,
        return_type: BuiltinType,
        category: BuiltinCategory,
    ) -> Self {
        Self {
            name: name.to_string(),
            parameters,
            return_type,
            category,
            wasm_import: None,
        }
    }

    pub fn with_wasm_import(mut self, module: &str, function: &str) -> Self {
        self.wasm_import = Some((module.to_string(), function.to_string()));
        self
    }
}

/// A builtin method definition (attached to a class)
#[derive(Debug, Clone)]
pub struct BuiltinMethod {
    pub name: String,
    pub parameters: Vec<BuiltinType>,
    pub return_type: BuiltinType,
    pub is_static: bool,
}

impl BuiltinMethod {
    pub fn new(name: &str, parameters: Vec<BuiltinType>, return_type: BuiltinType) -> Self {
        Self {
            name: name.to_string(),
            parameters,
            return_type,
            is_static: true, // Most builtin methods are static
        }
    }

    pub fn instance(name: &str, parameters: Vec<BuiltinType>, return_type: BuiltinType) -> Self {
        Self {
            name: name.to_string(),
            parameters,
            return_type,
            is_static: false,
        }
    }
}

/// A builtin class definition
#[derive(Debug, Clone)]
pub struct BuiltinClass {
    pub name: String,
    pub methods: Vec<BuiltinMethod>,
}

impl BuiltinClass {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            methods: Vec::new(),
        }
    }

    pub fn with_methods(mut self, methods: Vec<BuiltinMethod>) -> Self {
        self.methods = methods;
        self
    }
}

/// A builtin namespace definition (e.g., math, string, list)
#[derive(Debug, Clone)]
pub struct BuiltinNamespace {
    pub name: String,
    pub functions: Vec<BuiltinFunction>,
}

impl BuiltinNamespace {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            functions: Vec::new(),
        }
    }

    pub fn with_functions(mut self, functions: Vec<BuiltinFunction>) -> Self {
        self.functions = functions;
        self
    }
}

/// The centralized registry for all builtins
#[derive(Debug, Clone)]
pub struct BuiltinRegistry {
    pub functions: HashMap<String, BuiltinFunction>,
    pub classes: HashMap<String, BuiltinClass>,
    pub namespaces: HashMap<String, BuiltinNamespace>,
}

impl Default for BuiltinRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BuiltinRegistry {
    /// Create a new registry with all builtin definitions
    pub fn new() -> Self {
        let mut registry = Self {
            functions: HashMap::new(),
            classes: HashMap::new(),
            namespaces: HashMap::new(),
        };

        registry.register_io_functions();
        registry.register_math_functions();
        registry.register_type_conversion_functions();
        registry.register_math_class();
        registry.register_string_utils_class();
        registry.register_string_class();
        registry.register_integer_class();
        registry.register_math_namespace();
        registry.register_string_namespace();
        registry.register_list_namespace();
        registry.register_compare_namespace();
        registry.register_conditional_namespace();
        registry.register_logical_namespace();
        registry.register_json_namespace(); // BOOK: json-module

        registry
    }

    /// Register I/O functions (print, println, input, etc.)
    fn register_io_functions(&mut self) {
        let io_functions = vec![
            BuiltinFunction::new(
                "print",
                vec![BuiltinType::String],
                BuiltinType::Void,
                BuiltinCategory::IO,
            )
            .with_wasm_import("env", "print"),
            BuiltinFunction::new(
                "println",
                vec![BuiltinType::String],
                BuiltinType::Void,
                BuiltinCategory::IO,
            )
            .with_wasm_import("env", "println"),
            BuiltinFunction::new(
                "printl",
                vec![BuiltinType::String],
                BuiltinType::Void,
                BuiltinCategory::IO,
            )
            .with_wasm_import("env", "printl"),
            BuiltinFunction::new(
                "input",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::IO,
            )
            .with_wasm_import("env", "input"),
            BuiltinFunction::new(
                "inputInteger",
                vec![BuiltinType::String],
                BuiltinType::Integer,
                BuiltinCategory::IO,
            )
            .with_wasm_import("env", "input_integer"),
            BuiltinFunction::new(
                "inputNumber",
                vec![BuiltinType::String],
                BuiltinType::Number,
                BuiltinCategory::IO,
            )
            .with_wasm_import("env", "input_float"),
        ];

        for func in io_functions {
            self.functions.insert(func.name.clone(), func);
        }
    }

    /// Register global math functions (abs, sqrt, pow, etc.)
    fn register_math_functions(&mut self) {
        let math_functions = vec![
            BuiltinFunction::new(
                "abs",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "max",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "min",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "sqrt",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "pow",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
        ];

        for func in math_functions {
            self.functions.insert(func.name.clone(), func);
        }
    }

    /// Register type conversion functions
    fn register_type_conversion_functions(&mut self) {
        let type_functions = vec![
            BuiltinFunction::new(
                "toString",
                vec![BuiltinType::Integer],
                BuiltinType::String,
                BuiltinCategory::Type,
            ),
            BuiltinFunction::new(
                "toInteger",
                vec![BuiltinType::String],
                BuiltinType::Integer,
                BuiltinCategory::Type,
            ),
        ];

        for func in type_functions {
            self.functions.insert(func.name.clone(), func);
        }
    }

    /// Register Math class with static methods
    fn register_math_class(&mut self) {
        let math_class = BuiltinClass::new("Math").with_methods(vec![
            BuiltinMethod::new("abs", vec![BuiltinType::Number], BuiltinType::Number),
            BuiltinMethod::new("floor", vec![BuiltinType::Number], BuiltinType::Integer),
            BuiltinMethod::new("ceil", vec![BuiltinType::Number], BuiltinType::Integer),
            BuiltinMethod::new("round", vec![BuiltinType::Number], BuiltinType::Integer),
            BuiltinMethod::new("sqrt", vec![BuiltinType::Number], BuiltinType::Number),
            BuiltinMethod::new(
                "pow",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
            ),
            BuiltinMethod::new("sin", vec![BuiltinType::Number], BuiltinType::Number),
            BuiltinMethod::new("cos", vec![BuiltinType::Number], BuiltinType::Number),
            BuiltinMethod::new("tan", vec![BuiltinType::Number], BuiltinType::Number),
            BuiltinMethod::new(
                "max",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
            ),
            BuiltinMethod::new(
                "min",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
            ),
        ]);

        self.classes.insert("Math".to_string(), math_class);
    }

    /// Register StringUtils class with static methods
    fn register_string_utils_class(&mut self) {
        let stringutils_class = BuiltinClass::new("StringUtils").with_methods(vec![
            BuiltinMethod::new("length", vec![BuiltinType::String], BuiltinType::Integer),
            BuiltinMethod::new(
                "concat",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::String,
            ),
            BuiltinMethod::new(
                "substring",
                vec![
                    BuiltinType::String,
                    BuiltinType::Integer,
                    BuiltinType::Integer,
                ],
                BuiltinType::String,
            ),
            BuiltinMethod::new(
                "indexOf",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::Integer,
            ),
            // 3-arg indexOf with startIndex for searching from a specific position
            BuiltinMethod::new(
                "indexOf",
                vec![
                    BuiltinType::String,
                    BuiltinType::String,
                    BuiltinType::Integer,
                ],
                BuiltinType::Integer,
            ),
            BuiltinMethod::new(
                "replace",
                vec![
                    BuiltinType::String,
                    BuiltinType::String,
                    BuiltinType::String,
                ],
                BuiltinType::String,
            ),
            BuiltinMethod::new(
                "toUpperCase",
                vec![BuiltinType::String],
                BuiltinType::String,
            ),
            BuiltinMethod::new(
                "toLowerCase",
                vec![BuiltinType::String],
                BuiltinType::String,
            ),
        ]);

        self.classes
            .insert("StringUtils".to_string(), stringutils_class);
    }

    /// Register String class with static methods
    fn register_string_class(&mut self) {
        let string_class = BuiltinClass::new("String").with_methods(vec![
            BuiltinMethod::new(
                "fromCharCode",
                vec![BuiltinType::Integer],
                BuiltinType::String,
            ),
            BuiltinMethod::new("isEmpty", vec![BuiltinType::String], BuiltinType::Boolean),
        ]);

        self.classes.insert("String".to_string(), string_class);
    }

    /// Register Integer class with static methods
    fn register_integer_class(&mut self) {
        let integer_class = BuiltinClass::new("Integer").with_methods(vec![BuiltinMethod::new(
            "parse",
            vec![BuiltinType::String],
            BuiltinType::Integer,
        )]);

        self.classes.insert("Integer".to_string(), integer_class);
    }

    /// Register math namespace (math.sin, math.cos, etc.)
    fn register_math_namespace(&mut self) {
        let math_ns = BuiltinNamespace::new("math").with_functions(vec![
            // Trigonometric
            BuiltinFunction::new(
                "sin",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "cos",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "tan",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "asin",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "acos",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "atan",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "atan2",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            // Hyperbolic
            BuiltinFunction::new(
                "sinh",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "cosh",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "tanh",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            // Logarithmic
            BuiltinFunction::new(
                "log",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "ln",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "log10",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "log2",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "exp",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "exp2",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            // Basic math
            BuiltinFunction::new(
                "abs",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "floor",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "ceil",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "round",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "sqrt",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "trunc",
                vec![BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            // Constants (no parameters)
            BuiltinFunction::new("pi", vec![], BuiltinType::Number, BuiltinCategory::Math),
            BuiltinFunction::new("e", vec![], BuiltinType::Number, BuiltinCategory::Math),
            BuiltinFunction::new("tau", vec![], BuiltinType::Number, BuiltinCategory::Math),
            // Binary operations
            BuiltinFunction::new(
                "pow",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "max",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "min",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "add",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "subtract",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "multiply",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
            BuiltinFunction::new(
                "divide",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Number,
                BuiltinCategory::Math,
            ),
        ]);

        self.namespaces.insert("math".to_string(), math_ns);
    }

    /// Register string namespace (string.length, string.concat, etc.)
    fn register_string_namespace(&mut self) {
        let string_ns = BuiltinNamespace::new("string").with_functions(vec![
            BuiltinFunction::new(
                "length",
                vec![BuiltinType::String],
                BuiltinType::Integer,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "concat",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "isEmpty",
                vec![BuiltinType::String],
                BuiltinType::Boolean,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "isBlank",
                vec![BuiltinType::String],
                BuiltinType::Boolean,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "indexOf",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::Integer,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "lastIndexOf",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::Integer,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "substring",
                vec![
                    BuiltinType::String,
                    BuiltinType::Integer,
                    BuiltinType::Integer,
                ],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "toUpperCase",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "toLowerCase",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "trim",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "trimStart",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "trimEnd",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "split",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::List(Box::new(BuiltinType::String)),
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "join",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::String)),
                    BuiltinType::String,
                ],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "padStart",
                vec![
                    BuiltinType::String,
                    BuiltinType::Integer,
                    BuiltinType::String,
                ],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "padEnd",
                vec![
                    BuiltinType::String,
                    BuiltinType::Integer,
                    BuiltinType::String,
                ],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "charAt",
                vec![BuiltinType::String, BuiltinType::Integer],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "charCodeAt",
                vec![BuiltinType::String, BuiltinType::Integer],
                BuiltinType::Integer,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "contains",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::Boolean,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "startsWith",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::Boolean,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "endsWith",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::Boolean,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "replace",
                vec![
                    BuiltinType::String,
                    BuiltinType::String,
                    BuiltinType::String,
                ],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
            BuiltinFunction::new(
                "replaceAll",
                vec![
                    BuiltinType::String,
                    BuiltinType::String,
                    BuiltinType::String,
                ],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
        ]);

        self.namespaces.insert("string".to_string(), string_ns);
    }

    /// Register list namespace (list.size, list.push, etc.)
    fn register_list_namespace(&mut self) {
        let list_ns = BuiltinNamespace::new("list").with_functions(vec![
            BuiltinFunction::new(
                "size",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Integer,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "isEmpty",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Boolean,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "isNotEmpty",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Boolean,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "push",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::Void,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "pop",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Any,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "first",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Any,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "last",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Any,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "get",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Integer,
                ],
                BuiltinType::Any,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "set",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Integer,
                    BuiltinType::Any,
                ],
                BuiltinType::Void,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "clear",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Void,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "reverse",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::List(Box::new(BuiltinType::Any)),
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "fill",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::Void,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "contains",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::Boolean,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "indexOf",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::Integer,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "slice",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Integer,
                    BuiltinType::Integer,
                ],
                BuiltinType::List(Box::new(BuiltinType::Any)),
                BuiltinCategory::List,
            ),
        ]);

        self.namespaces.insert("list".to_string(), list_ns);
    }

    /// Register compare namespace
    fn register_compare_namespace(&mut self) {
        let compare_ns = BuiltinNamespace::new("compare").with_functions(vec![
            BuiltinFunction::new(
                "equals",
                vec![BuiltinType::Any, BuiltinType::Any],
                BuiltinType::Boolean,
                BuiltinCategory::Compare,
            ),
            BuiltinFunction::new(
                "notEquals",
                vec![BuiltinType::Any, BuiltinType::Any],
                BuiltinType::Boolean,
                BuiltinCategory::Compare,
            ),
            BuiltinFunction::new(
                "lessThan",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Boolean,
                BuiltinCategory::Compare,
            ),
            BuiltinFunction::new(
                "lessThanOrEqual",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Boolean,
                BuiltinCategory::Compare,
            ),
            BuiltinFunction::new(
                "greaterThan",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Boolean,
                BuiltinCategory::Compare,
            ),
            BuiltinFunction::new(
                "greaterThanOrEqual",
                vec![BuiltinType::Number, BuiltinType::Number],
                BuiltinType::Boolean,
                BuiltinCategory::Compare,
            ),
        ]);

        self.namespaces.insert("compare".to_string(), compare_ns);
    }

    /// Register conditional namespace
    fn register_conditional_namespace(&mut self) {
        let conditional_ns = BuiltinNamespace::new("conditional").with_functions(vec![
            BuiltinFunction::new(
                "if",
                vec![BuiltinType::Boolean, BuiltinType::Any, BuiltinType::Any],
                BuiltinType::Any,
                BuiltinCategory::Condition,
            ),
            BuiltinFunction::new(
                "when",
                vec![BuiltinType::Boolean, BuiltinType::Any],
                BuiltinType::Any,
                BuiltinCategory::Condition,
            ),
            BuiltinFunction::new(
                "unless",
                vec![BuiltinType::Boolean, BuiltinType::Any],
                BuiltinType::Any,
                BuiltinCategory::Condition,
            ),
        ]);

        self.namespaces
            .insert("conditional".to_string(), conditional_ns);
    }

    /// Register logical namespace
    fn register_logical_namespace(&mut self) {
        let logical_ns = BuiltinNamespace::new("logical").with_functions(vec![
            BuiltinFunction::new(
                "and",
                vec![BuiltinType::Boolean, BuiltinType::Boolean],
                BuiltinType::Boolean,
                BuiltinCategory::Logical,
            ),
            BuiltinFunction::new(
                "or",
                vec![BuiltinType::Boolean, BuiltinType::Boolean],
                BuiltinType::Boolean,
                BuiltinCategory::Logical,
            ),
            BuiltinFunction::new(
                "not",
                vec![BuiltinType::Boolean],
                BuiltinType::Boolean,
                BuiltinCategory::Logical,
            ),
            BuiltinFunction::new(
                "xor",
                vec![BuiltinType::Boolean, BuiltinType::Boolean],
                BuiltinType::Boolean,
                BuiltinCategory::Logical,
            ),
        ]);

        self.namespaces.insert("logical".to_string(), logical_ns);
    }

    /// Register json namespace (json.textToData, json.dataToText, etc.)
    /// BOOK: json-module - pure WASM JSON implementation
    fn register_json_namespace(&mut self) {
        let json_ns = BuiltinNamespace::new("json").with_functions(vec![
            // Parse JSON text into data structure
            // Returns Any type that supports bracket notation access
            BuiltinFunction::new(
                "textToData",
                vec![BuiltinType::String],
                BuiltinType::Any, // Returns Any for dynamic JSON access
                BuiltinCategory::Json,
            ),
            // Parse JSON text, returns null (0) on error
            BuiltinFunction::new(
                "tryTextToData",
                vec![BuiltinType::String],
                BuiltinType::Any, // Returns Any or null (0) on error
                BuiltinCategory::Json,
            ),
            // Convert data to JSON text
            BuiltinFunction::new(
                "dataToText",
                vec![BuiltinType::Any], // Takes Any (JSON value)
                BuiltinType::String,
                BuiltinCategory::Json,
            ),
            // Convert data to formatted JSON text
            BuiltinFunction::new(
                "prettyDataToText",
                vec![BuiltinType::Any], // Takes Any (JSON value)
                BuiltinType::String,
                BuiltinCategory::Json,
            ),
        ]);

        self.namespaces.insert("json".to_string(), json_ns);
    }

    // ============== Query Methods ==============

    /// Get a function by name
    pub fn get_function(&self, name: &str) -> Option<&BuiltinFunction> {
        self.functions.get(name)
    }

    /// Get a class by name
    pub fn get_class(&self, name: &str) -> Option<&BuiltinClass> {
        self.classes.get(name)
    }

    /// Get a namespace by name
    pub fn get_namespace(&self, name: &str) -> Option<&BuiltinNamespace> {
        self.namespaces.get(name)
    }

    /// Get a namespace function (e.g., "math.sin" -> sin function from math namespace)
    pub fn get_namespace_function(
        &self,
        namespace: &str,
        function: &str,
    ) -> Option<&BuiltinFunction> {
        self.namespaces
            .get(namespace)
            .and_then(|ns| ns.functions.iter().find(|f| f.name == function))
    }

    /// Get a class method
    pub fn get_class_method(&self, class: &str, method: &str) -> Option<&BuiltinMethod> {
        self.classes
            .get(class)
            .and_then(|c| c.methods.iter().find(|m| m.name == method))
    }

    /// List all namespace names
    pub fn namespace_names(&self) -> Vec<&str> {
        self.namespaces.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a name is a namespace
    pub fn is_namespace(&self, name: &str) -> bool {
        self.namespaces.contains_key(name)
    }

    /// Get the full qualified name for a namespace function (e.g., "math_sin" for resolver)
    pub fn get_namespace_function_resolver_name(&self, namespace: &str, function: &str) -> String {
        format!("{}_{}", namespace, function)
    }

    /// Get the full qualified name for a namespace function (e.g., "math.sin" for dot notation)
    pub fn get_namespace_function_dot_name(&self, namespace: &str, function: &str) -> String {
        format!("{}.{}", namespace, function)
    }

    // ============== Resolver Integration Helpers ==============

    /// Get all global functions as (name, params, return_type) tuples for resolver
    pub fn get_global_functions_for_resolver(
        &self,
    ) -> Vec<(
        String,
        Vec<crate::hir::HirType>,
        Option<crate::hir::HirType>,
    )> {
        self.functions
            .values()
            .map(|f| {
                let params: Vec<crate::hir::HirType> =
                    f.parameters.iter().map(|p| p.to_hir_type()).collect();
                let ret = if matches!(f.return_type, BuiltinType::Void) {
                    Some(crate::hir::HirType::Void)
                } else {
                    Some(f.return_type.to_hir_type())
                };
                (f.name.clone(), params, ret)
            })
            .collect()
    }

    /// Get all namespace functions as (namespace.function, params, return_type) tuples
    pub fn get_namespace_functions_for_resolver(
        &self,
    ) -> Vec<(String, Vec<crate::hir::HirType>, crate::hir::HirType)> {
        let mut result = Vec::new();
        for (ns_name, namespace) in &self.namespaces {
            for func in &namespace.functions {
                let full_name = format!("{}.{}", ns_name, func.name);
                let params: Vec<crate::hir::HirType> =
                    func.parameters.iter().map(|p| p.to_hir_type()).collect();
                let ret = func.return_type.to_hir_type();
                result.push((full_name, params, ret));
            }
        }
        result
    }

    /// Get all class methods as (class_name, method_name, params, return_type) tuples
    pub fn get_class_methods_for_resolver(
        &self,
    ) -> Vec<(
        String,
        String,
        Vec<crate::hir::HirType>,
        crate::hir::HirType,
    )> {
        let mut result = Vec::new();
        for (class_name, class) in &self.classes {
            for method in &class.methods {
                let params: Vec<crate::hir::HirType> =
                    method.parameters.iter().map(|p| p.to_hir_type()).collect();
                let ret = method.return_type.to_hir_type();
                result.push((class_name.clone(), method.name.clone(), params, ret));
            }
        }
        result
    }

    // ============== Type Checker Integration Helpers ==============

    /// Get a function's type signature as a ConcreteType::Function
    pub fn get_function_type(&self, name: &str) -> Option<crate::typechecker::ConcreteType> {
        self.functions
            .get(name)
            .map(|f| crate::typechecker::ConcreteType::Function {
                parameters: f.parameters.iter().map(|p| p.to_concrete_type()).collect(),
                return_type: Box::new(f.return_type.to_concrete_type()),
                is_async: false,
            })
    }

    /// Get a namespace function's type signature
    pub fn get_namespace_function_type(
        &self,
        namespace: &str,
        function: &str,
    ) -> Option<crate::typechecker::ConcreteType> {
        self.get_namespace_function(namespace, function).map(|f| {
            crate::typechecker::ConcreteType::Function {
                parameters: f.parameters.iter().map(|p| p.to_concrete_type()).collect(),
                return_type: Box::new(f.return_type.to_concrete_type()),
                is_async: false,
            }
        })
    }

    // ============== Plugin Bridge Functions ==============

    /// Register bridge functions from a plugin manifest
    /// These functions are defined in plugin.toml [bridge] section
    pub fn register_plugin_bridge_functions(
        &mut self,
        bridge: &crate::plugins::plugin_abi::PluginBridge,
    ) {
        for func in &bridge.functions {
            let builtin = func.to_builtin_function();
            tracing::debug!(
                "Registering plugin bridge function: {} with {} params, returns {:?}",
                builtin.name,
                builtin.parameters.len(),
                builtin.return_type
            );
            self.functions.insert(builtin.name.clone(), builtin);
        }
    }

    /// Register a single bridge function
    pub fn register_bridge_function(&mut self, func: &crate::plugins::plugin_abi::BridgeFunction) {
        let builtin = func.to_builtin_function();
        self.functions.insert(builtin.name.clone(), builtin);
    }

    // ============== Statistics ==============

    /// Get count of all registered items
    pub fn stats(&self) -> (usize, usize, usize, usize) {
        let namespace_functions: usize =
            self.namespaces.values().map(|ns| ns.functions.len()).sum();
        let class_methods: usize = self.classes.values().map(|c| c.methods.len()).sum();
        (
            self.functions.len(),
            self.classes.len(),
            self.namespaces.len(),
            namespace_functions + class_methods,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = BuiltinRegistry::new();

        // Check that basic functions exist
        assert!(registry.get_function("print").is_some());
        assert!(registry.get_function("println").is_some());
        assert!(registry.get_function("abs").is_some());

        // Check classes
        assert!(registry.get_class("Math").is_some());
        assert!(registry.get_class("StringUtils").is_some());

        // Check namespaces
        assert!(registry.get_namespace("math").is_some());
        assert!(registry.get_namespace("string").is_some());
        assert!(registry.get_namespace("list").is_some());
    }

    #[test]
    fn test_namespace_function_lookup() {
        let registry = BuiltinRegistry::new();

        let sin = registry.get_namespace_function("math", "sin");
        assert!(sin.is_some());
        assert_eq!(sin.unwrap().name, "sin");
        assert_eq!(sin.unwrap().parameters.len(), 1);
    }

    #[test]
    fn test_class_method_lookup() {
        let registry = BuiltinRegistry::new();

        let floor = registry.get_class_method("Math", "floor");
        assert!(floor.is_some());
        assert_eq!(floor.unwrap().name, "floor");
    }

    #[test]
    fn test_type_conversion_to_hir() {
        let ty = BuiltinType::List(Box::new(BuiltinType::String));
        let hir_type = ty.to_hir_type();

        match hir_type {
            crate::hir::HirType::List(inner) => {
                assert_eq!(*inner, crate::hir::HirType::String);
            }
            _ => panic!("Expected List type"),
        }
    }
}
