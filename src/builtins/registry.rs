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
    IO,     // print, input
    Math,   // abs, sqrt, pow, sin, cos, etc.
    String, // string operations
    List,   // list operations
    Type,   // type conversion
    File,   // file operations
    Http,   // HTTP operations
    Memory, // memory management
    Json,   // JSON operations  // BOOK: json-module
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
        registry.register_integer_class();
        registry.register_math_namespace();
        registry.register_string_namespace();
        registry.register_list_namespace();
        registry.register_json_namespace(); // BOOK: json-module
        registry.register_file_namespace();
        registry.register_http_namespace();

        registry
    }

    /// Register I/O functions (print, input, etc.)
    /// Note: Use print("text") for no newline, print("text") + for newline
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
            BuiltinFunction::new(
                "inputYesNo",
                vec![BuiltinType::String],
                BuiltinType::Boolean,
                BuiltinCategory::IO,
            )
            .with_wasm_import("env", "input_yes_no"),
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
            BuiltinFunction::new(
                "toNumber",
                vec![BuiltinType::Any],
                BuiltinType::Number,
                BuiltinCategory::Type,
            ),
            BuiltinFunction::new(
                "toBoolean",
                vec![BuiltinType::Any],
                BuiltinType::Boolean,
                BuiltinCategory::Type,
            ),
        ];

        for func in type_functions {
            self.functions.insert(func.name.clone(), func);
        }
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
                "sign",
                vec![BuiltinType::Number],
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
            BuiltinFunction::new(
                "fromCharCode",
                vec![BuiltinType::Integer],
                BuiltinType::String,
                BuiltinCategory::String,
            ),
        ]);

        self.namespaces.insert("string".to_string(), string_ns);
    }

    /// Register list namespace (list.length, list.add, etc.)
    fn register_list_namespace(&mut self) {
        let list_ns = BuiltinNamespace::new("list").with_functions(vec![
            BuiltinFunction::new(
                "size",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Integer,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "length",
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
                "removeLast",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::Any,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "removeLast",
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
            BuiltinFunction::new(
                "add",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::List(Box::new(BuiltinType::Any)),
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "insert",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Integer,
                    BuiltinType::Any,
                ],
                BuiltinType::List(Box::new(BuiltinType::Any)),
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "remove",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Integer,
                ],
                BuiltinType::Any,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "lastIndexOf",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::Integer,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "concat",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                ],
                BuiltinType::List(Box::new(BuiltinType::Any)),
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "sort",
                vec![BuiltinType::List(Box::new(BuiltinType::Any))],
                BuiltinType::List(Box::new(BuiltinType::Any)),
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "map",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::List(Box::new(BuiltinType::Any)),
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "filter",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::List(Box::new(BuiltinType::Any)),
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "reduce",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                    BuiltinType::Any,
                ],
                BuiltinType::Any,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "forEach",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::Any)),
                    BuiltinType::Any,
                ],
                BuiltinType::Void,
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "range",
                vec![BuiltinType::Integer, BuiltinType::Integer],
                BuiltinType::List(Box::new(BuiltinType::Integer)),
                BuiltinCategory::List,
            ),
            BuiltinFunction::new(
                "join",
                vec![
                    BuiltinType::List(Box::new(BuiltinType::String)),
                    BuiltinType::String,
                ],
                BuiltinType::String,
                BuiltinCategory::List,
            ),
        ]);

        self.namespaces.insert("list".to_string(), list_ns);
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

    /// Register file namespace (file.read, file.write, etc.)
    fn register_file_namespace(&mut self) {
        let file_ns = BuiltinNamespace::new("file").with_functions(vec![
            BuiltinFunction::new(
                "read",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "lines",
                vec![BuiltinType::String],
                BuiltinType::List(Box::new(BuiltinType::String)),
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "write",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::Void,
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "append",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::Void,
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "exists",
                vec![BuiltinType::String],
                BuiltinType::Boolean,
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "delete",
                vec![BuiltinType::String],
                BuiltinType::Void,
                BuiltinCategory::IO,
            ),
        ]);

        self.namespaces.insert("file".to_string(), file_ns);
    }

    /// Register http namespace (http.get, http.post, etc.)
    fn register_http_namespace(&mut self) {
        let http_ns = BuiltinNamespace::new("http").with_functions(vec![
            BuiltinFunction::new(
                "get",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "post",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "put",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "patch",
                vec![BuiltinType::String, BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::IO,
            ),
            BuiltinFunction::new(
                "delete",
                vec![BuiltinType::String],
                BuiltinType::String,
                BuiltinCategory::IO,
            ),
        ]);

        self.namespaces.insert("http".to_string(), http_ns);
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
                is_background: false,
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
                is_background: false,
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
        assert!(registry.get_function("input").is_some());
        assert!(registry.get_function("abs").is_some());

        // Check classes
        assert!(registry.get_class("Integer").is_some());

        // Check namespaces
        assert!(registry.get_namespace("math").is_some());
        assert!(registry.get_namespace("string").is_some());
        assert!(registry.get_namespace("list").is_some());
        assert!(registry.get_namespace("json").is_some());

        // Verify removed namespaces are gone
        assert!(registry.get_namespace("compare").is_none());
        assert!(registry.get_namespace("conditional").is_none());
        assert!(registry.get_namespace("logical").is_none());

        // Verify removed classes are gone
        assert!(registry.get_class("Math").is_none());
        assert!(registry.get_class("StringUtils").is_none());
        assert!(registry.get_class("String").is_none());
    }

    #[test]
    fn test_namespace_function_lookup() {
        let registry = BuiltinRegistry::new();

        let sin = registry.get_namespace_function("math", "sin");
        assert!(sin.is_some());
        assert_eq!(sin.unwrap().name, "sin");
        assert_eq!(sin.unwrap().parameters.len(), 1);

        // Verify fromCharCode moved to string namespace
        let from_char = registry.get_namespace_function("string", "fromCharCode");
        assert!(from_char.is_some());

        // Verify math.add was removed (use + operator instead)
        assert!(registry.get_namespace_function("math", "add").is_none());
    }

    #[test]
    fn test_class_method_lookup() {
        let registry = BuiltinRegistry::new();

        let parse = registry.get_class_method("Integer", "parse");
        assert!(parse.is_some());
        assert_eq!(parse.unwrap().name, "parse");
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
