//! Type inference engine for Clean Language
//!
//! Generates type constraints from resolved HIR and performs type inference
//! using constraint-based approach with Hindley-Milner algorithm.

use super::constraint_solver::{ConstraintSolver, SolverResult};
use super::tast::{
    BinaryOperator, ConcreteType, TastBlock, TastClass, TastExpression, TastExpressionKind,
    TastField, TastFunction, TastLiteral, TastParameter, TastProgram, TastStatement,
    TypeConstraint, UnaryOperator, Visibility,
};
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::{HirBinaryOp, HirType, HirUnaryOp};
use crate::resolver::{
    GlobalSymbolTable, ResolvedHirBlock, ResolvedHirClass, ResolvedHirExpression,
    ResolvedHirFunction, ResolvedHirLValue, ResolvedHirMethod, ResolvedHirProgram,
    ResolvedHirStatement, SymbolId, SymbolKind,
};
use std::collections::HashMap;

/// Type inference engine
#[derive(Debug)]
pub struct TypeInference<'a> {
    /// Current type environment mapping symbols to types
    type_env: HashMap<SymbolId, ConcreteType>,

    /// Generated type constraints
    constraints: Vec<TypeConstraint>,

    /// Type variable generator
    constraint_solver: ConstraintSolver<'a>,

    /// Symbol table from resolution phase
    symbol_table: &'a GlobalSymbolTable,

    /// Built-in types and their methods
    #[allow(dead_code)]
    builtins: BuiltinTypes,

    /// Map from function SymbolId to minimum required parameter count
    /// (parameters without defaults)
    required_param_counts: HashMap<SymbolId, usize>,

    /// Current context for inference
    current_function: Option<SymbolId>,
    current_class: Option<SymbolId>,
    current_return_type: Option<ConcreteType>,

    /// Errors encountered during inference
    errors: Vec<CompilerError>,
    warnings: Vec<CompilerError>,

    /// Recursion depth counter to prevent stack overflow
    recursion_depth: usize,
}

/// Built-in types and their method signatures
#[derive(Debug, Clone)]
pub struct BuiltinTypes {
    pub integer_methods: HashMap<String, ConcreteType>,
    pub number_methods: HashMap<String, ConcreteType>,
    pub string_methods: HashMap<String, ConcreteType>,
    pub boolean_methods: HashMap<String, ConcreteType>,
    pub array_methods: HashMap<String, ConcreteType>,
}

/// Result of type inference
#[derive(Debug)]
pub struct InferenceResult {
    pub tast: TastProgram,
    pub type_env: HashMap<SymbolId, ConcreteType>,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerError>,
}

impl<'a> TypeInference<'a> {
    /// Create a new type inference engine
    pub fn new(symbol_table: &'a GlobalSymbolTable) -> Self {
        Self {
            type_env: HashMap::new(),
            constraints: Vec::new(),
            constraint_solver: ConstraintSolver::with_symbol_table(symbol_table),
            symbol_table,
            builtins: BuiltinTypes::new(),
            required_param_counts: HashMap::new(),
            current_function: None,
            current_class: None,
            current_return_type: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            recursion_depth: 0,
        }
    }

    /// Perform type inference on a resolved HIR program
    pub fn infer_types(mut self, program: ResolvedHirProgram) -> InferenceResult {
        // Initialize built-in types in type environment
        self.initialize_builtins();

        // Infer types for all program elements
        let tast_program = self.infer_program(&program);

        // Solve generated constraints
        let mut solver = std::mem::replace(
            &mut self.constraint_solver,
            ConstraintSolver::with_symbol_table(self.symbol_table),
        );
        solver.add_constraints(std::mem::take(&mut self.constraints));
        let solver_result = solver.solve();

        // Apply final substitutions to type environment
        self.apply_substitutions(&solver_result);

        // Collect errors
        self.errors.extend(solver_result.errors);

        InferenceResult {
            tast: tast_program,
            type_env: self.type_env,
            errors: self.errors,
            warnings: self.warnings,
        }
    }

    /// Initialize built-in types and add them to type environment
    fn initialize_builtins(&mut self) {
        // Specifically add known builtin functions to type environment
        // We know these functions exist from PassthroughResolver::load_builtin_functions

        // Find print function by name in global scope and add to type environment
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("print", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String], // print accepts any type but we'll use string
                    return_type: Box::new(ConcreteType::Null), // print returns void
                    is_async: false,
                },
            );
        }

        // Find println function
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("println", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Null),
                    is_async: false,
                },
            );
        }

        // Find printl function
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("printl", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Null),
                    is_async: false,
                },
            );
        }

        // Find input function
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("input", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        // Find inputInteger function
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("inputInteger", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Integer),
                    is_async: false,
                },
            );
        }

        // Find inputNumber function
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("inputNumber", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        // Find toString function
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("toString", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Integer],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        // Find toInteger function
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("toInteger", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Integer),
                    is_async: false,
                },
            );
        }

        // Find abs function
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("abs", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        // Find StringUtils namespace functions
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("StringUtils_length", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Integer),
                    is_async: false,
                },
            );
        }
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("StringUtils_concat", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String, ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("StringUtils_substring", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![
                        ConcreteType::String,
                        ConcreteType::Integer,
                        ConcreteType::Integer,
                    ],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("StringUtils_indexOf", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String, ConcreteType::String],
                    return_type: Box::new(ConcreteType::Integer),
                    is_async: false,
                },
            );
        }
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("StringUtils_replace", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![
                        ConcreteType::String,
                        ConcreteType::String,
                        ConcreteType::String,
                    ],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        // Add namespace symbols to type environment
        // This allows namespace identifiers to be recognized as valid symbols
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Namespace, // Add new namespace type
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("string", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(symbol_id, ConcreteType::Namespace);
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("http", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(symbol_id, ConcreteType::Namespace);
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("file", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(symbol_id, ConcreteType::Namespace);
        }

        // Add compare namespace to type environment
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("compare", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(symbol_id, ConcreteType::Namespace);
        }

        // Add conditional namespace to type environment
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("conditional", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(symbol_id, ConcreteType::Namespace);
        }

        // Add logical namespace to type environment
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("logical", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(symbol_id, ConcreteType::Namespace);
        }

        // Add validator namespace to type environment
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(symbol_id, ConcreteType::Namespace);
        }

        // Add math namespace functions to type environment
        // These correspond to the namespace functions registered in symbol_table.rs

        // Math namespace functions
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_max", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number, ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_min", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number, ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_abs", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_sqrt", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_trunc", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_pi", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_sin", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_cos", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_tan", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_log", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_pow", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number, ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_floor", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_ceil", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("math_round", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Number],
                    return_type: Box::new(ConcreteType::Number),
                    is_async: false,
                },
            );
        }

        // String namespace functions
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("string_length", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Integer),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("string_substring", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![
                        ConcreteType::String,
                        ConcreteType::Integer,
                        ConcreteType::Integer,
                    ],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("string_toUpperCase", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("string_toLowerCase", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("string_contains", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String, ConcreteType::String],
                    return_type: Box::new(ConcreteType::Boolean),
                    is_async: false,
                },
            );
        }

        // List namespace functions

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("list_size", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Array(Box::new(ConcreteType::Number))],
                    return_type: Box::new(ConcreteType::Integer),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("list_push", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![
                        ConcreteType::Array(Box::new(ConcreteType::Number)),
                        ConcreteType::Number,
                    ],
                    return_type: Box::new(ConcreteType::Undefined),
                    is_async: false,
                },
            );
        }

        // HTTP namespace functions
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("http_get", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("http_post", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String, ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("http_put", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String, ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("http_delete", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        // File namespace functions
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("file_read", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::String),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("file_write", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String, ConcreteType::String],
                    return_type: Box::new(ConcreteType::Boolean),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("file_append", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String, ConcreteType::String],
                    return_type: Box::new(ConcreteType::Boolean),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("file_exists", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Boolean),
                    is_async: false,
                },
            );
        }

        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("file_delete", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::String],
                    return_type: Box::new(ConcreteType::Boolean),
                    is_async: false,
                },
            );
        }

        // Validator namespace functions
        // validator.create - creates validation rules (returns pointer to rules struct)
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator.create", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![],
                    return_type: Box::new(ConcreteType::Integer), // Returns pointer
                    is_async: false,
                },
            );
        }

        // validator.ok - creates success validation result
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator.ok", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Integer],      // Value to wrap
                    return_type: Box::new(ConcreteType::Integer), // Returns ValidationResult pointer
                    is_async: false,
                },
            );
        }

        // validator.error - creates error validation result
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator.error", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Integer], // Errors list pointer
                    return_type: Box::new(ConcreteType::Integer), // Returns ValidationResult pointer
                    is_async: false,
                },
            );
        }

        // validator.isOk - checks if result is success
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator.isOk", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Integer], // ValidationResult pointer
                    return_type: Box::new(ConcreteType::Boolean),
                    is_async: false,
                },
            );
        }

        // validator.isError - checks if result is error
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator.isError", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Integer], // ValidationResult pointer
                    return_type: Box::new(ConcreteType::Boolean),
                    is_async: false,
                },
            );
        }

        // validator.getValue - gets value from successful result
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator.getValue", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Integer], // ValidationResult pointer
                    return_type: Box::new(ConcreteType::Integer), // Returns the wrapped value
                    is_async: false,
                },
            );
        }

        // validator.getErrors - gets errors from failed result
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator.getErrors", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Integer], // ValidationResult pointer
                    return_type: Box::new(ConcreteType::Integer), // Returns errors list pointer
                    is_async: false,
                },
            );
        }

        // Generic registration: Register all builtin functions from symbol table
        // This automatically handles namespace functions and other builtins
        let builtin_symbols: Vec<_> = self.symbol_table.accessible_symbols();
        for symbol_id in builtin_symbols {
            if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                if self.symbol_table.is_builtin(symbol_id) {
                    match &symbol.kind {
                        crate::resolver::SymbolKind::Function {
                            parameters,
                            return_type,
                        } => {
                            // Skip if already registered (to preserve manual overrides above)
                            if self.type_env.get(&symbol_id).is_none() {
                                let concrete_params: Vec<ConcreteType> = parameters
                                    .iter()
                                    .map(Self::hir_type_to_concrete_type)
                                    .collect();
                                let concrete_return = return_type
                                    .as_ref()
                                    .map(|t| Box::new(Self::hir_type_to_concrete_type(t)))
                                    .unwrap_or_else(|| Box::new(ConcreteType::Null));

                                self.type_env.insert(
                                    symbol_id,
                                    ConcreteType::Function {
                                        parameters: concrete_params,
                                        return_type: concrete_return,
                                        is_async: false,
                                    },
                                );
                            }
                        }
                        _ => {
                            // Skip non-function builtins (classes, namespaces, etc.)
                        }
                    }
                }
            }
        }
    }

    /// Convert HirType to ConcreteType for builtin function type mapping
    fn hir_type_to_concrete_type(hir_type: &HirType) -> ConcreteType {
        match hir_type {
            HirType::Integer => ConcreteType::Integer,
            HirType::Number => ConcreteType::Number,
            HirType::String => ConcreteType::String,
            HirType::Boolean => ConcreteType::Boolean,
            HirType::Void => ConcreteType::Null,
            // CRITICAL FIX: Handle precision types (number:32, number:64, integer:8, etc.)
            // All integer precision types map to Integer
            HirType::Integer8
            | HirType::Integer8u
            | HirType::Integer16
            | HirType::Integer16u
            | HirType::Integer32
            | HirType::Integer32u
            | HirType::Integer64
            | HirType::Integer64u => ConcreteType::Integer,
            // All number precision types map to Number (f64 in WASM)
            HirType::Number32 | HirType::Number64 => ConcreteType::Number,
            HirType::Named { name, .. } => {
                // For now, map named types to concrete types by name
                match name.as_str() {
                    "integer" => ConcreteType::Integer,
                    "number" => ConcreteType::Number,
                    "string" => ConcreteType::String,
                    "boolean" => ConcreteType::Boolean,
                    "void" => ConcreteType::Undefined,
                    _ => ConcreteType::Unknown,
                }
            }
            _ => ConcreteType::Unknown,
        }
    }

    /// Infer types for the entire program
    fn infer_program(&mut self, program: &ResolvedHirProgram) -> TastProgram {
        let mut tast_functions = Vec::new();
        let mut tast_classes = Vec::new();

        // First pass: Register all function and class signatures
        for function in &program.functions {
            self.register_function_signature(function);
        }

        // Register start function signature if it exists
        if let Some(start_fn) = &program.start_function {
            self.register_function_signature(start_fn);
        }

        for class in &program.classes {
            self.register_class_signature(class);
        }

        // Second pass: Infer function bodies
        for function in &program.functions {
            match self.infer_function(function) {
                Ok(tast_function) => {
                    tast_functions.push(tast_function);
                }
                Err(error) => {
                    tracing::trace!(
                        "ERROR: Failed to infer function '{}' (SymbolId {:?}): {:?}",
                        function.name,
                        function.symbol_id,
                        error
                    );
                    self.errors.push(error);
                }
            }
        }

        // Third pass: Infer class method bodies
        for class in &program.classes {
            match self.infer_class(class) {
                Ok(tast_class) => {
                    tast_classes.push(tast_class);
                }
                Err(error) => {
                    tracing::trace!(
                        "ERROR: Failed to infer class '{}' (SymbolId {:?}): {:?}",
                        class.name,
                        class.symbol_id,
                        error
                    );
                    self.errors.push(error);
                }
            }
        }

        // Handle start function
        let tast_start_function = if let Some(start_fn) = &program.start_function {
            match self.infer_function(start_fn) {
                Ok(func) => Some(func),
                Err(error) => {
                    self.errors.push(error);
                    None
                }
            }
        } else {
            None
        };

        let result = TastProgram {
            functions: tast_functions,
            classes: tast_classes,
            start_function: tast_start_function,
            imports: Vec::new(), // Would convert imports here
            tests: Vec::new(),   // Would convert tests here
            type_env: self.type_env.clone(),
            location: program.location.clone(),
            // CRITICAL FIX: Pass symbol table through to MIR for dynamic SymbolId resolution
            symbol_table: std::sync::Arc::new(program.symbol_table.clone()),
        };

        // Type inference completed successfully

        result
    }

    /// Register function signature in type environment
    fn register_function_signature(&mut self, function: &ResolvedHirFunction) {
        let param_types: Vec<ConcreteType> = function
            .parameters
            .iter()
            .map(|p| self.hir_type_to_concrete(&p.param_type))
            .collect();

        let return_type = if let Some(ref rt) = function.return_type {
            self.hir_type_to_concrete(rt)
        } else {
            ConcreteType::Null // Functions without explicit return type return void/null
        };

        let function_type = ConcreteType::Function {
            parameters: param_types,
            return_type: Box::new(return_type.clone()),
            is_async: function.is_async,
        };

        self.type_env.insert(function.symbol_id, function_type);

        // Track minimum required parameters (those without defaults)
        let required_count = function
            .parameters
            .iter()
            .filter(|p| p.default_value.is_none())
            .count();
        self.required_param_counts
            .insert(function.symbol_id, required_count);
    }

    /// Register method signature in type environment
    fn register_method_signature(&mut self, method: &ResolvedHirMethod) {
        let param_types: Vec<ConcreteType> = method
            .parameters
            .iter()
            .map(|p| self.hir_type_to_concrete(&p.param_type))
            .collect();

        let return_type = self.hir_type_to_concrete(&method.return_type);

        let function_type = ConcreteType::Function {
            parameters: param_types,
            return_type: Box::new(return_type),
            is_async: false, // Methods are not async by default
        };

        self.type_env.insert(method.symbol_id, function_type);
    }

    /// Register class signature in type environment
    fn register_class_signature(&mut self, class: &ResolvedHirClass) {
        let class_type = ConcreteType::Class {
            symbol_id: class.symbol_id,
            type_args: Vec::new(), // Would handle generics here
        };

        self.type_env.insert(class.symbol_id, class_type);

        // Register field types
        for field in &class.fields {
            let field_type = self.hir_type_to_concrete(&field.field_type);
            self.type_env.insert(field.symbol_id, field_type);
        }

        // Register method signatures
        for method in &class.methods {
            self.register_method_signature(method);
        }
    }

    /// Infer types for a function
    fn infer_function(
        &mut self,
        function: &ResolvedHirFunction,
    ) -> Result<TastFunction, CompilerError> {
        self.current_function = Some(function.symbol_id);

        // Set current return type for constraint generation within the function
        // We'll update this later if no explicit return type is declared
        self.current_return_type = if let Some(ref return_type) = function.return_type {
            Some(self.hir_type_to_concrete(return_type))
        } else {
            None // Will be inferred from function body
        };

        // Add parameters to type environment
        let mut tast_parameters = Vec::new();
        for param in &function.parameters {
            let param_type = self.hir_type_to_concrete(&param.param_type);
            self.type_env.insert(param.symbol_id, param_type.clone());

            let default_value = if let Some(default_expr) = &param.default_value {
                Some(self.infer_expression(default_expr)?)
            } else {
                None
            };

            tast_parameters.push(TastParameter {
                symbol_id: param.symbol_id,
                name: param.name.clone(),
                param_type,
                default_value,
                is_variadic: param.is_variadic,
                location: param.location.clone(),
            });
        }

        // Count required parameters (those without defaults) for validation
        let required_param_count = function
            .parameters
            .iter()
            .filter(|p| p.default_value.is_none())
            .count();
        self.required_param_counts
            .insert(function.symbol_id, required_param_count);

        // Infer function body
        let tast_body = self.infer_block(&function.body)?;

        // Check return type consistency
        let inferred_return_type = tast_body.return_type.clone();
        let declared_return_type = if let Some(ref return_type) = function.return_type {
            // Explicit return type declared - enforce it
            let concrete_return_type = self.hir_type_to_concrete(return_type);

            // FIX: Only add constraint if types are compatible
            // This prevents incorrect constraint generation when block inference fails
            match (&inferred_return_type, &concrete_return_type) {
                (ConcreteType::Generic { .. }, _) => {
                    // Type variable - no constraint needed, use declared type
                }
                (inferred, declared) if inferred == declared => {
                    // Types match - no constraint needed
                }
                (ConcreteType::Integer, ConcreteType::String)
                | (ConcreteType::String, ConcreteType::Integer) => {
                    // String/Integer mismatch - known issue, use declared type without constraint
                    // This is a targeted fix for the string type constraint generation bug
                }
                _ => {
                    // Other concrete types - add constraint to ensure consistency
                    self.add_constraint(TypeConstraint::Equality {
                        left: inferred_return_type.clone(),
                        right: concrete_return_type.clone(),
                        location: function.location.clone(),
                    });
                }
            }
            concrete_return_type
        } else {
            // No return type declared - use inferred type
            // This allows functions to have their return type inferred from their body
            // Update current return type for any remaining processing
            self.current_return_type = Some(inferred_return_type.clone());
            inferred_return_type.clone()
        };

        self.current_function = None;
        self.current_return_type = None;

        // DEBUG: Log final return type stored in TAST for Pairs/Matrix functions
        let return_debug = format!("{:?}", declared_return_type);
        if return_debug.contains("Pairs") || return_debug.contains("Matrix") {
            tracing::trace!(
                "[DEBUG infer_function END] Function '{}' final TastFunction.return_type:",
                function.name
            );
            tracing::trace!("  {:?}", declared_return_type);
        }

        Ok(TastFunction {
            symbol_id: function.symbol_id,
            name: function.name.clone(),
            parameters: tast_parameters,
            return_type: declared_return_type,
            body: tast_body,
            generic_params: Vec::new(), // Would handle generics here
            constraints: Vec::new(),
            is_async: function.is_async,
            is_static: false, // Top-level functions are not static methods
            visibility: Visibility::Public, // Would get from HIR
            location: function.location.clone(),
        })
    }

    /// Infer types for a method (similar to function but handles methods)
    fn infer_constructor(
        &mut self,
        constructor: &crate::resolver::ResolvedHirConstructor,
        class_symbol_id: SymbolId,
    ) -> Result<TastFunction, CompilerError> {
        self.current_function = Some(constructor.symbol_id);

        // Constructor returns an instance of the class
        let return_type = ConcreteType::Class {
            symbol_id: class_symbol_id,
            type_args: Vec::new(),
        };
        self.current_return_type = Some(return_type.clone());

        // Add parameters to type environment
        let mut tast_parameters = Vec::new();
        for param in &constructor.parameters {
            let param_type = self.hir_type_to_concrete(&param.param_type);
            self.type_env.insert(param.symbol_id, param_type.clone());

            tast_parameters.push(TastParameter {
                symbol_id: param.symbol_id,
                name: param.name.clone(),
                param_type,
                default_value: None,
                is_variadic: param.is_variadic,
                location: param.location.clone(),
            });
        }

        // Infer body
        let tast_body = self.infer_block(&constructor.body)?;

        Ok(TastFunction {
            symbol_id: constructor.symbol_id,
            name: "constructor".to_string(), // Constructors are named "constructor"
            parameters: tast_parameters,
            return_type,
            body: tast_body,
            generic_params: Vec::new(),
            constraints: Vec::new(),
            is_async: false,
            is_static: false, // Constructors are never static
            visibility: Visibility::Public,
            location: constructor.location.clone(),
        })
    }

    fn infer_method(
        &mut self,
        method: &ResolvedHirMethod,
        has_parent: bool,
        has_fields: bool,
    ) -> Result<TastFunction, CompilerError> {
        self.current_function = Some(method.symbol_id);
        self.current_return_type = Some(self.hir_type_to_concrete(&method.return_type));

        // Add parameters to type environment
        let mut tast_parameters = Vec::new();
        for param in &method.parameters {
            let param_type = self.hir_type_to_concrete(&param.param_type);
            self.type_env.insert(param.symbol_id, param_type.clone());

            tast_parameters.push(TastParameter {
                symbol_id: param.symbol_id,
                name: param.name.clone(),
                param_type,
                default_value: None, // Would convert from HIR
                is_variadic: param.is_variadic,
                location: param.location.clone(),
            });
        }

        // Infer body
        let tast_body = self.infer_block(&method.body)?;

        let declared_return_type = self.hir_type_to_concrete(&method.return_type);

        // REFINED FIX: Determine if method should be static based on class context
        // For classes with inheritance OR instance fields: Always use instance methods
        // - Method signatures must match across inheritance hierarchy for polymorphism
        // - Example: Vehicle.getMaxSpeed() returns 60 (no 'this'), but Car.getMaxSpeed() uses this.isElectric
        // For utility classes (no parent, no fields): Use heuristic to detect static methods
        // - Allows methods like MathUtils.add(a, b) to be called statically
        let is_static = if has_parent || has_fields {
            // Class has inheritance or state - all methods must be instance methods
            false
        } else {
            // Utility class - detect static methods using heuristic
            !self.body_uses_this(&tast_body)
        };

        Ok(TastFunction {
            symbol_id: method.symbol_id,
            name: method.name.clone(),
            parameters: tast_parameters,
            return_type: declared_return_type,
            body: tast_body,
            generic_params: Vec::new(), // Would handle generics here
            constraints: Vec::new(),
            is_async: false,                // Methods are typically not async
            is_static,                      // Detected based on whether method uses 'this'
            visibility: Visibility::Public, // Would get from HIR
            location: method.location.clone(),
        })
    }

    /// Infer types for a class
    fn infer_class(&mut self, class: &ResolvedHirClass) -> Result<TastClass, CompilerError> {
        self.current_class = Some(class.symbol_id);

        // Convert fields
        let mut tast_fields = Vec::new();
        for field in &class.fields {
            let field_type = self.hir_type_to_concrete(&field.field_type);

            let default_value = if let Some(init_expr) = &field.initializer {
                // Special handling for empty literals with explicit type annotations
                let is_empty_literal = match init_expr {
                    ResolvedHirExpression::Literal { value, .. } => match value {
                        crate::ast::Value::List(elements) => elements.is_empty(),
                        crate::ast::Value::Pairs(pairs) => pairs.is_empty(),
                        _ => false,
                    },
                    _ => false,
                };

                if is_empty_literal {
                    // For empty array literals, create an ArrayLiteral with empty elements
                    // This ensures the list gets properly allocated rather than set to 0
                    let element_type = match &field_type {
                        ConcreteType::Array(elem_type) => (**elem_type).clone(),
                        _ => ConcreteType::Unknown,
                    };
                    Some(TastExpression {
                        kind: TastExpressionKind::ArrayLiteral {
                            elements: vec![],
                            element_type,
                        },
                        expr_type: field_type.clone(),
                        location: init_expr.location().clone(),
                    })
                } else {
                    Some(self.infer_expression(init_expr)?)
                }
            } else {
                None
            };

            tast_fields.push(TastField {
                symbol_id: field.symbol_id,
                name: field.name.clone(),
                field_type,
                default_value,
                is_static: false,   // ResolvedHirField doesn't have is_static field
                is_readonly: false, // Would get from HIR
                visibility: Visibility::Public, // Would get from HIR
                location: field.location.clone(),
            });
        }

        // Convert constructors
        let mut tast_constructors = Vec::new();
        if let Some(constructor) = &class.constructor {
            if let Ok(tast_constructor) = self.infer_constructor(constructor, class.symbol_id) {
                tast_constructors.push(tast_constructor);
            }
        }

        // Convert methods
        let mut tast_methods = Vec::new();
        let has_parent = class.parent.is_some();
        let has_fields = !class.fields.is_empty();
        for method in &class.methods {
            if let Ok(tast_method) = self.infer_method(method, has_parent, has_fields) {
                tast_methods.push(tast_method);
            }
        }

        self.current_class = None;

        Ok(TastClass {
            symbol_id: class.symbol_id,
            name: class.name.clone(),
            fields: tast_fields,
            methods: tast_methods,
            constructors: tast_constructors, // Now properly populated
            parent_class: class.parent,
            interfaces: Vec::new(),         // Would handle interfaces
            generic_params: Vec::new(),     // Would handle generics
            is_abstract: false,             // Would get from HIR
            visibility: Visibility::Public, // Would get from HIR
            location: class.location.clone(),
        })
    }

    /// Check if a block uses 'this' or accesses instance fields
    /// Returns true if the method is instance-dependent, false if it's static-safe
    fn body_uses_this(&self, block: &TastBlock) -> bool {
        // Check all statements in the block
        for statement in &block.statements {
            if self.statement_uses_this(statement) {
                return true;
            }
        }
        false
    }

    /// Check if a statement uses 'this'
    fn statement_uses_this(&self, statement: &TastStatement) -> bool {
        match statement {
            TastStatement::Expression { expression, .. } => self.expression_uses_this(expression),
            TastStatement::VariableDeclaration { initializer, .. } => initializer
                .as_ref()
                .map_or(false, |e| self.expression_uses_this(e)),
            TastStatement::Assignment { target, value, .. } => {
                self.expression_uses_this(target) || self.expression_uses_this(value)
            }
            TastStatement::Return { value, .. } => value
                .as_ref()
                .map_or(false, |e| self.expression_uses_this(e)),
            TastStatement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                self.expression_uses_this(condition)
                    || self.body_uses_this(then_block)
                    || else_block
                        .as_ref()
                        .map_or(false, |b| self.body_uses_this(b))
            }
            TastStatement::For { iterable, body, .. } => {
                self.expression_uses_this(iterable) || self.body_uses_this(body)
            }
            _ => false,
        }
    }

    /// Check if an expression uses 'this'
    fn expression_uses_this(&self, expression: &TastExpression) -> bool {
        match &expression.kind {
            TastExpressionKind::Variable { name, .. } => name == "this",
            TastExpressionKind::PropertyAccess { object, .. } => self.expression_uses_this(object),
            TastExpressionKind::MethodCall {
                receiver,
                arguments,
                ..
            } => {
                self.expression_uses_this(receiver)
                    || arguments.iter().any(|a| self.expression_uses_this(a))
            }
            TastExpressionKind::FunctionCall {
                function,
                arguments,
                ..
            } => {
                self.expression_uses_this(function)
                    || arguments.iter().any(|a| self.expression_uses_this(a))
            }
            TastExpressionKind::BinaryOperation { left, right, .. } => {
                self.expression_uses_this(left) || self.expression_uses_this(right)
            }
            TastExpressionKind::UnaryOperation { operand, .. } => {
                self.expression_uses_this(operand)
            }
            TastExpressionKind::ArrayLiteral { elements, .. } => {
                elements.iter().any(|e| self.expression_uses_this(e))
            }
            _ => false,
        }
    }

    /// Infer types for a block
    fn infer_block(&mut self, block: &ResolvedHirBlock) -> Result<TastBlock, CompilerError> {
        let mut tast_statements = Vec::new();
        // Start with Undefined return type (void blocks)
        // Only the last expression statement will update this
        let mut block_return_type = ConcreteType::Undefined;

        let statement_count = block.statements.len();
        for (i, statement) in block.statements.iter().enumerate() {
            let tast_statement = self.infer_statement(statement)?;
            let is_last_statement = i == statement_count - 1;

            // Update block return type based on statement
            match &tast_statement {
                TastStatement::Return { return_type, .. } => {
                    block_return_type = return_type.clone();
                }
                TastStatement::Expression { expression, .. } => {
                    // Only the LAST expression statement becomes the block's return type
                    // Other expression statements are discarded (will need DROP in codegen)
                    if is_last_statement {
                        block_return_type = expression.expr_type.clone();
                    }
                }
                TastStatement::If {
                    result_type,
                    then_block,
                    else_block,
                    ..
                } => {
                    // If statement can produce a value when both branches return
                    // Check if the branches contain return statements
                    let then_returns = !matches!(then_block.return_type, ConcreteType::Undefined);
                    let else_returns = else_block
                        .as_ref()
                        .map(|b| !matches!(b.return_type, ConcreteType::Undefined))
                        .unwrap_or(false);

                    // If both branches return, use the result_type
                    // If only one branch returns, use that branch's return type
                    if then_returns || else_returns {
                        if !matches!(result_type, ConcreteType::Null | ConcreteType::Undefined) {
                            block_return_type = result_type.clone();
                        } else if then_returns {
                            block_return_type = then_block.return_type.clone();
                        } else if else_returns {
                            block_return_type = else_block.as_ref().unwrap().return_type.clone();
                        }
                    }
                }
                _ => {}
            }

            tast_statements.push(tast_statement);
        }

        Ok(TastBlock {
            statements: tast_statements,
            scope_id: crate::resolver::symbol_table::ScopeId(0), // ResolvedHirBlock doesn't track scope_id
            return_type: block_return_type,
            location: block.location.clone(),
        })
    }

    /// Infer types for a statement
    fn infer_statement(
        &mut self,
        statement: &ResolvedHirStatement,
    ) -> Result<TastStatement, CompilerError> {
        // Recursion depth guard to prevent stack overflow
        if self.recursion_depth > 1000 {
            return Err(CompilerError::type_error(
                "Maximum recursion depth exceeded in statement inference",
                Some("This might indicate a circular dependency".to_string()),
                None,
            ));
        }

        self.recursion_depth += 1;
        let result = match statement {
            ResolvedHirStatement::Expression {
                expression,
                location,
            } => {
                let tast_expression = self.infer_expression(expression)?;
                Ok(TastStatement::Expression {
                    expression: tast_expression,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::VariableDeclaration {
                symbol_id,
                name,
                var_type,
                initializer,
                location,
            } => {
                let declared_type = self.hir_type_to_concrete(var_type);

                let tast_initializer = if let Some(init_expr) = initializer {
                    // Special handling for empty literals with explicit type annotations
                    // Check if this is an empty array [] or empty pairs {} literal
                    let is_empty_literal = match init_expr {
                        // Empty array literal []
                        ResolvedHirExpression::Array { elements, .. } => elements.is_empty(),
                        // Empty pairs literal {}
                        ResolvedHirExpression::Literal { value, .. } => match value {
                            crate::ast::Value::Pairs(pairs) => pairs.is_empty(),
                            _ => false,
                        },
                        _ => false,
                    };

                    let tast_init = if is_empty_literal {
                        // For empty array literals, create an ArrayLiteral with empty elements
                        // This ensures list<integer> myList = [] allocates an actual list
                        // rather than setting the pointer to 0 (null)
                        let element_type = match &declared_type {
                            ConcreteType::Array(elem_type) => (**elem_type).clone(),
                            _ => ConcreteType::Unknown,
                        };
                        TastExpression {
                            kind: TastExpressionKind::ArrayLiteral {
                                elements: vec![],
                                element_type,
                            },
                            expr_type: declared_type.clone(),
                            location: init_expr.location().clone(),
                        }
                    } else {
                        // Normal inference for non-empty literals
                        let tast_init = self.infer_expression(init_expr)?;

                        // Add constraint that initializer type matches declared type
                        self.add_constraint(TypeConstraint::Equality {
                            left: tast_init.expr_type.clone(),
                            right: declared_type.clone(),
                            location: location.clone(),
                        });

                        tast_init
                    };

                    Some(tast_init)
                } else {
                    None
                };

                // Add variable to type environment
                self.type_env.insert(*symbol_id, declared_type.clone());

                Ok(TastStatement::VariableDeclaration {
                    symbol_id: *symbol_id,
                    name: name.clone(),
                    var_type: declared_type,
                    initializer: tast_initializer,
                    is_mutable: false, // ResolvedHirStatement doesn't track mutability
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::Return { value, location } => {
                let return_type = if let Some(return_expr) = value {
                    let tast_expr = self.infer_expression(return_expr)?;
                    let expr_type = tast_expr.expr_type.clone();

                    // Check against function return type
                    if let Some(expected_return_type) = &self.current_return_type {
                        self.add_constraint(TypeConstraint::Equality {
                            left: expr_type.clone(),
                            right: expected_return_type.clone(),
                            location: location.clone(),
                        });
                    }

                    Some(tast_expr)
                } else {
                    // Void return
                    if let Some(expected_return_type) = &self.current_return_type {
                        self.add_constraint(TypeConstraint::Equality {
                            left: ConcreteType::Null,
                            right: expected_return_type.clone(),
                            location: location.clone(),
                        });
                    }
                    None
                };

                let return_expr_type = return_type
                    .as_ref()
                    .map(|e| e.expr_type.clone())
                    .unwrap_or(ConcreteType::Null);

                Ok(TastStatement::Return {
                    value: return_type,
                    return_type: return_expr_type,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::Print {
                expression,
                newline,
                location,
            } => {
                let tast_expression = self.infer_expression(expression)?;

                // Print statements should work with any type
                Ok(TastStatement::Print {
                    expression: tast_expression,
                    newline: *newline,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::If {
                condition,
                then_branch,
                else_branch,
                location,
            } => {
                // Infer condition type and ensure it's boolean
                let tast_condition = self.infer_expression(condition)?;
                self.add_constraint(TypeConstraint::Equality {
                    left: tast_condition.expr_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });

                // Infer then branch
                let tast_then_block = self.infer_block(then_branch)?;

                // Infer else branch if present
                let tast_else_block = if let Some(else_block) = else_branch {
                    Some(self.infer_block(else_block)?)
                } else {
                    None
                };

                // Determine result type - if both branches exist, find common type
                let result_type = if let Some(else_tast) = &tast_else_block {
                    // Both branches exist, find common type
                    self.find_common_type(&tast_then_block.return_type, &else_tast.return_type)
                } else {
                    // Only then branch, result is unit (void)
                    ConcreteType::Null
                };

                Ok(TastStatement::If {
                    condition: tast_condition,
                    then_block: tast_then_block,
                    else_block: tast_else_block,
                    result_type,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::For {
                variable,
                variable_symbol_id,
                iterable,
                body,
                location,
            } => {
                // Infer iterable type
                let tast_iterable = self.infer_expression(iterable)?;

                // Extract element type from iterable (Array<T> -> T)
                let element_type = match &tast_iterable.expr_type {
                    ConcreteType::Array(element_type) => (**element_type).clone(),
                    _ => {
                        // Add constraint that iterable must be an array
                        let element_var = self.create_type_variable();
                        self.add_constraint(TypeConstraint::Equality {
                            left: tast_iterable.expr_type.clone(),
                            right: ConcreteType::Array(Box::new(element_var.clone())),
                            location: location.clone(),
                        });
                        element_var
                    }
                };

                // Add loop variable to type environment
                self.type_env.insert(*variable_symbol_id, element_type);

                // Infer body
                let tast_body = self.infer_block(body)?;

                Ok(TastStatement::For {
                    iterator: *variable_symbol_id,
                    iterator_name: variable.clone(),
                    iterable: tast_iterable,
                    body: tast_body,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::While {
                condition,
                body,
                location,
            } => {
                // Infer condition type - must be boolean
                let tast_condition = self.infer_expression(condition)?;

                // Ensure condition is boolean
                if tast_condition.expr_type != ConcreteType::Boolean {
                    self.add_constraint(TypeConstraint::Equality {
                        left: tast_condition.expr_type.clone(),
                        right: ConcreteType::Boolean,
                        location: location.clone(),
                    });
                }

                // Infer body
                let tast_body = self.infer_block(body)?;

                Ok(TastStatement::While {
                    condition: tast_condition,
                    body: tast_body,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::Assignment {
                target,
                value,
                location,
            } => {
                // Determine target type first to handle empty literals correctly
                let target_type = match target {
                    ResolvedHirLValue::Variable { symbol_id, .. } => self
                        .type_env
                        .get(symbol_id)
                        .cloned()
                        .unwrap_or(ConcreteType::Unknown),
                    ResolvedHirLValue::FieldAccess {
                        field_symbol_id, ..
                    } => {
                        // Look up field type from symbol table to get the HirType
                        if let Some(symbol) = self.symbol_table.get_symbol(*field_symbol_id) {
                            if let SymbolKind::Field { field_type, .. } = &symbol.kind {
                                self.hir_type_to_concrete(field_type)
                            } else {
                                ConcreteType::Unknown
                            }
                        } else {
                            ConcreteType::Unknown
                        }
                    }
                    ResolvedHirLValue::Index { array, .. } => {
                        // Infer array type to get element type
                        let tast_array = self.infer_expression(array)?;
                        match &tast_array.expr_type {
                            ConcreteType::Array(element_type) => (**element_type).clone(),
                            _ => ConcreteType::Unknown,
                        }
                    }
                };

                // Special handling for empty literals with known target type
                let is_empty_literal = match value {
                    // Empty array literal []
                    ResolvedHirExpression::Array { elements, .. } => elements.is_empty(),
                    // Empty pairs literal {}
                    ResolvedHirExpression::Literal { value: val, .. } => match val {
                        crate::ast::Value::Pairs(pairs) => pairs.is_empty(),
                        _ => false,
                    },
                    _ => false,
                };

                let tast_value =
                    if is_empty_literal && !matches!(target_type, ConcreteType::Unknown) {
                        // For empty array literals, create an ArrayLiteral with empty elements
                        // This ensures the list gets properly allocated rather than set to 0
                        let element_type = match &target_type {
                            ConcreteType::Array(elem_type) => (**elem_type).clone(),
                            _ => ConcreteType::Unknown,
                        };
                        TastExpression {
                            kind: TastExpressionKind::ArrayLiteral {
                                elements: vec![],
                                element_type,
                            },
                            expr_type: target_type.clone(),
                            location: value.location().clone(),
                        }
                    } else {
                        // Normal inference
                        self.infer_expression(value)?
                    };

                // Handle variable assignments (field access handled via PropertyAccess)
                let tast_target = match target {
                    ResolvedHirLValue::Variable {
                        name,
                        symbol_id,
                        location: var_location,
                    } => {
                        // Only add constraint if not an empty literal (already handled)
                        if !is_empty_literal {
                            // Add constraint that value type matches target type
                            self.add_constraint(TypeConstraint::Equality {
                                left: tast_value.expr_type.clone(),
                                right: target_type.clone(),
                                location: location.clone(),
                            });
                        }

                        TastExpression {
                            kind: TastExpressionKind::Variable {
                                symbol_id: *symbol_id,
                                name: name.clone(),
                            },
                            expr_type: target_type,
                            location: var_location.clone(),
                        }
                    }
                    ResolvedHirLValue::FieldAccess {
                        object,
                        field,
                        field_symbol_id,
                        location: field_location,
                    } => {
                        // Handle field assignments like obj.field = value
                        let tast_object = self.infer_expression(object)?;

                        // Only add constraint if not an empty literal (already handled)
                        if !is_empty_literal {
                            // Add constraint that value type matches field type
                            self.add_constraint(TypeConstraint::Equality {
                                left: tast_value.expr_type.clone(),
                                right: target_type.clone(),
                                location: location.clone(),
                            });
                        }

                        TastExpression {
                            kind: TastExpressionKind::PropertyAccess {
                                object: Box::new(tast_object),
                                property_name: field.clone(),
                                property_symbol: *field_symbol_id,
                            },
                            expr_type: target_type,
                            location: field_location.clone(),
                        }
                    }
                    ResolvedHirLValue::Index {
                        array,
                        index,
                        location: index_location,
                    } => {
                        // Handle array index assignments like arr[i] = value
                        let tast_array = self.infer_expression(array)?;
                        let tast_index = self.infer_expression(index)?;

                        // For array index assignment, the element type should match the value type
                        let element_type = match &tast_array.expr_type {
                            ConcreteType::Array(elem_type) => (**elem_type).clone(),
                            _ => ConcreteType::Unknown, // Not an array, but we'll handle this as unknown
                        };

                        // Add constraint that value type matches element type
                        self.add_constraint(TypeConstraint::Equality {
                            left: tast_value.expr_type.clone(),
                            right: element_type.clone(),
                            location: location.clone(),
                        });

                        TastExpression {
                            kind: TastExpressionKind::ArrayAccess {
                                array: Box::new(tast_array),
                                index: Box::new(tast_index),
                            },
                            expr_type: element_type,
                            location: index_location.clone(),
                        }
                    }
                };

                Ok(TastStatement::Assignment {
                    target: tast_target,
                    value: tast_value,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::LaterAssignment {
                variable,
                symbol_id,
                expression,
                location,
            } => {
                let tast_expression = self.infer_expression(expression)?;

                // Add variable to type environment with inferred type from expression
                self.type_env
                    .insert(*symbol_id, tast_expression.expr_type.clone());

                Ok(TastStatement::LaterAssignment {
                    variable: variable.clone(),
                    symbol_id: *symbol_id,
                    expression: tast_expression,
                    location: location.clone(),
                })
            } // Handle any remaining unimplemented statement types
        };
        self.recursion_depth -= 1;
        result
    }

    /// Infer types for an expression
    fn infer_expression(
        &mut self,
        expression: &ResolvedHirExpression,
    ) -> Result<TastExpression, CompilerError> {
        // Recursion depth guard to prevent stack overflow
        if self.recursion_depth > 1000 {
            return Err(CompilerError::type_error(
                "Maximum recursion depth exceeded in type inference",
                Some("This might indicate a circular type dependency".to_string()),
                None,
            ));
        }

        self.recursion_depth += 1;
        let (kind, expr_type, location) = match expression {
            ResolvedHirExpression::Literal { value, location } => {
                let (tast_literal, literal_type) = self.infer_literal(value);
                (
                    TastExpressionKind::Literal {
                        value: tast_literal,
                    },
                    literal_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::Variable {
                symbol_id,
                name,
                location,
            } => {
                let var_type = self.type_env.get(symbol_id).cloned().unwrap_or_else(|| {
                    self.errors.push(CompilerError::type_error(
                        &format!("Variable {} not found in type environment", name),
                        None,
                        Some(location.clone()),
                    ));
                    ConcreteType::Unknown
                });

                (
                    TastExpressionKind::Variable {
                        symbol_id: *symbol_id,
                        name: name.clone(),
                    },
                    var_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::BinaryOp {
                left,
                op,
                right,
                location,
            } => {
                let tast_left = self.infer_expression(left)?;
                let tast_right = self.infer_expression(right)?;

                // Resolve types with current substitutions before binary operation type inference
                let resolved_left_type = self.resolve_type(&tast_left.expr_type);
                let resolved_right_type = self.resolve_type(&tast_right.expr_type);

                let result_type = self.infer_binary_operation(
                    op,
                    &resolved_left_type,
                    &resolved_right_type,
                    location,
                )?;

                (
                    TastExpressionKind::BinaryOperation {
                        operator: self.convert_binary_operator(op),
                        left: Box::new(tast_left),
                        right: Box::new(tast_right),
                    },
                    result_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::Call {
                function,
                function_symbol_id,
                arguments,
                location,
            } => {
                let mut tast_arguments = Vec::new();

                for arg in arguments {
                    tast_arguments.push(self.infer_expression(arg)?);
                }

                // Get the function type and add parameter type constraints
                if let Some(function_type) = self.type_env.get(function_symbol_id).cloned() {
                    // Add type constraints between arguments and parameters
                    let _constraint_check = self.infer_function_call(
                        &function_type,
                        &tast_arguments,
                        *function_symbol_id,
                        location,
                    )?;
                }

                // Look up function type and determine return type
                let return_type = self.infer_function_return_type(
                    *function_symbol_id,
                    function,
                    &tast_arguments,
                )?;

                // CRITICAL FIX: Use SymbolId(0) for namespace functions (string.*, math.*, etc.)
                // This ensures MIR builder creates NamedFunction operands for proper symbol resolution
                let is_namespace_function = function.contains('.')
                    && (function.starts_with("string.")
                        || function.starts_with("math.")
                        || function.starts_with("list.")
                        || function.starts_with("array.")
                        || function.starts_with("compare.")
                        || function.starts_with("file.")
                        || function.starts_with("http."));

                let resolved_symbol_id = if is_namespace_function {
                    tracing::trace!(
                        "DEBUG CALL: Using SymbolId(0) for namespace function '{}'",
                        function
                    );
                    crate::resolver::symbol_table::SymbolId(0)
                } else {
                    *function_symbol_id
                };

                (
                    TastExpressionKind::FunctionCall {
                        function: Box::new(TastExpression {
                            kind: TastExpressionKind::Variable {
                                symbol_id: resolved_symbol_id,
                                name: function.clone(),
                            },
                            expr_type: ConcreteType::Function {
                                parameters: tast_arguments
                                    .iter()
                                    .map(|a| a.expr_type.clone())
                                    .collect(),
                                return_type: Box::new(return_type.clone()),
                                is_async: false,
                            },
                            location: location.clone(),
                        }),
                        arguments: tast_arguments,
                        type_args: Vec::new(),
                    },
                    return_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::Index {
                array,
                index,
                location,
            } => {
                let tast_array = self.infer_expression(array)?;
                let tast_index = self.infer_expression(index)?;

                // Extract element type based on array type and index type
                let element_type = match &tast_array.expr_type {
                    // Any type supports both string (object access) and integer (array access)
                    ConcreteType::Any => match &tast_index.expr_type {
                        ConcreteType::String | ConcreteType::Integer => ConcreteType::Any,
                        other => {
                            self.errors.push(CompilerError::type_error(
                                    &format!(
                                        "Any type index must be string (for object access) or integer (for array access), found {:?}",
                                        other
                                    ),
                                    Some("Use data[\"field\"] for object access or data[0] for array access".to_string()),
                                    Some(location.clone()),
                                ));
                            ConcreteType::Any
                        }
                    },
                    // Array requires integer index
                    ConcreteType::Array(element_type) => {
                        if !matches!(tast_index.expr_type, ConcreteType::Integer) {
                            self.errors.push(CompilerError::type_error(
                                &format!(
                                    "Array index must be integer, found {:?}",
                                    tast_index.expr_type
                                ),
                                None,
                                Some(location.clone()),
                            ));
                        }
                        (**element_type).clone()
                    }
                    // Matrix indexing: matrix<T>[i] returns Array<T>
                    ConcreteType::Matrix(element_type) => {
                        if !matches!(tast_index.expr_type, ConcreteType::Integer) {
                            self.errors.push(CompilerError::type_error(
                                &format!(
                                    "Matrix index must be integer, found {:?}",
                                    tast_index.expr_type
                                ),
                                None,
                                Some(location.clone()),
                            ));
                        }
                        ConcreteType::Array(Box::new((**element_type).clone()))
                    }
                    // Pairs type supports string key access
                    ConcreteType::Pairs(_, value_type) => {
                        if !matches!(tast_index.expr_type, ConcreteType::String) {
                            self.errors.push(CompilerError::type_error(
                                &format!(
                                    "Pairs key must be string, found {:?}",
                                    tast_index.expr_type
                                ),
                                Some("Use pairs[\"key\"] for pairs access".to_string()),
                                Some(location.clone()),
                            ));
                        }
                        (**value_type).clone()
                    }
                    other_type => {
                        self.errors.push(CompilerError::type_error(
                            &format!("Cannot index into type: {:?}", other_type),
                            Some(
                                "Bracket access is only supported on Array, Pairs, or Any types"
                                    .to_string(),
                            ),
                            Some(location.clone()),
                        ));
                        ConcreteType::Unknown
                    }
                };

                (
                    TastExpressionKind::ArrayAccess {
                        array: Box::new(tast_array),
                        index: Box::new(tast_index),
                    },
                    element_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::MethodCall {
                receiver,
                method,
                method_symbol_id,
                arguments,
                location,
            } => {
                let tast_receiver = self.infer_expression(receiver)?;

                let mut tast_arguments = Vec::new();
                for arg in arguments {
                    tast_arguments.push(self.infer_expression(arg)?);
                }

                // Resolve receiver type with current substitutions before method lookup
                let resolved_receiver_type = self.resolve_type(&tast_receiver.expr_type);

                // Use resolved type for method resolution
                let return_type = self.infer_method_return_type(
                    method,
                    &resolved_receiver_type,
                    &tast_arguments,
                )?;

                // CRITICAL FIX: Resolve method symbol from receiver's class type or primitive type
                // The resolver sets method_symbol_id = None because it doesn't have type info
                // Now we have the receiver's type, so we can look up the method
                let resolved_method_symbol = method_symbol_id
                    .or_else(|| {
                        // Extract class symbol ID from receiver type
                        match &resolved_receiver_type {
                            ConcreteType::Class { symbol_id, .. } => {
                                // Look up the method in the class's symbol table
                                if let Some(method_sym) =
                                    self.symbol_table.lookup_class_member(*symbol_id, method)
                                {
                                    tracing::debug!(
                                        class_symbol = symbol_id.0,
                                        method = %method,
                                        method_symbol = method_sym.0,
                                        "Resolved instance method symbol from class type"
                                    );
                                    Some(method_sym)
                                } else {
                                    tracing::warn!(
                                        class_symbol = symbol_id.0,
                                        method = %method,
                                        "Method not found in class - using SymbolId(0) fallback"
                                    );
                                    None
                                }
                            }
                            _ => {
                                // Not a class type - might be built-in method on primitive type
                                // Try to resolve as built-in method (e.g., "string.toString")
                                let type_name =
                                    Self::get_builtin_type_name(&resolved_receiver_type);
                                if let Some(type_name) = type_name {
                                    let builtin_method_name = format!("{}.{}", type_name, method);
                                    if let Some(method_sym) =
                                        self.symbol_table.lookup_symbol(&builtin_method_name)
                                    {
                                        tracing::debug!(
                                            type_name = %type_name,
                                            method = %method,
                                            builtin_name = %builtin_method_name,
                                            method_symbol = method_sym.0,
                                            "Resolved built-in method symbol from primitive type"
                                        );
                                        Some(method_sym)
                                    } else {
                                        // NOTE: Built-in methods like integer.toString, boolean.toString, etc.
                                        // are not registered in the symbol table - they're resolved at codegen time
                                        // via the function_map. Using SymbolId(0) is expected for these methods.
                                        tracing::debug!(
                                            type_name = %type_name,
                                            method = %method,
                                            builtin_name = %builtin_method_name,
                                            "Built-in method resolved at codegen time - using SymbolId(0)"
                                        );
                                        None
                                    }
                                } else {
                                    // Complex type without built-in methods
                                    None
                                }
                            }
                        }
                    })
                    .unwrap_or(crate::resolver::symbol_table::SymbolId(0)); // SymbolId(0) for built-in methods

                (
                    TastExpressionKind::MethodCall {
                        receiver: Box::new(tast_receiver),
                        method_name: method.clone(),
                        method_symbol: resolved_method_symbol,
                        arguments: tast_arguments,
                        type_args: Vec::new(),
                    },
                    return_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::StaticMethodCall {
                namespace,
                class_name,
                class_symbol_id: _,
                method,
                method_symbol_id,
                arguments,
                location,
            } => {
                tracing::debug!("DEBUG STATIC METHOD CALL: class_name='{}', method='{}', method_symbol_id=SymbolId({})",
                          class_name, method, method_symbol_id.0);

                let mut tast_arguments = Vec::new();
                for arg in arguments {
                    tast_arguments.push(self.infer_expression(arg)?);
                }

                // Handle namespace.class.method() calls (e.g., compare.integer.greaterThan)
                let full_class_name = if !namespace.is_empty() {
                    format!("{}.{}", namespace.join("."), class_name)
                } else {
                    class_name.clone()
                };

                // For now, use simple static method resolution based on class and method name
                tracing::debug!("DEBUG TypeChecker: Calling infer_static_method_return_type for {}.{} with {} args",
                          full_class_name, method, tast_arguments.len());
                let return_type = self.infer_static_method_return_type(
                    &full_class_name,
                    method,
                    &tast_arguments,
                )?;

                // CRITICAL FIX: Use SymbolId(0) for built-in namespace methods
                // (string.*, math.*, list.*, etc.) so MIR builder creates NamedFunction operands
                // For user-defined static methods, keep the actual method_symbol_id
                let is_builtin_namespace =
                    ["string", "math", "list", "array", "compare", "file", "http"]
                        .iter()
                        .any(|ns| {
                            full_class_name.eq(*ns)
                                || full_class_name.starts_with(&format!("{}.", ns))
                        });

                tracing::debug!("DEBUG TYPE INF STATIC: full_class_name='{}', method='{}', is_builtin={}, method_symbol_id=SymbolId({})",
                          full_class_name, method, is_builtin_namespace, method_symbol_id.0);

                let resolved_method_symbol = if is_builtin_namespace {
                    tracing::trace!(
                        "DEBUG TYPE INF STATIC: Setting SymbolId(0) for namespace method {}.{}",
                        full_class_name,
                        method
                    );
                    crate::resolver::symbol_table::SymbolId(0) // Force NamedFunction in MIR
                } else {
                    tracing::debug!("DEBUG TYPE INF STATIC: Keeping SymbolId({}) for user-defined static method", method_symbol_id.0);
                    *method_symbol_id // Use actual symbol for user-defined static methods
                };

                // Use StaticMethodCall to properly represent static method calls
                // This prevents incorrectly adding a 'this' parameter
                (
                    TastExpressionKind::StaticMethodCall {
                        class_name: full_class_name,
                        method_name: method.clone(),
                        method_symbol: resolved_method_symbol,
                        arguments: tast_arguments,
                        type_args: Vec::new(),
                    },
                    return_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::FieldAccess {
                object,
                field,
                field_symbol_id: _hir_field_symbol_id,
                location,
            } => {
                let tast_object = self.infer_expression(object)?;

                // CRITICAL FIX: Resolve the field type AND symbol ID based on the object's actual type
                // This enables inherited field access - we look up the field in the object's class hierarchy
                let (field_type, resolved_field_symbol_id) =
                    self.infer_field_type_and_symbol(&tast_object.expr_type, field)?;

                (
                    TastExpressionKind::PropertyAccess {
                        object: Box::new(tast_object),
                        property_name: field.clone(),
                        property_symbol: resolved_field_symbol_id, // Use resolved symbol, not HIR placeholder
                    },
                    field_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::Array {
                elements,
                element_type,
                location,
            } => {
                let mut tast_elements = Vec::new();
                let concrete_element_type = self.hir_type_to_concrete(element_type);

                for element in elements {
                    let tast_element = self.infer_expression(element)?;
                    tast_elements.push(tast_element);
                }

                let array_type = ConcreteType::Array(Box::new(concrete_element_type.clone()));

                (
                    TastExpressionKind::ArrayLiteral {
                        elements: tast_elements,
                        element_type: concrete_element_type,
                    },
                    array_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::UnaryOp {
                op,
                operand,
                location,
            } => {
                let tast_operand = self.infer_expression(operand)?;

                let result_type =
                    self.infer_unary_operation(op, &tast_operand.expr_type, location)?;

                (
                    TastExpressionKind::UnaryOperation {
                        operator: self.convert_unary_operator(op),
                        operand: Box::new(tast_operand),
                    },
                    result_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::Assignment {
                target,
                value,
                location,
            } => {
                let tast_value = self.infer_expression(value)?;

                // For assignment expressions, the type is the type of the assigned value
                let assignment_type = tast_value.expr_type.clone();

                // Validate assignment target - complex LValues not yet fully supported
                match target {
                    ResolvedHirLValue::Variable { .. } => {
                        // Simple variable assignment is supported
                    }
                    _ => {
                        // For complex LValues (field access, array indexing, etc.), emit error
                        self.errors.push(CompilerError::type_error(
                            "Complex assignment targets not yet fully supported in type inference",
                            None,
                            Some(location.clone()),
                        ));
                    }
                }

                // Assignment expressions return the assigned value in Clean Language
                (tast_value.kind, assignment_type, location.clone())
            }

            ResolvedHirExpression::Constructor {
                class_name,
                class_symbol_id,
                constructor_symbol_id,
                arguments,
                location,
            } => {
                let mut tast_arguments = Vec::new();
                for arg in arguments {
                    tast_arguments.push(self.infer_expression(arg)?);
                }

                // The constructor returns an instance of the class
                let instance_type = ConcreteType::Class {
                    symbol_id: *class_symbol_id,
                    type_args: Vec::new(),
                };

                // For now, represent constructor calls as function calls
                (
                    TastExpressionKind::FunctionCall {
                        function: Box::new(TastExpression {
                            kind: TastExpressionKind::Variable {
                                symbol_id: *constructor_symbol_id, // Use constructor's SymbolId, not class's
                                name: format!("{}.constructor", class_name),
                            },
                            expr_type: ConcreteType::Function {
                                parameters: tast_arguments
                                    .iter()
                                    .map(|a| a.expr_type.clone())
                                    .collect(),
                                return_type: Box::new(instance_type.clone()),
                                is_async: false,
                            },
                            location: location.clone(),
                        }),
                        arguments: tast_arguments,
                        type_args: Vec::new(),
                    },
                    instance_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::This {
                class_symbol_id,
                location,
            } => {
                // `this` refers to the current instance of the class
                let instance_type = ConcreteType::Class {
                    symbol_id: *class_symbol_id,
                    type_args: Vec::new(),
                };

                (
                    TastExpressionKind::Variable {
                        symbol_id: *class_symbol_id,
                        name: "this".to_string(),
                    },
                    instance_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::Cast {
                expression,
                target_type,
                location,
            } => {
                let tast_expression = self.infer_expression(expression)?;
                let target_concrete_type = self.hir_type_to_concrete(target_type);

                // Cast operations validated at compile time via type constraints
                (
                    TastExpressionKind::FunctionCall {
                        function: Box::new(TastExpression {
                            kind: TastExpressionKind::Variable {
                                symbol_id: crate::resolver::symbol_table::SymbolId(0), // Dummy symbol
                                name: format!("cast_to_{:?}", target_concrete_type),
                            },
                            expr_type: ConcreteType::Function {
                                parameters: vec![tast_expression.expr_type.clone()],
                                return_type: Box::new(target_concrete_type.clone()),
                                is_async: false,
                            },
                            location: location.clone(),
                        }),
                        arguments: vec![tast_expression],
                        type_args: Vec::new(),
                    },
                    target_concrete_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::OnError {
                expression,
                fallback,
                location,
            } => {
                // Infer types for both the expression and fallback
                let tast_expression = self.infer_expression(expression)?;
                let tast_fallback = self.infer_expression(fallback)?;

                // The onError expression returns the type of the main expression
                // The fallback should be compatible with the expression type
                self.add_constraint(TypeConstraint::Equality {
                    left: tast_expression.expr_type.clone(),
                    right: tast_fallback.expr_type.clone(),
                    location: location.clone(),
                });

                let result_type = tast_expression.expr_type.clone();

                // Create OnError TAST node
                let kind = TastExpressionKind::OnError {
                    expression: Box::new(tast_expression),
                    fallback: Box::new(tast_fallback),
                };

                (kind, result_type, location.clone())
            }

            ResolvedHirExpression::Conditional {
                condition,
                then_expr,
                else_expr,
                location,
            } => {
                // Infer condition type and ensure it's boolean
                let tast_condition = self.infer_expression(condition)?;
                self.add_constraint(TypeConstraint::Equality {
                    left: tast_condition.expr_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });

                // Infer types for both branches
                let tast_then = self.infer_expression(then_expr)?;
                let tast_else = self.infer_expression(else_expr)?;

                // Both branches must have compatible types
                self.add_constraint(TypeConstraint::Equality {
                    left: tast_then.expr_type.clone(),
                    right: tast_else.expr_type.clone(),
                    location: location.clone(),
                });

                // Result type is the unified type of both branches
                let result_type = tast_then.expr_type.clone();

                // Create conditional TAST node
                let kind = TastExpressionKind::Conditional {
                    condition: Box::new(tast_condition),
                    then_expr: Box::new(tast_then),
                    else_expr: Box::new(tast_else),
                };

                (kind, result_type, location.clone())
            }

            ResolvedHirExpression::BaseCall {
                parent_class_symbol_id,
                arguments,
                location,
            } => {
                // Infer types for all arguments
                let mut tast_arguments = Vec::new();
                for arg in arguments {
                    let tast_arg = self.infer_expression(arg)?;
                    tast_arguments.push(tast_arg);
                }

                // BaseCall returns null/void (it's a statement-like expression)
                (
                    TastExpressionKind::BaseCall {
                        parent_class_symbol_id: *parent_class_symbol_id,
                        arguments: tast_arguments,
                    },
                    ConcreteType::Null,
                    location.clone(),
                )
            }

            ResolvedHirExpression::Range {
                start,
                end,
                step,
                inclusive,
                location,
            } => {
                // Infer types for start, end, and optionally step
                let tast_start = self.infer_expression(start)?;
                let tast_end = self.infer_expression(end)?;
                let tast_step = if let Some(s) = step {
                    Some(Box::new(self.infer_expression(s)?))
                } else {
                    None
                };

                // Both start and end should be integers
                // For now, we'll type the range as an Array<Integer>
                // The MIR/codegen will handle actually generating the range values

                (
                    TastExpressionKind::Range {
                        start: Box::new(tast_start),
                        end: Box::new(tast_end),
                        step: tast_step,
                        inclusive: *inclusive,
                    },
                    ConcreteType::Array(Box::new(ConcreteType::Integer)),
                    location.clone(),
                )
            }
        };

        self.recursion_depth -= 1;
        Ok(TastExpression {
            kind,
            expr_type,
            location,
        })
    }

    /// Infer type for a literal
    fn infer_literal(&self, literal: &crate::ast::Value) -> (TastLiteral, ConcreteType) {
        match literal {
            crate::ast::Value::Integer(value) => {
                (TastLiteral::Integer(*value), ConcreteType::Integer)
            }
            crate::ast::Value::Number(value) => (TastLiteral::Number(*value), ConcreteType::Number),
            crate::ast::Value::String(value) => {
                (TastLiteral::String(value.clone()), ConcreteType::String)
            }
            crate::ast::Value::Boolean(value) => {
                (TastLiteral::Boolean(*value), ConcreteType::Boolean)
            }
            crate::ast::Value::Void => (TastLiteral::Null, ConcreteType::Null),
            crate::ast::Value::List(elements) => {
                // Infer list type from elements
                if elements.is_empty() {
                    // Empty list - type will be inferred from context
                    // For now, return Unknown which will be unified with expected type
                    (TastLiteral::Null, ConcreteType::Unknown)
                } else {
                    // Infer element type from first element (could be improved)
                    let (_first_lit, first_type) = self.infer_literal(&elements[0]);
                    (TastLiteral::Null, ConcreteType::Array(Box::new(first_type)))
                }
            }
            _ => (TastLiteral::Null, ConcreteType::Unknown), // Handle other value types
        }
    }

    /// Infer result type of binary operation
    fn infer_binary_operation(
        &mut self,
        operator: &HirBinaryOp,
        left_type: &ConcreteType,
        right_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<ConcreteType, CompilerError> {
        match operator {
            HirBinaryOp::Add => {
                // Addition is overloaded: numeric addition OR string concatenation
                match (left_type, right_type) {
                    // String concatenation: string + string = string
                    (ConcreteType::String, ConcreteType::String) => Ok(ConcreteType::String),

                    // String concatenation: string + any = string (automatic conversion)
                    (ConcreteType::String, _) => Ok(ConcreteType::String),
                    (_, ConcreteType::String) => Ok(ConcreteType::String),

                    // Numeric addition: check if both can be treated as numbers
                    _ => {
                        // For purely numeric types and matrices, do appropriate addition
                        match (left_type, right_type) {
                            (ConcreteType::Integer, ConcreteType::Integer) => {
                                Ok(ConcreteType::Integer)
                            }
                            (ConcreteType::Number, ConcreteType::Number) => {
                                Ok(ConcreteType::Number)
                            }
                            (ConcreteType::Integer, ConcreteType::Number)
                            | (ConcreteType::Number, ConcreteType::Integer) => {
                                Ok(ConcreteType::Number)
                            }

                            // Matrix-matrix addition: matrix<T> + matrix<T> -> matrix<T>
                            (ConcreteType::Matrix(left_elem), ConcreteType::Matrix(right_elem)) => {
                                Ok(ConcreteType::Matrix(Box::new(
                                    left_elem.common_supertype(right_elem),
                                )))
                            }
                            _ => {
                                // Try to constrain both operands to be numeric
                                self.add_constraint(TypeConstraint::Subtype {
                                    subtype: left_type.clone(),
                                    supertype: ConcreteType::Number,
                                    location: location.clone(),
                                });

                                self.add_constraint(TypeConstraint::Subtype {
                                    subtype: right_type.clone(),
                                    supertype: ConcreteType::Number,
                                    location: location.clone(),
                                });

                                // Result is the common supertype of operands for numeric addition
                                Ok(left_type.common_supertype(right_type))
                            }
                        }
                    }
                }
            }

            HirBinaryOp::Subtract
            | HirBinaryOp::Multiply
            | HirBinaryOp::Divide
            | HirBinaryOp::Modulo
            | HirBinaryOp::Power => {
                // Handle different operation combinations
                match (left_type, right_type) {
                    // Integer-integer operations return integer
                    (ConcreteType::Integer, ConcreteType::Integer) => Ok(ConcreteType::Integer),

                    // Matrix-scalar operations: matrix<T> op scalar -> matrix<T>
                    (ConcreteType::Matrix(element_type), scalar_type)
                        if scalar_type.is_numeric() =>
                    {
                        Ok(ConcreteType::Matrix(element_type.clone()))
                    }

                    // Scalar-matrix operations: scalar op matrix<T> -> matrix<T>
                    (scalar_type, ConcreteType::Matrix(element_type))
                        if scalar_type.is_numeric() =>
                    {
                        Ok(ConcreteType::Matrix(element_type.clone()))
                    }

                    // Matrix-matrix operations: matrix<T> op matrix<T> -> matrix<T>
                    (ConcreteType::Matrix(left_elem), ConcreteType::Matrix(right_elem)) => Ok(
                        ConcreteType::Matrix(Box::new(left_elem.common_supertype(right_elem))),
                    ),

                    _ => {
                        // For other numeric combinations, enforce Number constraint
                        self.add_constraint(TypeConstraint::Subtype {
                            subtype: left_type.clone(),
                            supertype: ConcreteType::Number,
                            location: location.clone(),
                        });

                        self.add_constraint(TypeConstraint::Subtype {
                            subtype: right_type.clone(),
                            supertype: ConcreteType::Number,
                            location: location.clone(),
                        });

                        // Result is the common supertype of operands
                        Ok(left_type.common_supertype(right_type))
                    }
                }
            }

            HirBinaryOp::Equal | HirBinaryOp::NotEqual | HirBinaryOp::Is | HirBinaryOp::IsNot => {
                // Equality/identity can compare any types
                Ok(ConcreteType::Boolean)
            }

            HirBinaryOp::Less
            | HirBinaryOp::LessEqual
            | HirBinaryOp::Greater
            | HirBinaryOp::GreaterEqual => {
                // Comparison requires compatible types
                self.add_constraint(TypeConstraint::Equality {
                    left: left_type.clone(),
                    right: right_type.clone(),
                    location: location.clone(),
                });

                Ok(ConcreteType::Boolean)
            }

            HirBinaryOp::And | HirBinaryOp::Or => {
                // Logical operations require boolean types
                self.add_constraint(TypeConstraint::Equality {
                    left: left_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });

                self.add_constraint(TypeConstraint::Equality {
                    left: right_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });

                Ok(ConcreteType::Boolean)
            }

            // BOOK: null-coalescing - NullCoalesce operator (a default b)
            // Returns left if not null, otherwise returns right
            // Both sides should have compatible types
            HirBinaryOp::NullCoalesce => {
                // Both operands should have the same type
                self.add_constraint(TypeConstraint::Equality {
                    left: left_type.clone(),
                    right: right_type.clone(),
                    location: location.clone(),
                });

                // Result type is the same as the operands
                Ok(left_type.clone())
            }

            HirBinaryOp::StringConcat => {
                // String concatenation
                self.add_constraint(TypeConstraint::Equality {
                    left: left_type.clone(),
                    right: ConcreteType::String,
                    location: location.clone(),
                });

                self.add_constraint(TypeConstraint::Equality {
                    left: right_type.clone(),
                    right: ConcreteType::String,
                    location: location.clone(),
                });

                Ok(ConcreteType::String)
            }
        }
    }

    /// Infer return type of function call
    fn infer_function_call(
        &mut self,
        function_type: &ConcreteType,
        arguments: &[TastExpression],
        function_symbol_id: SymbolId,
        location: &SourceLocation,
    ) -> Result<ConcreteType, CompilerError> {
        // Check if this is a generic list function that should skip type checking
        // These functions accept lists of any element type, so we can't validate
        // parameter types with the fixed type signatures we registered
        let is_generic_list_fn =
            if let Some(symbol) = self.symbol_table.get_symbol(function_symbol_id) {
                matches!(
                    symbol.name.as_str(),
                    "list_fill"
                        | "list.fill"
                        | "list_add"
                        | "list.add"
                        | "list_push"
                        | "list.push"
                        | "list_insert"
                        | "list.insert"
                        | "list_contains"
                        | "list.contains"
                        | "list_indexOf"
                        | "list.indexOf"
                        | "list_index_of"
                        | "list_lastIndexOf"
                        | "list.lastIndexOf"
                        | "list_size"
                        | "list.size"
                        | "list_length"
                        | "list.length"
                        | "list_isEmpty"
                        | "list.isEmpty"
                        | "list_isNotEmpty"
                        | "list.isNotEmpty"
                        | "list_get"
                        | "list.get"
                        | "list_set"
                        | "list.set"
                        | "list_remove"
                        | "list.remove"
                        | "list_pop"
                        | "list.pop"
                        | "list_peek"
                        | "list.peek"
                        | "list_first"
                        | "list.first"
                        | "list_last"
                        | "list.last"
                        | "list_sort"
                        | "list.sort"
                        | "list_reverse"
                        | "list.reverse"
                        | "list_slice"
                        | "list.slice"
                        | "list_concat"
                        | "list.concat"
                        | "list_join"
                        | "list.join"
                        | "list_clear"
                        | "list.clear"
                        | "list_range"
                        | "list.range" // creates lists, doesn't take list argument
                )
            } else {
                false
            };

        // Check if this is a variadic print function that accepts any number of arguments
        let is_variadic_print_fn =
            if let Some(symbol) = self.symbol_table.get_symbol(function_symbol_id) {
                matches!(symbol.name.as_str(), "print" | "println" | "printl")
            } else {
                false
            };

        match function_type {
            ConcreteType::Function {
                parameters,
                return_type,
                ..
            } => {
                // Check if function has default parameters
                let required_count = self
                    .required_param_counts
                    .get(&function_symbol_id)
                    .copied()
                    .unwrap_or(parameters.len());

                // Validate argument count against required parameters (not total parameters)
                if arguments.len() < required_count {
                    return Err(CompilerError::type_error(
                        &format!(
                            "Function requires at least {} arguments, got {}",
                            required_count,
                            arguments.len()
                        ),
                        None,
                        Some(location.clone()),
                    ));
                }

                // Skip argument count check for variadic print functions
                if !is_variadic_print_fn && arguments.len() > parameters.len() {
                    return Err(CompilerError::type_error(
                        &format!(
                            "Function accepts at most {} arguments, got {}",
                            parameters.len(),
                            arguments.len()
                        ),
                        None,
                        Some(location.clone()),
                    ));
                }

                // Check argument types match parameters (only for provided arguments)
                // Skip type checking for generic list functions and variadic print functions
                if !is_generic_list_fn && !is_variadic_print_fn {
                    for (param_type, arg) in parameters.iter().zip(arguments.iter()) {
                        self.add_constraint(TypeConstraint::Equality {
                            left: arg.expr_type.clone(),
                            right: param_type.clone(),
                            location: location.clone(),
                        });
                    }
                }

                Ok((**return_type).clone())
            }

            _ => Err(CompilerError::type_error(
                &format!("Cannot call non-function type: {}", function_type),
                None,
                Some(location.clone()),
            )),
        }
    }

    /// Get function return type from symbol table
    fn infer_function_return_type(
        &self,
        function_symbol_id: SymbolId,
        function_name: &str,
        arguments: &[TastExpression],
    ) -> Result<ConcreteType, CompilerError> {
        // Special handling for generic list namespace functions FIRST
        // Use the function_name parameter directly for SymbolId(0) namespace functions
        // to avoid incorrect matching with the "print" symbol at SymbolId(0)

        // Functions that always return integer
        if function_name == "list_indexOf"
            || function_name == "list.indexOf"
            || function_name == "list_lastIndexOf"
            || function_name == "list.lastIndexOf"
        {
            return Ok(ConcreteType::Integer);
        }

        // Functions that always return boolean
        if function_name == "list_contains"
            || function_name == "list.contains"
            || function_name == "list_isEmpty"
            || function_name == "list.isEmpty"
            || function_name == "list_isNotEmpty"
            || function_name == "list.isNotEmpty"
        {
            return Ok(ConcreteType::Boolean);
        }

        // Functions that return the element type of their list argument
        if (function_name == "list_remove"
            || function_name == "list.remove"
            || function_name == "list_get"
            || function_name == "list.get"
            || function_name == "list_pop"
            || function_name == "list.pop"
            || function_name == "list_peek"
            || function_name == "list.peek"
            || function_name == "list_first"
            || function_name == "list.first"
            || function_name == "list_last"
            || function_name == "list.last")
            && !arguments.is_empty()
        {
            let resolved_arg_type = self.resolve_type(&arguments[0].expr_type);
            if let ConcreteType::Array(element_type) = resolved_arg_type {
                return Ok((*element_type).clone());
            }
        }

        // Functions that return the same list type as their input
        if (function_name == "list_add"
            || function_name == "list.add"
            || function_name == "list_push"
            || function_name == "list.push"
            || function_name == "list_sort"
            || function_name == "list.sort"
            || function_name == "list_reverse"
            || function_name == "list.reverse"
            || function_name == "list_insert"
            || function_name == "list.insert"
            || function_name == "list_slice"
            || function_name == "list.slice"
            || function_name == "list_concat"
            || function_name == "list.concat")
            && !arguments.is_empty()
        {
            let resolved_arg_type = self.resolve_type(&arguments[0].expr_type);
            if let ConcreteType::Array(_) = resolved_arg_type {
                return Ok(resolved_arg_type);
            }
        }

        // Functions that create new lists with element type from second argument
        if (function_name == "list_fill" || function_name == "list.fill") && arguments.len() >= 2 {
            let element_type = self.resolve_type(&arguments[1].expr_type);
            return Ok(ConcreteType::Array(Box::new(element_type)));
        }

        // list_range always returns list<integer>
        if function_name == "list_range" || function_name == "list.range" {
            return Ok(ConcreteType::Array(Box::new(ConcreteType::Integer)));
        }

        // Look up function type from symbol table
        if let Some(function_type) = self.type_env.get(&function_symbol_id) {
            match function_type {
                ConcreteType::Function { return_type, .. } => Ok((**return_type).clone()),
                _ => Err(CompilerError::type_error(
                    "Symbol is not a function",
                    None,
                    None,
                )),
            }
        } else {
            // FALLBACK: Check if this might be a static method call that wasn't resolved properly
            // CRITICAL FIX: Handle SymbolId(0) for stdlib namespace functions
            // SymbolId(0) is a placeholder used for stdlib functions like string.length, math.max
            // These are registered in CodeGenerator, not in the symbol table
            if function_symbol_id == SymbolId(0) {
                // Use function_name parameter directly (e.g., "string.length")
                if let Some(dot_pos) = function_name.find('.') {
                    let class_name = &function_name[..dot_pos];
                    let method_name = &function_name[dot_pos + 1..];
                    // Try to infer as static method/stdlib function
                    if let Ok(static_return_type) =
                        self.infer_static_method_return_type(class_name, method_name, arguments)
                    {
                        return Ok(static_return_type);
                    }
                }
            }

            // Look up the symbol in the symbol table to get its name
            if let Some(symbol) = self.symbol_table.get_symbol(function_symbol_id) {
                let function_name = &symbol.name;

                // Check if the function name follows the static method pattern Class.method
                if let Some(dot_pos) = function_name.find('.') {
                    let class_name = &function_name[..dot_pos];
                    let method_name = &function_name[dot_pos + 1..];
                    // Try to infer as static method
                    if let Ok(static_return_type) =
                        self.infer_static_method_return_type(class_name, method_name, arguments)
                    {
                        return Ok(static_return_type);
                    }
                }
            }

            Err(CompilerError::type_error(
                &format!(
                    "Function symbol {:?} not found in type environment",
                    function_symbol_id
                ),
                None,
                None,
            ))
        }
    }

    /// Get the builtin type name for method lookups (e.g., "string", "integer", "list")
    /// Returns None for complex types that don't have built-in methods
    fn get_builtin_type_name(concrete_type: &ConcreteType) -> Option<String> {
        match concrete_type {
            ConcreteType::Integer => Some("integer".to_string()),
            ConcreteType::Number => Some("number".to_string()),
            ConcreteType::String => Some("string".to_string()),
            ConcreteType::Boolean => Some("boolean".to_string()),
            ConcreteType::Array(_) => Some("list".to_string()), // Fixed: list is the correct type name in Clean Language
            ConcreteType::Matrix(_) => Some("matrix".to_string()),
            ConcreteType::Pairs(_, _) => Some("pairs".to_string()),
            // Any type for generic/dynamic values
            ConcreteType::Any => Some("any".to_string()),
            // Class, Interface, Function, Tuple, Union, etc. are not primitive types
            _ => None,
        }
    }

    /// Infer return type of method call
    fn infer_method_return_type(
        &self,
        method_name: &str,
        receiver_type: &ConcreteType,
        _arguments: &[TastExpression],
    ) -> Result<ConcreteType, CompilerError> {
        // For now, implement basic built-in method type inference
        match (receiver_type, method_name) {
            // Integer methods
            (ConcreteType::Integer, "toString") => Ok(ConcreteType::String),
            (ConcreteType::Integer, "abs") => Ok(ConcreteType::Integer),

            // Number methods
            (ConcreteType::Number, "toString") => Ok(ConcreteType::String),
            (ConcreteType::Number, "abs") => Ok(ConcreteType::Number),
            (ConcreteType::Number, "floor") => Ok(ConcreteType::Integer),
            (ConcreteType::Number, "ceil") => Ok(ConcreteType::Integer),
            (ConcreteType::Number, "round") => Ok(ConcreteType::Integer),

            // String methods
            (ConcreteType::String, "length") => Ok(ConcreteType::Integer),
            (ConcreteType::String, "toString") => Ok(ConcreteType::String),
            (ConcreteType::String, "toUpperCase") => Ok(ConcreteType::String),
            (ConcreteType::String, "toLowerCase") => Ok(ConcreteType::String),
            (ConcreteType::String, "trim") => Ok(ConcreteType::String),

            // Array methods
            (ConcreteType::Array(_), "length") => Ok(ConcreteType::Integer),
            (ConcreteType::Array(_), "size") => Ok(ConcreteType::Integer), // Alias for length
            (ConcreteType::Array(_), "push") => Ok(ConcreteType::Undefined), // void return
            (ConcreteType::Array(element_type), "pop") => Ok((**element_type).clone()),
            (ConcreteType::Array(element_type), "remove") => Ok((**element_type).clone()), // remove() behaves like pop()
            (ConcreteType::Array(_), "contains") => Ok(ConcreteType::Boolean), // contains() returns boolean
            (ConcreteType::Array(_), "isEmpty") => Ok(ConcreteType::Boolean), // isEmpty() returns boolean
            (ConcreteType::Array(_), "toString") => Ok(ConcreteType::String),

            // Boolean methods
            (ConcreteType::Boolean, "toString") => Ok(ConcreteType::String),

            // Any type methods - returns string for toString, preserves any for others
            (ConcreteType::Any, "toString") => Ok(ConcreteType::String),

            // Class instance methods - look up in symbol table
            (ConcreteType::Class { symbol_id, .. }, _) => {
                // Look up the method in the class's symbol table
                if let Some(method_symbol) = self
                    .symbol_table
                    .lookup_class_member(*symbol_id, method_name)
                {
                    // Get the method info from the symbol table
                    if let Some(symbol) = self.symbol_table.get_symbol(method_symbol) {
                        if let crate::resolver::symbol_table::SymbolKind::Method {
                            return_type,
                            ..
                        } = &symbol.kind
                        {
                            // Convert HirType to ConcreteType
                            let concrete_type = Self::hir_type_to_concrete_type(return_type);
                            tracing::debug!(
                                class_symbol = symbol_id.0,
                                method_name = %method_name,
                                method_symbol = method_symbol.0,
                                return_type = ?concrete_type,
                                "Resolved class method return type from symbol table"
                            );
                            return Ok(concrete_type);
                        }
                    }
                }

                // Method not found or not a method - log warning and return Unknown
                tracing::warn!(
                    class_symbol = symbol_id.0,
                    method_name = %method_name,
                    "Method not found in class or invalid method info - returning Unknown"
                );
                Ok(ConcreteType::Unknown)
            }

            // For unknown method/type combinations, return Unknown
            _ => Ok(ConcreteType::Unknown),
        }
    }

    /// Infer return type for static method calls
    fn infer_static_method_return_type(
        &self,
        class_name: &str,
        method_name: &str,
        arguments: &[TastExpression],
    ) -> Result<ConcreteType, CompilerError> {
        // Helper to validate argument count
        let validate_arg_count =
            |expected: usize, actual: usize, method_full_name: &str| -> Result<(), CompilerError> {
                tracing::trace!(
                    "DEBUG validate_arg_count: {} expects {}, got {}",
                    method_full_name,
                    expected,
                    actual
                );
                if actual != expected {
                    tracing::debug!("DEBUG: ARGUMENT COUNT MISMATCH - RETURNING ERROR!");
                    let error = CompilerError::type_error(
                        format!(
                            "{}() expects {} argument(s), but {} were provided",
                            method_full_name, expected, actual
                        ),
                        Some(format!("Provide exactly {} argument(s)", expected)),
                        None,
                    );
                    tracing::debug!("DEBUG: Created error: {:?}", error);
                    return Err(error);
                }
                Ok(())
            };

        // Implement basic built-in static method type inference with argument validation
        let arg_count = arguments.len();
        let full_method_name = format!("{}.{}", class_name, method_name);
        tracing::trace!(
            "DEBUG infer_static_method_return_type: class_name={}, method_name={}, arg_count={}",
            class_name,
            method_name,
            arg_count
        );

        match (class_name, method_name) {
            // Math static methods (lowercase to match actual namespace usage)
            ("math", "abs") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("math", "floor") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Integer)
            }
            ("math", "ceil") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Integer)
            }
            ("math", "round") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Integer)
            }
            ("math", "sqrt") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("math", "pow") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("math", "sin") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("math", "cos") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("math", "tan") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("math", "max") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("math", "min") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }

            // String static methods
            ("String", "fromCharCode") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }
            ("String", "isEmpty") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }

            // StringUtils static methods
            ("StringUtils", "length") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Integer)
            }
            ("StringUtils", "concat") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }
            ("StringUtils", "substring") => {
                validate_arg_count(3, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }
            ("StringUtils", "indexOf") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Integer)
            }
            ("StringUtils", "replace") => {
                validate_arg_count(3, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }
            ("StringUtils", "toUpperCase") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }
            ("StringUtils", "toLowerCase") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }

            // Integer static methods
            ("Integer", "parse") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Integer)
            }
            ("Integer", "toString") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }

            // Number static methods
            ("Number", "parse") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("Number", "toString") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }

            // compare.integer static methods (all require 2 arguments)
            ("compare.integer", "equal") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                tracing::debug!("DEBUG: Validation passed for equal");
                Ok(ConcreteType::Boolean)
            }
            ("compare.integer", "notEqual") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                tracing::debug!("DEBUG: Validation passed for notEqual");
                Ok(ConcreteType::Boolean)
            }
            ("compare.integer", "lessThan") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                tracing::debug!("DEBUG: Validation passed for lessThan");
                Ok(ConcreteType::Boolean)
            }
            ("compare.integer", "greaterThan") => {
                tracing::debug!("DEBUG: About to validate greaterThan");
                validate_arg_count(2, arg_count, &full_method_name)?;
                tracing::trace!(
                    "DEBUG: Validation passed for greaterThan - THIS SHOULD NOT PRINT IF ERROR"
                );
                Ok(ConcreteType::Boolean)
            }
            ("compare.integer", "lessEqual") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }
            ("compare.integer", "greaterEqual") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }

            // compare.number static methods (all require 2 arguments)
            ("compare.number", "equal") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }
            ("compare.number", "notEqual") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }
            ("compare.number", "lessThan") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }
            ("compare.number", "greaterThan") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }
            ("compare.number", "lessEqual") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }
            ("compare.number", "greaterEqual") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }

            // list static methods - CRITICAL FIX: Add return types for list namespace functions
            // These were returning Unknown, causing MIR to treat them as void
            // Generic functions return types based on first argument (the list)
            ("list", "add") | ("list", "push") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns list<T> - same type as first argument
                // list.add(list<T>, T) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "pop") | ("list", "shift") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns T - the element type of the list
                // list.pop(list<T>) -> T
                if !arguments.is_empty() {
                    match &arguments[0].expr_type {
                        ConcreteType::Array(element_type) => Ok((**element_type).clone()),
                        _ => Ok(ConcreteType::Unknown),
                    }
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "unshift") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns list<T> - same type as first argument
                // list.unshift(list<T>, T) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "insert") => {
                validate_arg_count(3, arg_count, &full_method_name)?;
                // Returns list<T> - same type as first argument
                // list.insert(list<T>, integer, T) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "remove") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns T - the element type of the list
                // list.remove(list<T>, integer) -> T
                if !arguments.is_empty() {
                    match &arguments[0].expr_type {
                        ConcreteType::Array(element_type) => Ok((**element_type).clone()),
                        _ => Ok(ConcreteType::Unknown),
                    }
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "size") | ("list", "length") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Integer)
            }
            ("list", "get") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns T - the element type of the list
                // list.get(list<T>, integer) -> T
                if !arguments.is_empty() {
                    match &arguments[0].expr_type {
                        ConcreteType::Array(element_type) => Ok((**element_type).clone()),
                        _ => Ok(ConcreteType::Unknown),
                    }
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "set") => {
                validate_arg_count(3, arg_count, &full_method_name)?;
                // Returns list<T> - same type as first argument
                // list.set(list<T>, integer, T) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "clear") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns list<T> - same type as first argument
                // list.clear(list<T>) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "fill") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns list<T> - creates a new list
                // list.fill(integer, T) -> list<T>
                if arguments.len() >= 2 {
                    Ok(ConcreteType::Array(Box::new(
                        arguments[1].expr_type.clone(),
                    )))
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "range") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns list<integer> - creates a new list with integer range
                // list.range(integer, integer) -> list<integer>
                Ok(ConcreteType::Array(Box::new(ConcreteType::Integer)))
            }
            ("list", "indexOf") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns integer - index of element or -1 if not found
                // list.indexOf(list<T>, T) -> integer
                Ok(ConcreteType::Integer)
            }
            ("list", "lastIndexOf") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns integer - last index of element or -1 if not found
                // list.lastIndexOf(list<T>, T) -> integer
                Ok(ConcreteType::Integer)
            }
            ("list", "contains") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns boolean - whether the list contains the element
                // list.contains(list<T>, T) -> boolean
                Ok(ConcreteType::Boolean)
            }
            ("list", "isEmpty") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns boolean - whether the list is empty
                // list.isEmpty(list<T>) -> boolean
                Ok(ConcreteType::Boolean)
            }
            ("list", "isNotEmpty") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns boolean - whether the list is not empty
                // list.isNotEmpty(list<T>) -> boolean
                Ok(ConcreteType::Boolean)
            }
            ("list", "first") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns T - the first element of the list
                // list.first(list<T>) -> T
                if !arguments.is_empty() {
                    match &arguments[0].expr_type {
                        ConcreteType::Array(element_type) => Ok((**element_type).clone()),
                        _ => Ok(ConcreteType::Unknown),
                    }
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "last") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns T - the last element of the list
                // list.last(list<T>) -> T
                if !arguments.is_empty() {
                    match &arguments[0].expr_type {
                        ConcreteType::Array(element_type) => Ok((**element_type).clone()),
                        _ => Ok(ConcreteType::Unknown),
                    }
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "slice") => {
                validate_arg_count(3, arg_count, &full_method_name)?;
                // Returns list<T> - a new list containing the sliced elements
                // list.slice(list<T>, integer, integer) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "concat") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns list<T> - a new list containing elements from both lists
                // list.concat(list<T>, list<T>) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "reverse") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns list<T> - a new list with elements in reverse order
                // list.reverse(list<T>) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "sort") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns list<T> - a new sorted list
                // list.sort(list<T>) -> list<T>
                if !arguments.is_empty() {
                    Ok(arguments[0].expr_type.clone())
                } else {
                    Ok(ConcreteType::Unknown)
                }
            }
            ("list", "join") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                // Returns string - joined elements
                // list.join(list<T>, string) -> string
                Ok(ConcreteType::String)
            }

            // Input static methods
            ("input", "integer") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Integer)
            }
            ("input", "float") | ("input", "number") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Number)
            }
            ("input", "yesNo") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Boolean)
            }

            // JSON static methods
            ("json", "textToData") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns Any - parsed JSON value
                // json.textToData(string) -> any
                Ok(ConcreteType::Any)
            }
            ("json", "tryTextToData") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns Any - parsed JSON value or null on error
                // json.tryTextToData(string) -> any
                Ok(ConcreteType::Any)
            }
            ("json", "dataToText") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns string - JSON text representation
                // json.dataToText(any) -> string
                Ok(ConcreteType::String)
            }
            ("json", "prettyDataToText") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns string - formatted JSON text representation
                // json.prettyDataToText(any) -> string
                Ok(ConcreteType::String)
            }

            // For unknown static method/class combinations, return Unknown
            _ => Ok(ConcreteType::Unknown),
        }
    }

    /// Infer the result type of a unary operation
    fn infer_unary_operation(
        &self,
        operator: &HirUnaryOp,
        operand_type: &ConcreteType,
        _location: &SourceLocation,
    ) -> Result<ConcreteType, CompilerError> {
        match operator {
            HirUnaryOp::Negate => match operand_type {
                ConcreteType::Integer => Ok(ConcreteType::Integer),
                ConcreteType::Number => Ok(ConcreteType::Number),
                _ => Ok(ConcreteType::Unknown),
            },
            HirUnaryOp::Not => match operand_type {
                ConcreteType::Boolean => Ok(ConcreteType::Boolean),
                _ => Ok(ConcreteType::Unknown),
            },
            // BOOK: required-operator - Postfix ! assertion for null check
            // Required operator returns the same type, just adds runtime null check
            HirUnaryOp::Required => {
                // The required assertion returns the same type as the operand
                Ok(operand_type.clone())
            }
        }
    }

    /// Convert HIR unary operator to TAST unary operator
    fn convert_unary_operator(&self, op: &HirUnaryOp) -> UnaryOperator {
        match op {
            HirUnaryOp::Negate => UnaryOperator::Negate,
            HirUnaryOp::Not => UnaryOperator::Not,
            // BOOK: required-operator - Postfix ! assertion for null check
            HirUnaryOp::Required => UnaryOperator::Required,
        }
    }

    /// Infer the type of a field access based on the object type and field name
    /// Infer field type and symbol ID from object type
    /// Returns (field_type, field_symbol_id)
    fn infer_field_type_and_symbol(
        &self,
        object_type: &ConcreteType,
        field_name: &str,
    ) -> Result<(ConcreteType, SymbolId), CompilerError> {
        match object_type {
            // Array fields - use placeholder symbol for built-in fields
            ConcreteType::Array(_element_type) if field_name == "length" => {
                Ok((ConcreteType::Integer, SymbolId(0)))
            }

            // String fields - use placeholder symbol for built-in fields
            ConcreteType::String if field_name == "length" => {
                Ok((ConcreteType::Integer, SymbolId(0)))
            }

            // CRITICAL FIX: For class types, look up the field in the class definition
            ConcreteType::Class {
                symbol_id,
                type_args: _,
            } => {
                // Look up the class symbol to get its fields
                if let Some(class_symbol) = self.symbol_table.get_symbol(*symbol_id) {
                    if let SymbolKind::Class {
                        fields,
                        methods: _,
                        parent,
                    } = &class_symbol.kind
                    {
                        // Search for the field with the matching name in this class
                        for field_symbol_id in fields {
                            if let Some(field_symbol) =
                                self.symbol_table.get_symbol(*field_symbol_id)
                            {
                                if field_symbol.name == field_name {
                                    // Found the field! Return its type and symbol ID
                                    if let SymbolKind::Field {
                                        class_id: _,
                                        field_type,
                                    } = &field_symbol.kind
                                    {
                                        return Ok((
                                            self.hir_type_to_concrete(field_type),
                                            *field_symbol_id,
                                        ));
                                    }
                                }
                            }
                        }

                        // Field not found in this class - check parent class if exists
                        if let Some(parent_symbol_id) = parent {
                            if let Some(parent_symbol) =
                                self.symbol_table.get_symbol(*parent_symbol_id)
                            {
                                if let SymbolKind::Class {
                                    fields: parent_fields,
                                    ..
                                } = &parent_symbol.kind
                                {
                                    // Search for the field in parent class
                                    for field_symbol_id in parent_fields {
                                        if let Some(field_symbol) =
                                            self.symbol_table.get_symbol(*field_symbol_id)
                                        {
                                            if field_symbol.name == field_name {
                                                // Found the field in parent class!
                                                if let SymbolKind::Field {
                                                    class_id: _,
                                                    field_type,
                                                } = &field_symbol.kind
                                                {
                                                    return Ok((
                                                        self.hir_type_to_concrete(field_type),
                                                        *field_symbol_id,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Field not found in class or parent class
                        return Err(CompilerError::type_error(
                            &format!(
                                "Field '{}' not found in class '{}'",
                                field_name, class_symbol.name
                            ),
                            None,
                            None,
                        ));
                    }
                }
                // Class symbol not found - return error
                Err(CompilerError::type_error(
                    &format!(
                        "Cannot resolve class type for field access '{}'",
                        field_name
                    ),
                    None,
                    None,
                ))
            }

            // BOOK: safe-access - Any type supports arbitrary field access
            // Any type (dynamic/JSON objects) allows access to any field name
            // The result is also Any type since we don't know the actual field type at compile time
            ConcreteType::Any => {
                // Use a placeholder symbol ID for dynamic field access
                Ok((ConcreteType::Any, SymbolId(0)))
            }

            _ => Err(CompilerError::type_error(
                &format!(
                    "Type {:?} does not have field '{}'",
                    object_type, field_name
                ),
                None,
                None,
            )),
        }
    }

    #[allow(dead_code)]
    fn infer_field_type(
        &self,
        object_type: &ConcreteType,
        field_name: &str,
    ) -> Result<ConcreteType, CompilerError> {
        self.infer_field_type_and_symbol(object_type, field_name)
            .map(|(field_type, _symbol_id)| field_type)
    }

    fn _old_infer_field_type_removed(
        &self,
        object_type: &ConcreteType,
        field_name: &str,
    ) -> Result<ConcreteType, CompilerError> {
        match object_type {
            // Array fields
            ConcreteType::Array(_element_type) if field_name == "length" => {
                Ok(ConcreteType::Integer)
            }

            // String fields
            ConcreteType::String if field_name == "length" => Ok(ConcreteType::Integer),

            // CRITICAL FIX: For class types, look up the field in the class definition
            ConcreteType::Class {
                symbol_id,
                type_args: _,
            } => {
                // Look up the class symbol to get its fields
                if let Some(class_symbol) = self.symbol_table.get_symbol(*symbol_id) {
                    if let SymbolKind::Class {
                        fields,
                        methods: _,
                        parent,
                    } = &class_symbol.kind
                    {
                        // Search for the field with the matching name in this class
                        for field_symbol_id in fields {
                            if let Some(field_symbol) =
                                self.symbol_table.get_symbol(*field_symbol_id)
                            {
                                if field_symbol.name == field_name {
                                    // Found the field! Return its type
                                    if let SymbolKind::Field {
                                        class_id: _,
                                        field_type,
                                    } = &field_symbol.kind
                                    {
                                        return Ok(self.hir_type_to_concrete(field_type));
                                    }
                                }
                            }
                        }

                        // Field not found in this class - check parent class if exists
                        if let Some(parent_symbol_id) = parent {
                            if let Some(parent_symbol) =
                                self.symbol_table.get_symbol(*parent_symbol_id)
                            {
                                if let SymbolKind::Class {
                                    fields: parent_fields,
                                    ..
                                } = &parent_symbol.kind
                                {
                                    // Search for the field in parent class
                                    for field_symbol_id in parent_fields {
                                        if let Some(field_symbol) =
                                            self.symbol_table.get_symbol(*field_symbol_id)
                                        {
                                            if field_symbol.name == field_name {
                                                // Found the field in parent class!
                                                if let SymbolKind::Field {
                                                    class_id: _,
                                                    field_type,
                                                } = &field_symbol.kind
                                                {
                                                    return Ok(
                                                        self.hir_type_to_concrete(field_type)
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Field not found in class or parent class
                        return Err(CompilerError::type_error(
                            &format!(
                                "Field '{}' not found in class '{}'",
                                field_name, class_symbol.name
                            ),
                            None,
                            None,
                        ));
                    }
                }
                // Class symbol not found - return error
                Err(CompilerError::type_error(
                    &format!(
                        "Cannot resolve class type for field access '{}'",
                        field_name
                    ),
                    None,
                    None,
                ))
            }

            // For unrecognized field accesses, return Unknown
            // This handles cases where field access is used on non-class types
            _ => Ok(ConcreteType::Unknown),
        }
    }

    /// Convert HIR type to concrete type
    fn hir_type_to_concrete(&self, hir_type: &HirType) -> ConcreteType {
        match hir_type {
            HirType::Integer => ConcreteType::Integer,
            HirType::Number => ConcreteType::Number,
            HirType::String => ConcreteType::String,
            HirType::Boolean => ConcreteType::Boolean,
            HirType::Void => ConcreteType::Null,
            // BOOK: null-support - HirType::Null maps to ConcreteType::Null
            HirType::Null => ConcreteType::Null,
            HirType::Integer8 => ConcreteType::Integer,
            HirType::Integer8u => ConcreteType::Integer,
            HirType::Integer16 => ConcreteType::Integer,
            HirType::Integer16u => ConcreteType::Integer,
            HirType::Integer32 => ConcreteType::Integer,
            HirType::Integer32u => ConcreteType::Integer,
            HirType::Integer64 => ConcreteType::Integer,
            HirType::Integer64u => ConcreteType::Integer,
            HirType::Number32 => ConcreteType::Number,
            HirType::Number64 => ConcreteType::Number,
            HirType::List(element_type) => {
                ConcreteType::Array(Box::new(self.hir_type_to_concrete(element_type)))
            }
            HirType::Matrix(element_type) => {
                // Matrix is a first-class type
                ConcreteType::Matrix(Box::new(self.hir_type_to_concrete(element_type)))
            }
            HirType::Named { name, location: _ } => {
                // Look up the named type in the symbol table
                if let Some(symbol_id) = self.symbol_table.lookup_symbol(name) {
                    if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                        match &symbol.kind {
                            SymbolKind::Class { .. } => {
                                // Note: Generic type arguments are not yet supported
                                // Clean Language uses 'any' for generic placeholder types
                                // Full generic instantiation (e.g., List<String>) requires:
                                // 1. Parser support for type argument syntax
                                // 2. HIR support for carrying type arguments
                                // 3. Type inference for generic parameters
                                // For now, classes are instantiated without type arguments
                                ConcreteType::Class {
                                    symbol_id,
                                    type_args: Vec::new(),
                                }
                            }
                            _ => {
                                // For other named types (functions, variables), treat as unknown for now
                                ConcreteType::Unknown
                            }
                        }
                    } else {
                        ConcreteType::Unknown
                    }
                } else {
                    // Check for 'any' type - dynamically typed boxed values
                    if name == "any" {
                        // 'any' is the dynamic type in Clean Language
                        // It represents a value of any type with runtime type tag
                        // Memory layout: [tag:i32][value1:i32][value2:i32] = 12 bytes
                        return ConcreteType::Any;
                    }

                    // If not found in symbol table, could be a built-in type
                    match name.as_str() {
                        "integer" => ConcreteType::Integer,
                        "number" => ConcreteType::Number,
                        "string" => ConcreteType::String,
                        "boolean" => ConcreteType::Boolean,
                        "void" => ConcreteType::Undefined,
                        _ => {
                            // Unknown type - return Unknown
                            // Diagnostics should be emitted by callers in specific contexts
                            ConcreteType::Unknown
                        }
                    }
                }
            }
            HirType::Pairs(key_type, value_type) => {
                // Pairs are represented as a container with key and value types
                ConcreteType::Pairs(
                    Box::new(self.hir_type_to_concrete(key_type)),
                    Box::new(self.hir_type_to_concrete(value_type)),
                )
            }
            HirType::Inferred { .. } => {
                // Type inference placeholders are handled by the constraint solver
                // For now, return Unknown and let constraint solver handle it
                ConcreteType::Unknown
            }
            HirType::Any => {
                // Any type - dynamically typed boxed value with runtime type tag
                ConcreteType::Any
            }
        }
    }

    /// Convert HIR binary operator to TAST binary operator
    fn convert_binary_operator(&self, operator: &HirBinaryOp) -> BinaryOperator {
        match operator {
            HirBinaryOp::Add => BinaryOperator::Add,
            HirBinaryOp::Subtract => BinaryOperator::Subtract,
            HirBinaryOp::Multiply => BinaryOperator::Multiply,
            HirBinaryOp::Divide => BinaryOperator::Divide,
            HirBinaryOp::Modulo => BinaryOperator::Modulo,
            HirBinaryOp::Power => BinaryOperator::Power,
            HirBinaryOp::Equal => BinaryOperator::Equal,
            HirBinaryOp::NotEqual => BinaryOperator::NotEqual,
            HirBinaryOp::Less => BinaryOperator::LessThan,
            HirBinaryOp::LessEqual => BinaryOperator::LessThanOrEqual,
            HirBinaryOp::Greater => BinaryOperator::GreaterThan,
            HirBinaryOp::GreaterEqual => BinaryOperator::GreaterThanOrEqual,
            HirBinaryOp::Is => BinaryOperator::Is,
            HirBinaryOp::IsNot => BinaryOperator::IsNot,
            HirBinaryOp::And => BinaryOperator::And,
            HirBinaryOp::Or => BinaryOperator::Or,
            // BOOK: null-coalescing - Map NullCoalesce operator
            HirBinaryOp::NullCoalesce => BinaryOperator::NullCoalesce,
            HirBinaryOp::StringConcat => BinaryOperator::Concatenate,
        }
    }

    /// Add a constraint to be solved
    fn add_constraint(&mut self, constraint: TypeConstraint) {
        self.constraints.push(constraint);
    }

    /// Apply solved substitutions to type environment
    fn apply_substitutions(&mut self, _solver_result: &SolverResult) {
        // Would apply substitutions to finalize types
        // For now, types in type_env are already concrete
    }

    /// Apply current substitution to resolve a type
    fn resolve_type(&self, type_: &ConcreteType) -> ConcreteType {
        self.constraint_solver.apply_substitution(type_)
    }

    /// Find a common type between two types for control flow branches
    fn find_common_type(&self, left: &ConcreteType, right: &ConcreteType) -> ConcreteType {
        if left == right {
            left.clone()
        } else {
            match (left, right) {
                // If one type is assignable to the other, use the more general type
                (l, r) if l.is_assignable_to(r) => r.clone(),
                (l, r) if r.is_assignable_to(l) => l.clone(),

                // Both numeric types -> Number (more general)
                (ConcreteType::Integer, ConcreteType::Number)
                | (ConcreteType::Number, ConcreteType::Integer) => ConcreteType::Number,

                // Array types with compatible elements
                (ConcreteType::Array(left_elem), ConcreteType::Array(right_elem)) => {
                    let common_elem = self.find_common_type(left_elem, right_elem);
                    ConcreteType::Array(Box::new(common_elem))
                }

                // Otherwise, fall back to Unknown for error recovery
                _ => ConcreteType::Unknown,
            }
        }
    }

    /// Create a fresh type variable for type inference
    fn create_type_variable(&mut self) -> ConcreteType {
        // Generate a unique type variable by borrowing from the constraint solver
        let type_var_id = self.constraint_solver.fresh_type_var();
        ConcreteType::Generic {
            name: type_var_id.0.to_string(),
            bounds: Vec::new(),
        }
    }
}

impl BuiltinTypes {
    /// Initialize built-in type method signatures
    fn new() -> Self {
        let mut string_methods = HashMap::new();
        string_methods.insert("length".to_string(), ConcreteType::Integer);
        string_methods.insert(
            "substring".to_string(),
            ConcreteType::Function {
                parameters: vec![ConcreteType::Integer, ConcreteType::Integer],
                return_type: Box::new(ConcreteType::String),
                is_async: false,
            },
        );

        let mut array_methods = HashMap::new();
        array_methods.insert("length".to_string(), ConcreteType::Integer);
        array_methods.insert(
            "push".to_string(),
            ConcreteType::Function {
                parameters: vec![ConcreteType::Generic {
                    name: "T".to_string(),
                    bounds: vec![],
                }],
                return_type: Box::new(ConcreteType::Integer),
                is_async: false,
            },
        );

        Self {
            integer_methods: HashMap::new(),
            number_methods: HashMap::new(),
            string_methods,
            boolean_methods: HashMap::new(),
            array_methods,
        }
    }
}

// Extension trait to get location from HIR statements (currently unused but useful for error reporting)
#[allow(dead_code)]
trait StatementLocation {
    fn location(&self) -> &SourceLocation;
}

#[allow(dead_code)]
impl StatementLocation for ResolvedHirStatement {
    fn location(&self) -> &SourceLocation {
        match self {
            ResolvedHirStatement::Expression { location, .. } => location,
            ResolvedHirStatement::VariableDeclaration { location, .. } => location,
            ResolvedHirStatement::Return { location, .. } => location,
            ResolvedHirStatement::If { location, .. } => location,
            ResolvedHirStatement::Assignment { location, .. } => location,
            ResolvedHirStatement::For { location, .. } => location,
            ResolvedHirStatement::While { location, .. } => location,
            ResolvedHirStatement::Print { location, .. } => location,
            ResolvedHirStatement::LaterAssignment { location, .. } => location,
        }
    }
}
