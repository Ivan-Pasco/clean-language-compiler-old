//! Type inference engine for Clean Language
//!
//! Generates type constraints from resolved HIR and performs type inference
//! using constraint-based approach with Hindley-Milner algorithm.

use super::constraint_solver::{ConstraintSolver, SolverResult};
use super::tast::{
    BinaryOperator, ConcreteType, TastBlock, TastClass, TastComputedDeclaration, TastExpression,
    TastExpressionKind, TastField, TastFunction, TastGuardClause, TastLiteral, TastParameter,
    TastProgram, TastStateBlock, TastStateDeclaration, TastStateScope, TastStatement,
    TastWatchBlock, TypeConstraint, UnaryOperator, Visibility,
};
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::{HirBinaryOp, HirStateScope, HirType, HirUnaryOp};
use crate::resolver::{
    GlobalSymbolTable, ResolvedHirBlock, ResolvedHirClass, ResolvedHirExpression,
    ResolvedHirFunction, ResolvedHirLValue, ResolvedHirMethod, ResolvedHirProgram,
    ResolvedHirStateBlock, ResolvedHirStatement, ResolvedHirWatchBlock, SymbolId, SymbolKind,
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
    #[allow(dead_code)] // BuiltinTypes constructed but lookups go through symbol_table instead
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

        // Register state variables from symbol table
        self.register_state_variables();

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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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

        // Dynamically register ALL namespace symbols (including plugin-provided ones
        // like req, db, ui) so they are recognized as valid identifiers
        for (&symbol_id, symbol_info) in self.symbol_table.all_symbols() {
            if matches!(
                symbol_info.kind,
                crate::resolver::symbol_table::SymbolKind::Namespace { .. }
            ) {
                self.type_env
                    .entry(symbol_id)
                    .or_insert(ConcreteType::Namespace);
            }
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
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
                    is_background: false,
                },
            );
        }

        // validator.getErrors - gets errors from failed result
        // Returns a list<string> pointer (matches resolver registration and HIR desugaring)
        if let Some(symbol_id) = self
            .symbol_table
            .lookup_symbol_in_scope("validator.getErrors", crate::resolver::ScopeId(0))
        {
            self.type_env.insert(
                symbol_id,
                ConcreteType::Function {
                    parameters: vec![ConcreteType::Integer], // ValidationResult pointer
                    return_type: Box::new(ConcreteType::Array(Box::new(ConcreteType::String))), // Returns list<string>
                    is_background: false,
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
                            self.type_env.entry(symbol_id).or_insert_with(|| {
                                let concrete_params: Vec<ConcreteType> = parameters
                                    .iter()
                                    .map(Self::hir_type_to_concrete_type)
                                    .collect();
                                let concrete_return = return_type
                                    .as_ref()
                                    .map(|t| Box::new(Self::hir_type_to_concrete_type(t)))
                                    .unwrap_or_else(|| Box::new(ConcreteType::Null));

                                ConcreteType::Function {
                                    parameters: concrete_params,
                                    return_type: concrete_return,
                                    is_background: false,
                                }
                            });
                        }
                        _ => {
                            // Skip non-function builtins (classes, namespaces, etc.)
                        }
                    }
                }
            }
        }
    }

    /// Register state variables from symbol table in type environment
    fn register_state_variables(&mut self) {
        // Scan all symbols in the global scope and register state variables
        let all_symbols: Vec<_> = self.symbol_table.accessible_symbols();
        for symbol_id in all_symbols {
            if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                if let crate::resolver::SymbolKind::StateVariable { var_type, .. } = &symbol.kind {
                    // Convert HirType to ConcreteType and register in type environment
                    let concrete_type = Self::hir_type_to_concrete_type(var_type);
                    self.type_env.insert(symbol_id, concrete_type);
                    tracing::trace!(
                        "Registered state variable '{}' (SymbolId {:?}) with type",
                        symbol.name,
                        symbol_id
                    );
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
            // Precision types - preserve bit width through type inference
            HirType::Integer8 => ConcreteType::IntegerSized {
                bits: 8,
                unsigned: false,
            },
            HirType::Integer8u => ConcreteType::IntegerSized {
                bits: 8,
                unsigned: true,
            },
            HirType::Integer16 => ConcreteType::IntegerSized {
                bits: 16,
                unsigned: false,
            },
            HirType::Integer16u => ConcreteType::IntegerSized {
                bits: 16,
                unsigned: true,
            },
            HirType::Integer32 => ConcreteType::IntegerSized {
                bits: 32,
                unsigned: false,
            },
            HirType::Integer32u => ConcreteType::IntegerSized {
                bits: 32,
                unsigned: true,
            },
            HirType::Integer64 => ConcreteType::IntegerSized {
                bits: 64,
                unsigned: false,
            },
            HirType::Integer64u => ConcreteType::IntegerSized {
                bits: 64,
                unsigned: true,
            },
            HirType::Number32 => ConcreteType::NumberSized { bits: 32 },
            HirType::Number64 => ConcreteType::NumberSized { bits: 64 },
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

        // Type-check state block if present
        let tast_state = if let Some(ref state_block) = program.state {
            match self.infer_state_block(state_block) {
                Ok(tast_state) => Some(tast_state),
                Err(error) => {
                    self.errors.push(error);
                    None
                }
            }
        } else {
            None
        };

        // Convert external functions (WASM imports)
        let tast_externals: Vec<crate::typechecker::tast::TastExternalFunction> = program
            .externals
            .iter()
            .map(|ext| crate::typechecker::tast::TastExternalFunction {
                name: ext.name.clone(),
                parameters: ext
                    .parameters
                    .iter()
                    .enumerate()
                    .map(|(idx, p)| crate::typechecker::tast::TastParameter {
                        symbol_id: crate::resolver::SymbolId(10000 + idx),
                        name: p.name.clone(),
                        param_type: self.hir_type_to_concrete(&p.param_type),
                        default_value: None,
                        is_variadic: false,
                        location: p.location.clone(),
                    })
                    .collect(),
                return_type: self.hir_type_to_concrete(&ext.return_type),
                module: ext.module.clone(),
                location: ext.location.clone(),
            })
            .collect();

        // Type-check top-level watch blocks.
        //
        // Each watch block's body is treated as a void statement sequence — it
        // does not need to return a value.  We validate that the body statements
        // are type-correct and propagate any errors into the error list so that
        // compilation continues and subsequent errors are also reported.
        let mut tast_watch_blocks: Vec<TastWatchBlock> = Vec::new();
        for watch in &program.watch_blocks {
            match self.infer_watch_block(watch) {
                Ok(tast_watch) => tast_watch_blocks.push(tast_watch),
                Err(error) => {
                    tracing::trace!(
                        targets = ?watch.targets,
                        "ERROR: Failed to infer watch block: {:?}",
                        error
                    );
                    self.errors.push(error);
                }
            }
        }

        // Type-check top-level test blocks.
        //
        // Each test body is a single Return statement that evaluates (lhs == rhs).
        // The body is inferred using the standard block inference path.
        let mut tast_tests: Vec<crate::typechecker::tast::TastTest> = Vec::new();
        for test in &program.tests {
            match self.infer_block(&test.body) {
                Ok(tast_body) => {
                    tast_tests.push(crate::typechecker::tast::TastTest {
                        name: test.name.clone(),
                        body: tast_body,
                        is_background: false,
                        location: test.location.clone(),
                    });
                }
                Err(error) => {
                    tracing::trace!(
                        name = %test.name,
                        "ERROR: Failed to infer test block: {:?}",
                        error
                    );
                    self.errors.push(error);
                }
            }
        }

        // Type inference completed successfully

        TastProgram {
            functions: tast_functions,
            classes: tast_classes,
            start_function: tast_start_function,
            imports: Vec::new(), // Would convert imports here
            tests: tast_tests,
            state: tast_state,
            watch_blocks: tast_watch_blocks,
            type_env: self.type_env.clone(),
            location: program.location.clone(),
            // NOTE: Pass symbol table through to MIR for dynamic SymbolId resolution
            symbol_table: std::sync::Arc::new(program.symbol_table.clone()),
            externals: tast_externals,
        }
    }

    /// Type-check a watch block's body.
    ///
    /// Watch blocks do not have an expected return type — their body is purely
    /// for side effects.  This function infers each statement in the body and
    /// reports type errors, but does not require a `return` statement.
    fn infer_watch_block(
        &mut self,
        watch: &ResolvedHirWatchBlock,
    ) -> Result<TastWatchBlock, CompilerError> {
        // SCOPE004: Watch block target must reference a state variable.
        // spec/semantic-rules.md SCOPE004: "Watch block target identifiers must reference
        // variables declared in a `state:` block."
        for (target_name, &symbol_id) in watch.targets.iter().zip(watch.target_symbol_ids.iter()) {
            match self.symbol_table.get_symbol(symbol_id) {
                None => {
                    self.errors.push(CompilerError::Validation {
                        context: Box::new(
                            crate::error::ErrorContext::new(
                                format!("Watch target '{}' is not defined", target_name),
                                Some(
                                    "Watch targets must reference declared state variables"
                                        .to_string(),
                                ),
                                crate::error::ErrorType::Validation,
                                Some(watch.location.clone()),
                            )
                            .with_severity(crate::error::ErrorSeverity::Error)
                            .with_error_code("SCOPE004"),
                        ),
                    });
                }
                Some(symbol) => {
                    if !matches!(symbol.kind, SymbolKind::StateVariable { .. }) {
                        self.errors.push(CompilerError::Validation {
                            context: Box::new(
                                crate::error::ErrorContext::new(
                                    format!(
                                        "Watch target '{}' does not reference a state variable",
                                        target_name
                                    ),
                                    Some(
                                        "Only variables declared in a state: block can be watched"
                                            .to_string(),
                                    ),
                                    crate::error::ErrorType::Validation,
                                    Some(watch.location.clone()),
                                )
                                .with_severity(crate::error::ErrorSeverity::Error)
                                .with_error_code("SCOPE004"),
                            ),
                        });
                    }
                }
            }
        }

        let tast_body = self.infer_block(&watch.body)?;

        Ok(TastWatchBlock {
            targets: watch.targets.clone(),
            target_symbol_ids: watch.target_symbol_ids.clone(),
            body: tast_body,
            location: watch.location.clone(),
        })
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
            is_background: function.is_background,
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
            is_background: false, // Methods are not async by default
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
        self.current_return_type = function
            .return_type
            .as_ref()
            .map(|return_type| self.hir_type_to_concrete(return_type));

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

        // Gap 1: Return path analysis (FUNC004)
        // Warn when a non-void function may not return a value on all paths.
        // We only warn — never hard-error — to preserve backwards compatibility.
        let is_void_return = matches!(
            declared_return_type,
            ConcreteType::Null | ConcreteType::Undefined | ConcreteType::Never
        );
        if !is_void_return && function.return_type.is_some() {
            let body_definitely_returns = Self::block_definitely_returns(&tast_body);
            if !body_definitely_returns {
                let warning = CompilerError::Validation {
                    context: Box::new(
                        crate::error::ErrorContext::new(
                            format!(
                                "Function '{}' may not return a value on all paths",
                                function.name
                            ),
                            Some("Ensure every code path ends with a return statement".to_string()),
                            crate::error::ErrorType::Validation,
                            Some(function.location.clone()),
                        )
                        .with_severity(crate::error::ErrorSeverity::Warning)
                        .with_error_code("FUNC004"),
                    ),
                };
                self.warnings.push(warning);
            }
        }

        // Optional return type inference (spec Step 3):
        // If the body has any `return none` path AND any non-none return path, the
        // effective return type is Optional(T) — regardless of what was explicitly declared.
        // If the function explicitly declared a non-null/non-void return type and the body
        // has `return none` paths, we wrap in Optional.
        let has_none_return = Self::block_has_none_return(&tast_body);
        let has_non_none_return = Self::block_has_non_none_return(&tast_body);
        let final_return_type = if has_none_return && has_non_none_return {
            // Mixed paths: return type is Optional(T) where T is the non-none type

            match &declared_return_type {
                // If already Optional, keep it
                ConcreteType::Optional(_) => declared_return_type.clone(),
                // If declared as Null/Undefined/Unknown, treat inner as Unknown
                ConcreteType::Null | ConcreteType::Undefined | ConcreteType::Unknown => {
                    ConcreteType::Optional(Box::new(ConcreteType::Unknown))
                }
                other => ConcreteType::Optional(Box::new(other.clone())),
            }
        } else {
            declared_return_type
        };

        self.current_function = None;
        self.current_return_type = None;

        let return_debug = format!("{:?}", final_return_type);
        if return_debug.contains("Pairs") || return_debug.contains("Matrix") {
            tracing::trace!(
                "[DEBUG infer_function END] Function '{}' final TastFunction.return_type:",
                function.name
            );
            tracing::trace!("  {:?}", final_return_type);
        }

        Ok(TastFunction {
            symbol_id: function.symbol_id,
            name: function.name.clone(),
            parameters: tast_parameters,
            return_type: final_return_type,
            body: tast_body,
            generic_params: Vec::new(), // Would handle generics here
            constraints: Vec::new(),
            is_background: function.is_background,
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
            is_background: false,
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
            is_background: false,           // Methods are typically not async
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

        // Type-check always: block expressions — each must be boolean.
        // Re-enter the class scope so field names are visible.
        self.current_class = Some(class.symbol_id);
        let mut tast_invariants: Vec<TastExpression> = Vec::new();
        for invariant_expr in &class.invariants {
            match self.infer_expression(invariant_expr) {
                Ok(tast_expr) => {
                    if tast_expr.expr_type != ConcreteType::Boolean
                        && tast_expr.expr_type != ConcreteType::Unknown
                    {
                        return Err(CompilerError::Type {
                            context: Box::new(
                                crate::error::ErrorContext::new(
                                    format!(
                                        "always: condition must be boolean, found {:?}",
                                        tast_expr.expr_type
                                    ),
                                    None,
                                    crate::error::ErrorType::Type,
                                    Some(class.location.clone()),
                                )
                                .with_error_code("CLASS006"),
                            ),
                        });
                    }
                    tast_invariants.push(tast_expr);
                }
                Err(e) => return Err(e),
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
            invariants: tast_invariants,
            location: class.location.clone(),
        })
    }

    /// Type-check state block and all its declarations
    fn infer_state_block(
        &mut self,
        state_block: &ResolvedHirStateBlock,
    ) -> Result<TastStateBlock, CompilerError> {
        let mut tast_declarations = Vec::new();

        for decl in &state_block.declarations {
            // Symbol ID already resolved
            let symbol_id = decl.symbol_id;

            // Convert HIR type to concrete type
            let state_type = self.hir_type_to_concrete(&decl.state_type);

            // Type-check the initializer expression
            let initializer = self.infer_expression(&decl.initializer)?;

            // Verify initializer type matches declared type
            if !initializer.expr_type.is_assignable_to(&state_type) {
                return Err(CompilerError::type_error(
                    format!(
                        "Type mismatch in state variable '{}': expected {}, found {}",
                        decl.name, state_type, initializer.expr_type
                    ),
                    Some(format!(
                        "Consider changing the initializer to match type {}",
                        state_type
                    )),
                    Some(decl.location.clone()),
                ));
            }

            // Type-check guard clause if present
            let guard = if let Some(ref resolved_guard) = decl.guard {
                // Add 'value' to the type environment with the state variable's type
                // This allows the guard condition to reference 'value' (the proposed new value)
                self.type_env
                    .insert(resolved_guard.value_symbol_id, state_type.clone());

                let guard_condition = self.infer_expression(&resolved_guard.condition)?;

                // STATE001: Guard condition must be a pure boolean expression.
                // spec/semantic-rules.md STATE001: "The expression after `guard` must be
                // a pure boolean expression." This is a compile-time error.
                if guard_condition.expr_type != ConcreteType::Boolean
                    && guard_condition.expr_type != ConcreteType::Unknown
                    && guard_condition.expr_type != ConcreteType::Any
                {
                    self.errors.push(CompilerError::Validation {
                        context: Box::new(
                            crate::error::ErrorContext::new(
                                format!(
                                    "Guard condition for state variable '{}' must be boolean, found {}",
                                    decl.name, guard_condition.expr_type
                                ),
                                Some("Guard conditions must evaluate to true or false".to_string()),
                                crate::error::ErrorType::Validation,
                                Some(resolved_guard.location.clone()),
                            )
                            .with_severity(crate::error::ErrorSeverity::Error)
                            .with_error_code("STATE001"),
                        ),
                    });
                }

                // STATE001: Guard expression must be pure — no I/O calls allowed.
                // spec/semantic-rules.md STATE001: Guards are checked synchronously on
                // every state mutation; allowing I/O calls would cause network/file
                // side-effects on every assignment, violating the purity contract.
                if let Some(io_call_name) = find_io_call_in_expression(&guard_condition) {
                    self.errors.push(CompilerError::Validation {
                        context: Box::new(
                            crate::error::ErrorContext::new(
                                format!(
                                    "Guard expression must be pure — found I/O call '{}'. Guards cannot have side effects.",
                                    io_call_name
                                ),
                                Some(
                                    "Remove the I/O call from the guard, or move the I/O operation outside the state declaration."
                                        .to_string(),
                                ),
                                crate::error::ErrorType::Validation,
                                Some(resolved_guard.location.clone()),
                            )
                            .with_severity(crate::error::ErrorSeverity::Error)
                            .with_error_code("STATE001"),
                        ),
                    });
                }

                Some(TastGuardClause {
                    condition: guard_condition,
                    value_symbol_id: resolved_guard.value_symbol_id,
                    error_message: resolved_guard.error_message.clone(),
                    location: resolved_guard.location.clone(),
                })
            } else {
                None
            };

            tast_declarations.push(TastStateDeclaration {
                symbol_id,
                name: decl.name.clone(),
                state_type,
                initializer,
                guard,
                location: decl.location.clone(),
            });
        }

        // STATE003 — Circular dependency detection (spec/semantic-rules.md STATE003).
        // Build a dependency graph: computed symbol_id → set of computed symbol_ids
        // referenced in its body. Then run DFS cycle detection.
        {
            use std::collections::{HashMap, HashSet};

            // Map computed symbol_id → name (for error messages)
            let id_to_name: HashMap<SymbolId, &str> = state_block
                .computed
                .iter()
                .map(|c| (c.symbol_id, c.name.as_str()))
                .collect();

            let computed_ids: HashSet<SymbolId> =
                state_block.computed.iter().map(|c| c.symbol_id).collect();

            // Collect all symbol IDs referenced in a block (recursive helper via closure)
            fn collect_refs_expr(
                expr: &crate::resolver::ResolvedHirExpression,
                out: &mut std::collections::HashSet<SymbolId>,
            ) {
                use crate::resolver::ResolvedHirExpression::*;
                match expr {
                    Variable { symbol_id, .. } => {
                        out.insert(*symbol_id);
                    }
                    BinaryOp { left, right, .. } => {
                        collect_refs_expr(left, out);
                        collect_refs_expr(right, out);
                    }
                    UnaryOp { operand, .. } => {
                        collect_refs_expr(operand, out);
                    }
                    Call { arguments, .. } => {
                        for a in arguments {
                            collect_refs_expr(a, out);
                        }
                    }
                    MethodCall {
                        receiver,
                        arguments,
                        ..
                    } => {
                        collect_refs_expr(receiver, out);
                        for a in arguments {
                            collect_refs_expr(a, out);
                        }
                    }
                    StaticMethodCall { arguments, .. } => {
                        for a in arguments {
                            collect_refs_expr(a, out);
                        }
                    }
                    FieldAccess { object, .. } => {
                        collect_refs_expr(object, out);
                    }
                    Index { array, index, .. } => {
                        collect_refs_expr(array, out);
                        collect_refs_expr(index, out);
                    }
                    Conditional {
                        condition,
                        then_expr,
                        else_expr,
                        ..
                    } => {
                        collect_refs_expr(condition, out);
                        collect_refs_expr(then_expr, out);
                        collect_refs_expr(else_expr, out);
                    }
                    Array { elements, .. } => {
                        for e in elements {
                            collect_refs_expr(e, out);
                        }
                    }
                    Constructor { arguments, .. } => {
                        for a in arguments {
                            collect_refs_expr(a, out);
                        }
                    }
                    OnError {
                        expression,
                        fallback,
                        ..
                    } => {
                        collect_refs_expr(expression, out);
                        collect_refs_expr(fallback, out);
                    }
                    Cast { expression, .. } => collect_refs_expr(expression, out),
                    Assignment { value, .. } => collect_refs_expr(value, out),
                    Range {
                        start, end, step, ..
                    } => {
                        collect_refs_expr(start, out);
                        collect_refs_expr(end, out);
                        if let Some(s) = step {
                            collect_refs_expr(s, out);
                        }
                    }
                    BaseCall { arguments, .. } => {
                        for a in arguments {
                            collect_refs_expr(a, out);
                        }
                    }
                    _ => {}
                }
            }

            fn collect_refs_block(
                block: &crate::resolver::ResolvedHirBlock,
                out: &mut std::collections::HashSet<SymbolId>,
            ) {
                use crate::resolver::ResolvedHirStatement::*;
                for stmt in &block.statements {
                    match stmt {
                        Expression { expression, .. } => collect_refs_expr(expression, out),
                        Assignment { value, .. } => collect_refs_expr(value, out),
                        Return { value: Some(v), .. } => collect_refs_expr(v, out),
                        Return { value: None, .. } => {}
                        If {
                            condition,
                            then_branch,
                            else_branch,
                            ..
                        } => {
                            collect_refs_expr(condition, out);
                            collect_refs_block(then_branch, out);
                            if let Some(b) = else_branch {
                                collect_refs_block(b, out);
                            }
                        }
                        For { iterable, body, .. } => {
                            collect_refs_expr(iterable, out);
                            collect_refs_block(body, out);
                        }
                        While {
                            condition, body, ..
                        } => {
                            collect_refs_expr(condition, out);
                            collect_refs_block(body, out);
                        }
                        Print { expression, .. } => collect_refs_expr(expression, out),
                        VariableDeclaration {
                            initializer: Some(init),
                            ..
                        } => collect_refs_expr(init, out),
                        LaterAssignment { expression, .. } => collect_refs_expr(expression, out),
                        Background { expression, .. } => collect_refs_expr(expression, out),
                        Require { condition, .. } => collect_refs_expr(condition, out),
                        _ => {}
                    }
                }
            }

            // Build dependency edges: only edges to other computed state symbols
            let mut deps: HashMap<SymbolId, HashSet<SymbolId>> = HashMap::new();
            for comp in &state_block.computed {
                let mut refs = HashSet::new();
                collect_refs_block(&comp.body, &mut refs);
                let computed_deps: HashSet<SymbolId> =
                    refs.intersection(&computed_ids).cloned().collect();
                deps.insert(comp.symbol_id, computed_deps);
            }

            // DFS cycle detection with full cycle path tracking.
            // Uses the standard 3-color algorithm:
            //   white (not in map) = unvisited
            //   grey  (1)          = on the current DFS recursion stack
            //   black (2)          = fully explored
            //
            // When a back-edge is found (grey → grey), we extract the cycle by
            // finding the slice of `path` from the repeated node to the current
            // position.  This gives the complete cycle for the error message, e.g.
            // "a → b → c → a".
            let mut color: HashMap<SymbolId, u8> = HashMap::new();
            // Records the full cycle path when one is found (the cycle slice is
            // inclusive of the starting node at both ends).
            let mut cycle_path: Option<Vec<SymbolId>> = None;

            fn dfs_with_path(
                node: SymbolId,
                deps: &HashMap<SymbolId, HashSet<SymbolId>>,
                color: &mut HashMap<SymbolId, u8>,
                path: &mut Vec<SymbolId>,
                cycle_path: &mut Option<Vec<SymbolId>>,
            ) {
                if color.get(&node) == Some(&2) {
                    // Already fully explored — no cycle via this node.
                    return;
                }
                if color.get(&node) == Some(&1) {
                    // Back-edge: `node` is already on the current recursion stack.
                    // Extract the cycle from `path`.
                    if cycle_path.is_none() {
                        if let Some(start_pos) = path.iter().position(|&id| id == node) {
                            let mut cycle = path[start_pos..].to_vec();
                            cycle.push(node); // close the cycle: a → b → ... → a
                            *cycle_path = Some(cycle);
                        }
                    }
                    return;
                }
                color.insert(node, 1); // mark grey
                path.push(node);
                if let Some(children) = deps.get(&node) {
                    // Sort children for deterministic error messages.
                    let mut sorted_children: Vec<SymbolId> = children.iter().cloned().collect();
                    sorted_children.sort_by_key(|id| id.0);
                    for child in sorted_children {
                        if cycle_path.is_some() {
                            break;
                        }
                        dfs_with_path(child, deps, color, path, cycle_path);
                    }
                }
                path.pop();
                color.insert(node, 2); // mark black
            }

            let mut path: Vec<SymbolId> = Vec::new();
            for comp in &state_block.computed {
                if color.get(&comp.symbol_id) != Some(&2) {
                    dfs_with_path(
                        comp.symbol_id,
                        &deps,
                        &mut color,
                        &mut path,
                        &mut cycle_path,
                    );
                }
                if cycle_path.is_some() {
                    break;
                }
            }

            if let Some(cycle) = cycle_path {
                // Build the human-readable path string: "a → b → c → a"
                let path_str = cycle
                    .iter()
                    .map(|id| id_to_name.get(id).copied().unwrap_or("unknown").to_string())
                    .collect::<Vec<_>>()
                    .join(" → ");

                let first_id = cycle[0];
                let location = state_block
                    .computed
                    .iter()
                    .find(|c| c.symbol_id == first_id)
                    .map(|c| c.location.clone());

                self.errors.push(CompilerError::Validation {
                    context: Box::new(
                        crate::error::ErrorContext::new(
                            format!(
                                "Circular dependency detected in computed state: {}",
                                path_str
                            ),
                            Some(
                                "Computed state variables cannot form dependency cycles"
                                    .to_string(),
                            ),
                            crate::error::ErrorType::Validation,
                            location,
                        )
                        .with_severity(crate::error::ErrorSeverity::Error)
                        .with_error_code("STATE003"),
                    ),
                });
            }
        }

        // Type-check computed state declarations.
        //
        // For each computed declaration we open a fresh block scope, infer the
        // body statements, and then verify that the final return statement (if
        // explicitly present) matches the declared computed type.
        let mut tast_computed: Vec<TastComputedDeclaration> = Vec::new();
        for comp in &state_block.computed {
            let computed_type = self.hir_type_to_concrete(&comp.computed_type);

            // Make the computed type visible so that return-type checking inside
            // the body has a target to validate against.
            self.type_env.insert(comp.symbol_id, computed_type.clone());

            // Infer the body in a fresh scope.
            let tast_body = self.infer_block_with_expected_return(
                &comp.body,
                Some(&computed_type),
                &comp.name,
            )?;

            tast_computed.push(TastComputedDeclaration {
                symbol_id: comp.symbol_id,
                name: comp.name.clone(),
                computed_type,
                body: tast_body,
                location: comp.location.clone(),
            });
        }

        // Type-check state invariant rules
        let mut tast_rules = Vec::new();
        for rule_expr in &state_block.rules {
            let rule = self.infer_expression(rule_expr)?;

            // Each rule must be a boolean expression.
            // spec/semantic-rules.md STATE005: "The expression in a `rules:` block must have
            // type `boolean`. Non-boolean expressions are a compile-time error."
            if rule.expr_type != ConcreteType::Boolean
                && rule.expr_type != ConcreteType::Unknown
                && rule.expr_type != ConcreteType::Any
            {
                self.errors.push(CompilerError::Validation {
                    context: Box::new(
                        crate::error::ErrorContext::new(
                            format!(
                                "State rule expression must be a boolean expression, got {}",
                                rule.expr_type
                            ),
                            Some(
                                "Each expression in a rules: block must evaluate to boolean"
                                    .to_string(),
                            ),
                            crate::error::ErrorType::Validation,
                            Some(rule.location.clone()),
                        )
                        .with_severity(crate::error::ErrorSeverity::Error)
                        .with_error_code("STATE005"),
                    ),
                });
            }

            tast_rules.push(rule);
        }

        let scope = match state_block.scope {
            HirStateScope::App => TastStateScope::App,
            HirStateScope::Screen => TastStateScope::Screen,
        };

        Ok(TastStateBlock {
            declarations: tast_declarations,
            computed: tast_computed,
            rules: tast_rules,
            scope,
            location: state_block.location.clone(),
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
                .is_some_and(|e| self.expression_uses_this(e)),
            TastStatement::Assignment { target, value, .. } => {
                self.expression_uses_this(target) || self.expression_uses_this(value)
            }
            TastStatement::Return { value, .. } => {
                value.as_ref().is_some_and(|e| self.expression_uses_this(e))
            }
            TastStatement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                self.expression_uses_this(condition)
                    || self.body_uses_this(then_block)
                    || else_block.as_ref().is_some_and(|b| self.body_uses_this(b))
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
                TastStatement::Expression { expression, .. }
                    // Only the LAST expression statement becomes the block's return type
                    // Other expression statements are discarded (will need DROP in codegen)
                    if is_last_statement =>
                {
                    block_return_type = expression.expr_type.clone();
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
                    // Null/Undefined/Unknown result_type means one or both branches are void
                    // (bare return or no value). Fall through to per-branch logic so we don't
                    // incorrectly infer a non-void return type for a void function.
                    if then_returns || else_returns {
                        if !matches!(
                            result_type,
                            ConcreteType::Null | ConcreteType::Undefined | ConcreteType::Unknown
                        ) {
                            block_return_type = result_type.clone();
                        } else if then_returns {
                            block_return_type = then_block.return_type.clone();
                        } else if else_returns {
                            // Safe: else_returns is only true when else_block is Some
                            block_return_type = else_block
                                .as_ref()
                                .expect("else_block must exist when else_returns is true")
                                .return_type
                                .clone();
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

    /// Infer types for a block while checking that explicit `return` statements
    /// match the expected return type `expected`.
    ///
    /// This is used for computed state bodies where the declared type must be
    /// consistent with every `return` in the body.  When `expected` is `None`
    /// the check is skipped (behaves identically to `infer_block`).
    fn infer_block_with_expected_return(
        &mut self,
        block: &ResolvedHirBlock,
        expected: Option<&ConcreteType>,
        context_name: &str,
    ) -> Result<TastBlock, CompilerError> {
        let tast_block = self.infer_block(block)?;

        // Validate that every explicit return inside the block is compatible with
        // the expected type.  We only need to check the top-level return_type
        // recorded on the block itself because `infer_block` propagates the last
        // return upward.
        if let Some(expected_ty) = expected {
            let actual = &tast_block.return_type;
            // Undefined means the block has no return statement at all — skip check
            // so the rest of the pipeline proceeds.
            // STATE003: Computed state return type must match declared type.
            // spec/semantic-rules.md STATE003: "The last expression (or explicit return)
            // in a computed state body must be assignable to the computed variable's
            // declared type." This is a compile-time error.
            if *actual != ConcreteType::Undefined
                && *actual != ConcreteType::Unknown
                && *actual != ConcreteType::Any
                && !actual.is_assignable_to(expected_ty)
            {
                self.errors.push(CompilerError::Validation {
                    context: Box::new(
                        crate::error::ErrorContext::new(
                            format!(
                                "Computed state '{}' declares type {} but body returns {}",
                                context_name, expected_ty, actual
                            ),
                            Some(format!(
                                "Change the return expression to match the declared type {}",
                                expected_ty
                            )),
                            crate::error::ErrorType::Validation,
                            Some(block.location.clone()),
                        )
                        .with_severity(crate::error::ErrorSeverity::Error)
                        .with_error_code("STATE003"),
                    ),
                });
            }
        }

        Ok(tast_block)
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
                    // NOTE: null/none is valid as a variable initializer per type-system.md §5
                    // "Null can be assigned to any type (nullable types)" — type-system.md row 121
                    // e.g., `any value = null`, `integer x = null`, `string s = null` are all valid.
                    // The ConcreteType::Null produced by null literals is assignable to every type.

                    // Special handling for empty literals with explicit type annotations
                    // Check if this is an empty array [] or empty pairs {} literal
                    let is_empty_literal = match init_expr {
                        // Empty array literal []
                        ResolvedHirExpression::Array { elements, .. } => elements.is_empty(),
                        // Empty pairs literal {}
                        ResolvedHirExpression::Literal {
                            value: crate::ast::Value::Pairs(pairs),
                            ..
                        } => pairs.is_empty(),
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

                        // Add constraint that initializer type matches declared type.
                        // SEM004: If a declared type annotation is provably incompatible
                        // with the inferred type (e.g. integer x = "hello"), emit SEM004.
                        // Unknown/Any/Undefined types are excluded because they participate
                        // in error-recovery and late-binding paths.
                        tracing::debug!(
                            "DEBUG CONSTRAINT: VariableDecl '{}' at line {} - left={:?}, right={:?}",
                            name, location.line, tast_init.expr_type, declared_type
                        );
                        let init_ty = &tast_init.expr_type;
                        let declared_ty = &declared_type;
                        let types_are_concrete = !matches!(
                            init_ty,
                            ConcreteType::Unknown
                                | ConcreteType::Undefined
                                | ConcreteType::Any
                                | ConcreteType::Generic { .. }
                        ) && !matches!(
                            declared_ty,
                            ConcreteType::Unknown
                                | ConcreteType::Undefined
                                | ConcreteType::Any
                                | ConcreteType::Generic { .. }
                                | ConcreteType::Optional(_)
                        );
                        // For class types, also allow subclass → parent assignment (polymorphism)
                        let class_subtype_ok = match (init_ty, declared_ty) {
                            (
                                ConcreteType::Class {
                                    symbol_id: child_id,
                                    ..
                                },
                                ConcreteType::Class {
                                    symbol_id: parent_id,
                                    ..
                                },
                            ) => self.class_is_subclass_of(*child_id, *parent_id),
                            _ => false,
                        };
                        if types_are_concrete
                            && !init_ty.is_assignable_to(declared_ty)
                            && !class_subtype_ok
                        {
                            self.errors.push(CompilerError::Validation {
                                context: Box::new(
                                    crate::error::ErrorContext::new(
                                        format!(
                                            "Type annotation '{}' contradicts the inferred type '{}' for variable '{}'",
                                            declared_ty, init_ty, name
                                        ),
                                        Some(format!(
                                            "Change the type annotation to '{}' or change the initializer to produce a '{}'",
                                            init_ty, declared_ty
                                        )),
                                        crate::error::ErrorType::Validation,
                                        Some(location.clone()),
                                    )
                                    .with_error_code("SEM004"),
                                ),
                            });
                        } else {
                            self.add_constraint(TypeConstraint::Equality {
                                left: tast_init.expr_type.clone(),
                                right: declared_type.clone(),
                                location: location.clone(),
                            });
                        }

                        tast_init
                    };

                    Some(tast_init)
                } else {
                    None
                };

                // Optional propagation (spec Step 4):
                // If the initializer comes from a function that returns Optional(T), the
                // variable must also be Optional(T) so that guarded-use enforcement works.
                // We only upgrade when the declared type is NOT already Optional.
                let effective_type = if let Some(ref init_expr) = tast_initializer {
                    match (&init_expr.expr_type, &declared_type) {
                        // Initializer is Optional(T) and declaration is a plain T — promote
                        (ConcreteType::Optional(_), non_optional)
                            if !matches!(non_optional, ConcreteType::Optional(_)) =>
                        {
                            init_expr.expr_type.clone()
                        }
                        _ => declared_type.clone(),
                    }
                } else {
                    declared_type
                };

                // Add variable to type environment
                self.type_env.insert(*symbol_id, effective_type.clone());

                Ok(TastStatement::VariableDeclaration {
                    symbol_id: *symbol_id,
                    name: name.clone(),
                    var_type: effective_type,
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

                // Determine result type - if both branches exist, find common type.
                // If either branch is void (Null/Undefined — bare `return` or no-value block),
                // the if-statement itself doesn't produce a value. Guard against
                // find_common_type(Null, Integer) = Integer (due to Null.is_assignable_to(Integer))
                // incorrectly typing a void function as returning Integer.
                let result_type = if let Some(else_tast) = &tast_else_block {
                    if matches!(
                        tast_then_block.return_type,
                        ConcreteType::Null | ConcreteType::Undefined
                    ) || matches!(
                        else_tast.return_type,
                        ConcreteType::Null | ConcreteType::Undefined
                    ) {
                        ConcreteType::Null
                    } else {
                        self.find_common_type(&tast_then_block.return_type, &else_tast.return_type)
                    }
                } else {
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
                    ResolvedHirExpression::Literal {
                        value: crate::ast::Value::Pairs(pairs),
                        ..
                    } => pairs.is_empty(),
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
                        // STATE004: Assignment to a computed state variable is a compile error.
                        // spec/semantic-rules.md STATE004: "Computed state is read-only. Any
                        // assignment statement whose left-hand side is a computed state variable
                        // is rejected at compile time."
                        if let Some(symbol) = self.symbol_table.get_symbol(*symbol_id) {
                            if let SymbolKind::StateVariable {
                                is_computed: true, ..
                            } = &symbol.kind
                            {
                                self.errors.push(CompilerError::Validation {
                                    context: Box::new(
                                        crate::error::ErrorContext::new(
                                            format!(
                                                "Cannot assign to computed state variable '{}': it is read-only",
                                                name
                                            ),
                                            Some(
                                                "Computed state variables are derived values; they cannot be assigned directly".to_string(),
                                            ),
                                            crate::error::ErrorType::Validation,
                                            Some(var_location.clone()),
                                        )
                                        .with_severity(crate::error::ErrorSeverity::Error)
                                        .with_error_code("STATE004"),
                                    ),
                                });
                            }
                        }

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
            }

            ResolvedHirStatement::Background {
                expression,
                location,
            } => {
                let tast_expression = self.infer_expression(expression)?;
                Ok(TastStatement::Background {
                    expression: tast_expression,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::Break { location } => Ok(TastStatement::Break {
                location: location.clone(),
            }),

            ResolvedHirStatement::Continue { location } => Ok(TastStatement::Continue {
                location: location.clone(),
            }),

            ResolvedHirStatement::Require {
                condition,
                location,
            } => {
                let tast_condition = self.infer_expression(condition)?;

                // Verify the condition is boolean
                if tast_condition.expr_type != ConcreteType::Boolean {
                    return Err(CompilerError::type_error(
                        format!(
                            "require condition must be boolean, found {:?}",
                            tast_condition.expr_type
                        ),
                        None,
                        Some(location.clone()),
                    ));
                }

                Ok(TastStatement::Require {
                    condition: tast_condition,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::Ensure {
                condition,
                location,
            } => {
                // `result` in an ensure condition is a synthetic variable whose SymbolId
                // was injected by the resolver but whose type was never added to `type_env`
                // (because the MIR builder, not the type checker, introduces it at runtime).
                //
                // Before type-checking the condition we scan for any `Variable { name: "result" }`
                // nodes and populate their entries in `type_env` using the enclosing function's
                // known return type.  This lets the type checker resolve `result > 0` correctly
                // without treating `result` as Unknown.
                if let Some(result_type) = self.current_return_type.clone() {
                    self.inject_result_symbol_type(condition, &result_type);
                }

                let tast_condition = self.infer_expression(condition)?;

                if tast_condition.expr_type != ConcreteType::Boolean
                    && tast_condition.expr_type != ConcreteType::Unknown
                {
                    return Err(CompilerError::type_error(
                        format!(
                            "ensure condition must be boolean, found {:?}",
                            tast_condition.expr_type
                        ),
                        None,
                        Some(location.clone()),
                    ));
                }

                Ok(TastStatement::Ensure {
                    condition: tast_condition,
                    location: location.clone(),
                })
            }
        };
        self.recursion_depth -= 1;
        result
    }

    /// Walk a resolved expression and, for every `Variable { name: "result" }` node found,
    /// inject the given type into `type_env`.  Used to pre-populate the synthetic `result`
    /// variable that the MIR builder introduces for `ensure` postcondition checking.
    fn inject_result_symbol_type(
        &mut self,
        expression: &ResolvedHirExpression,
        result_type: &ConcreteType,
    ) {
        match expression {
            ResolvedHirExpression::Variable {
                name, symbol_id, ..
            } if name == "result" => {
                self.type_env.insert(*symbol_id, result_type.clone());
            }
            ResolvedHirExpression::BinaryOp { left, right, .. } => {
                self.inject_result_symbol_type(left, result_type);
                self.inject_result_symbol_type(right, result_type);
            }
            ResolvedHirExpression::UnaryOp { operand, .. } => {
                self.inject_result_symbol_type(operand, result_type);
            }
            ResolvedHirExpression::MethodCall {
                receiver,
                arguments,
                ..
            } => {
                self.inject_result_symbol_type(receiver, result_type);
                for arg in arguments {
                    self.inject_result_symbol_type(arg, result_type);
                }
            }
            ResolvedHirExpression::Call { arguments, .. } => {
                for arg in arguments {
                    self.inject_result_symbol_type(arg, result_type);
                }
            }
            ResolvedHirExpression::Index { array, index, .. } => {
                self.inject_result_symbol_type(array, result_type);
                self.inject_result_symbol_type(index, result_type);
            }
            // Other expression kinds (Literal, This, Constructor, Cast, etc.) do not
            // typically contain `result` references in postconditions.
            _ => {}
        }
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
                        format!("Variable {} not found in type environment", name),
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

                // Get the function type and add parameter type constraints.
                // Skip constraint check for namespace symbols — these resolve by name
                // in infer_function_return_type, not by symbol type.
                if let Some(function_type) = self.type_env.get(function_symbol_id).cloned() {
                    if matches!(function_type, ConcreteType::Namespace) {
                        return Err(CompilerError::type_error(
                            format!(
                                "'{}' is a namespace, not a function — use '{}.function_name()' syntax",
                                function, function
                            ),
                            None,
                            Some(location.clone()),
                        ));
                    }
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

                // Use SymbolId(0) for namespace functions (string.*, math.*, req.*, etc.)
                // This ensures MIR builder creates NamedFunction operands for proper symbol resolution
                // Any dotted function name is a namespace function (no hardcoded list needed)
                let is_namespace_function = function.contains('.');

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
                                is_background: false,
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
                            // IDX004: wrong index type on Any — emit warning, not error
                            self.warnings.push(
                                CompilerError::Validation {
                                    context: Box::new(
                                        crate::error::ErrorContext::new(
                                            format!(
                                                "Index type {:?} is unusual for Any: expected string (object access) or integer (array access)",
                                                other
                                            ),
                                            Some("Use data[\"field\"] for object access or data[0] for array access".to_string()),
                                            crate::error::ErrorType::Validation,
                                            Some(location.clone()),
                                        )
                                        .with_severity(crate::error::ErrorSeverity::Warning)
                                        .with_error_code("IDX004"),
                                    ),
                                }
                            );
                            ConcreteType::Any
                        }
                    },
                    // Array requires integer index (IDX001)
                    ConcreteType::Array(element_type) => {
                        if !matches!(tast_index.expr_type, ConcreteType::Integer) {
                            self.warnings.push(CompilerError::Validation {
                                context: Box::new(
                                    crate::error::ErrorContext::new(
                                        format!(
                                            "Array index should be integer, found {}",
                                            tast_index.expr_type
                                        ),
                                        Some(
                                            "Use an integer expression to index into an array"
                                                .to_string(),
                                        ),
                                        crate::error::ErrorType::Validation,
                                        Some(location.clone()),
                                    )
                                    .with_severity(crate::error::ErrorSeverity::Warning)
                                    .with_error_code("IDX001"),
                                ),
                            });
                        }
                        (**element_type).clone()
                    }
                    // Matrix indexing: matrix<T>[i] returns Array<T> (IDX002)
                    ConcreteType::Matrix(element_type) => {
                        if !matches!(tast_index.expr_type, ConcreteType::Integer) {
                            self.warnings.push(CompilerError::Validation {
                                context: Box::new(
                                    crate::error::ErrorContext::new(
                                        format!(
                                            "Matrix index should be integer, found {}",
                                            tast_index.expr_type
                                        ),
                                        Some(
                                            "Use an integer expression to index into a matrix"
                                                .to_string(),
                                        ),
                                        crate::error::ErrorType::Validation,
                                        Some(location.clone()),
                                    )
                                    .with_severity(crate::error::ErrorSeverity::Warning)
                                    .with_error_code("IDX002"),
                                ),
                            });
                        }
                        ConcreteType::Array(Box::new((**element_type).clone()))
                    }
                    // Pairs type supports string key access (IDX003)
                    ConcreteType::Pairs(_, value_type) => {
                        if !matches!(tast_index.expr_type, ConcreteType::String) {
                            self.warnings.push(CompilerError::Validation {
                                context: Box::new(
                                    crate::error::ErrorContext::new(
                                        format!(
                                            "Pairs key should be string, found {}",
                                            tast_index.expr_type
                                        ),
                                        Some("Use pairs[\"key\"] for pairs access".to_string()),
                                        crate::error::ErrorType::Validation,
                                        Some(location.clone()),
                                    )
                                    .with_severity(crate::error::ErrorSeverity::Warning)
                                    .with_error_code("IDX003"),
                                ),
                            });
                        }
                        (**value_type).clone()
                    }
                    other_type => {
                        // IDX004: indexing into a non-indexable type — warn only
                        self.warnings.push(
                            CompilerError::Validation {
                                context: Box::new(
                                    crate::error::ErrorContext::new(
                                        format!("Cannot index into type: {}", other_type),
                                        Some(
                                            "Bracket access is only supported on list, matrix, pairs, or any types"
                                                .to_string(),
                                        ),
                                        crate::error::ErrorType::Validation,
                                        Some(location.clone()),
                                    )
                                    .with_severity(crate::error::ErrorSeverity::Warning)
                                    .with_error_code("IDX004"),
                                ),
                            }
                        );
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

                // SEM010: string.matches() argument must be a known pattern name literal.
                // semantic-rules.md §SEM010
                // Valid patterns per spec: email, url, uuid, slug, numeric, alpha, phone, date
                if method == "matches" && matches!(tast_receiver.expr_type, ConcreteType::String) {
                    const VALID_PATTERNS: &[&str] = &[
                        "email", "url", "uuid", "slug", "numeric", "alpha", "phone", "date",
                    ];
                    if let Some(first_arg) = tast_arguments.first() {
                        let is_valid = match &first_arg.kind {
                            crate::typechecker::tast::TastExpressionKind::Literal {
                                value: crate::typechecker::tast::TastLiteral::String(pattern_name),
                            } => VALID_PATTERNS.contains(&pattern_name.as_str()),
                            _ => false, // non-literal argument also rejected
                        };
                        if !is_valid {
                            let name = match &first_arg.kind {
                                crate::typechecker::tast::TastExpressionKind::Literal {
                                    value: crate::typechecker::tast::TastLiteral::String(s),
                                } => s.clone(),
                                _ => "<expression>".to_string(),
                            };
                            self.errors.push(CompilerError::Validation {
                                context: Box::new(
                                    crate::error::ErrorContext::new(
                                        format!(
                                            "Unknown pattern name '{}'. Valid patterns: email, url, uuid, slug, numeric, alpha, phone, date",
                                            name
                                        ),
                                        None,
                                        crate::error::ErrorType::Validation,
                                        Some(location.clone()),
                                    )
                                    .with_error_code("SEM010"),
                                ),
                            });
                        }
                    } else {
                        // No argument at all — report missing argument
                        self.errors.push(CompilerError::Validation {
                            context: Box::new(
                                crate::error::ErrorContext::new(
                                    "string.matches() requires a pattern name argument (email, url, uuid, slug, numeric, alpha, phone, date)".to_string(),
                                    None,
                                    crate::error::ErrorType::Validation,
                                    Some(location.clone()),
                                )
                                .with_error_code("SEM010"),
                            ),
                        });
                    }
                }

                // Resolve receiver type with current substitutions before method lookup
                let resolved_receiver_type = self.resolve_type(&tast_receiver.expr_type);

                // Use resolved type for method resolution
                let return_type = self.infer_method_return_type(
                    method,
                    &resolved_receiver_type,
                    &tast_arguments,
                )?;

                // NOTE: Resolve method symbol from receiver's class type or primitive type
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

                // SEM005: Private method access check.
                // If the method is private, it may only be called from within the class that
                // owns it.  We know the receiver's class (from resolved_receiver_type) and the
                // current class (from self.current_class).  A violation is when:
                //   - the method symbol is known (not the SymbolId(0) placeholder),
                //   - the method is private,
                //   - and the receiver's class differs from the currently executing class.
                if resolved_method_symbol.0 != 0 {
                    if let Some(method_sym) = self.symbol_table.get_symbol(resolved_method_symbol) {
                        if method_sym.is_private {
                            if let Some(owner) = &method_sym.owner_scope_name {
                                // Determine the receiver's class name via its symbol.
                                let receiver_class_name = match &resolved_receiver_type {
                                    ConcreteType::Class { symbol_id, .. } => self
                                        .symbol_table
                                        .get_symbol(*symbol_id)
                                        .map(|s| s.name.clone()),
                                    _ => None,
                                };
                                // Determine the current execution class name.
                                let current_class_name = self.current_class.and_then(|cid| {
                                    self.symbol_table.get_symbol(cid).map(|s| s.name.clone())
                                });
                                // Violation: receiver class matches owner AND we're not inside that class.
                                let inside_owner = current_class_name
                                    .as_deref()
                                    .map(|cn| cn == owner)
                                    .unwrap_or(false);
                                if !inside_owner {
                                    if let Some(rcn) = &receiver_class_name {
                                        if rcn == owner {
                                            self.errors.push(CompilerError::Validation {
                                                context: Box::new(
                                                    crate::error::ErrorContext::new(
                                                        format!(
                                                            "'{}' is private and cannot be accessed from outside '{}'",
                                                            method, owner
                                                        ),
                                                        None,
                                                        crate::error::ErrorType::Validation,
                                                        Some(location.clone()),
                                                    )
                                                    .with_error_code("SEM005"),
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

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

                // Handle namespace.class.method() calls
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

                // NOTE: Use SymbolId(0) for built-in namespace methods
                // (string.*, math.*, list.*, etc.) so MIR builder creates NamedFunction operands
                // For user-defined static methods, keep the actual method_symbol_id
                let is_builtin_namespace = [
                    "string",
                    "math",
                    "list",
                    "array",
                    "compare",
                    "file",
                    "http",
                    "json",
                    "input",
                    "validator",
                ]
                .iter()
                .any(|ns| {
                    full_class_name.eq(*ns) || full_class_name.starts_with(&format!("{}.", ns))
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

                // NOTE: Resolve the field type AND symbol ID based on the object's actual type
                // This enables inherited field access - we look up the field in the object's class hierarchy
                let (field_type, resolved_field_symbol_id) =
                    self.infer_field_type_and_symbol(&tast_object.expr_type, field)?;

                // SEM005: Private field access check.
                // A private class field may only be read/written from within the class that
                // owns it.  resolved_field_symbol_id (when != SymbolId(0)) is the actual
                // field symbol, which carries is_private and owner_scope_name.
                if resolved_field_symbol_id.0 != 0 {
                    if let Some(field_sym) = self.symbol_table.get_symbol(resolved_field_symbol_id)
                    {
                        if field_sym.is_private {
                            if let Some(owner) = &field_sym.owner_scope_name {
                                let current_class_name = self.current_class.and_then(|cid| {
                                    self.symbol_table.get_symbol(cid).map(|s| s.name.clone())
                                });
                                let inside_owner = current_class_name
                                    .as_deref()
                                    .map(|cn| cn == owner)
                                    .unwrap_or(false);
                                if !inside_owner {
                                    self.errors.push(CompilerError::validation_error(
                                        format!(
                                            "'{}' is private and cannot be accessed from outside '{}'",
                                            field, owner
                                        ),
                                        location.clone(),
                                    ));
                                }
                            }
                        }
                    }
                }

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
                                is_background: false,
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
                                is_background: false,
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
            // `none` literal — typed as Null at the expression level.
            // The function-level return-type analysis in `infer_function` will later wrap
            // the function's declared return type in Optional(T) when it sees a Null return.
            crate::ast::Value::None => (TastLiteral::Null, ConcreteType::Null),
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
        // Optional guarded-use enforcement (spec Step 5):
        // When an Optional(T) value appears in a context that requires T, the compiler must
        // emit an error.  The following operators are "safe" contexts for Optional:
        //   - Equal / NotEqual / Is / IsNot  →  comparison against none  (`x == none`)
        //   - Or                             →  optional fallback         (`x or fallback`)
        //   - NullCoalesce                   →  null coalescing           (`x default fallback`)
        // All other operators (arithmetic, string concatenation, logical AND, ordering)
        // require the operand to be unwrapped first.
        let operator_allows_optional = matches!(
            operator,
            HirBinaryOp::Equal
                | HirBinaryOp::NotEqual
                | HirBinaryOp::Is
                | HirBinaryOp::IsNot
                | HirBinaryOp::Or
                | HirBinaryOp::NullCoalesce
        );

        if !operator_allows_optional {
            // Check left operand
            if let ConcreteType::Optional(inner) = left_type {
                return Err(CompilerError::type_error(
                    format!(
                        "value of type '{}?' might be none — use 'or' for a fallback or \
                         'if' to check before using it",
                        inner
                    ),
                    Some(
                        "Wrap with: value or defaultValue, or guard with: if value == none"
                            .to_string(),
                    ),
                    Some(location.clone()),
                ));
            }
            // Check right operand (only if not a none-literal comparison, which is fine)
            if let ConcreteType::Optional(inner) = right_type {
                return Err(CompilerError::type_error(
                    format!(
                        "value of type '{}?' might be none — use 'or' for a fallback or \
                         'if' to check before using it",
                        inner
                    ),
                    Some(
                        "Wrap with: value or defaultValue, or guard with: if value == none"
                            .to_string(),
                    ),
                    Some(location.clone()),
                ));
            }
        }

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
                // SEM006: Incompatible operand types in binary expression.
                // Emit up-front when both operands are concrete and non-numeric.
                let is_concrete_non_numeric = |t: &ConcreteType| -> bool {
                    matches!(t, ConcreteType::String | ConcreteType::Boolean)
                };
                if is_concrete_non_numeric(left_type) || is_concrete_non_numeric(right_type) {
                    self.errors.push(CompilerError::Validation {
                        context: Box::new(
                            crate::error::ErrorContext::new(
                                format!(
                                    "Incompatible operand types '{}' and '{}' for arithmetic operator",
                                    left_type, right_type
                                ),
                                Some("Arithmetic operators require numeric operands (integer or number)".to_string()),
                                crate::error::ErrorType::Validation,
                                Some(location.clone()),
                            )
                            .with_error_code("SEM006"),
                        ),
                    });
                    return Ok(ConcreteType::Unknown);
                }

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

            HirBinaryOp::And => {
                // Logical AND requires boolean types on both sides
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

            HirBinaryOp::Or => {
                // `or` is overloaded in Clean Language:
                //
                //   1. Optional fallback: `optionalVar or fallback`
                //      — left is Optional(T), right is T → result is T (safe unwrap)
                //
                //   2. Boolean logical OR: `boolA or boolB`
                //      — both sides are boolean → result is boolean
                match (left_type, right_type) {
                    // Optional<T> or T → T  (the safe-handling path)
                    (ConcreteType::Optional(inner), right)
                        if right == inner.as_ref()
                            || right.is_assignable_to(inner)
                            || inner.is_assignable_to(right) =>
                    {
                        Ok((**inner).clone())
                    }

                    // Optional<T> or Optional<T> → T  (two optionals coalesced)
                    (ConcreteType::Optional(inner_l), ConcreteType::Optional(_inner_r)) => {
                        Ok((**inner_l).clone())
                    }

                    // Optional<Unknown> or T → T  (type variable not yet resolved)
                    (ConcreteType::Optional(_), right)
                        if !matches!(right, ConcreteType::Optional(_)) =>
                    {
                        Ok(right.clone())
                    }

                    // boolean or boolean → boolean (plain logical OR)
                    (ConcreteType::Boolean, ConcreteType::Boolean) => Ok(ConcreteType::Boolean),

                    // Fallback: treat both sides as boolean (preserves existing behaviour for
                    // boolean expressions involving Unknown / Generic types)
                    _ => {
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
                }
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
                        | "list_removeLast"
                        | "list.removeLast"
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
                        format!(
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
                        format!(
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
                format!("Cannot call non-function type: {}", function_type),
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
        // Numeric-polymorphic math functions: preserve the argument type so that
        // `integer x = math.abs(integer_val)` does not produce a false SEM004 error.
        // These checks MUST run before the type_env lookup which would return Number.
        if function_name == "math.abs" || function_name == "math_abs" {
            let arg_type = arguments.first().map(|a| self.resolve_type(&a.expr_type));
            return Ok(match arg_type.as_ref() {
                Some(ConcreteType::Integer) => ConcreteType::Integer,
                _ => ConcreteType::Number,
            });
        }
        if function_name == "math.max"
            || function_name == "math_max"
            || function_name == "math.min"
            || function_name == "math_min"
        {
            let both_int = arguments
                .iter()
                .all(|a| matches!(self.resolve_type(&a.expr_type), ConcreteType::Integer));
            return Ok(if both_int {
                ConcreteType::Integer
            } else {
                ConcreteType::Number
            });
        }

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
            || function_name == "list_removeLast"
            || function_name == "list.removeLast"
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
            // NOTE: Handle SymbolId(0) for stdlib namespace functions
            // SymbolId(0) is a placeholder used for stdlib functions like string.length, math.max
            // These are registered in MirCodeGenerator, not in the symbol table
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
                format!(
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
            (ConcreteType::String, "size") => Ok(ConcreteType::Integer),
            (ConcreteType::String, "toString") => Ok(ConcreteType::String),
            (ConcreteType::String, "toUpperCase") => Ok(ConcreteType::String),
            (ConcreteType::String, "toLowerCase") => Ok(ConcreteType::String),
            (ConcreteType::String, "trim") => Ok(ConcreteType::String),
            (ConcreteType::String, "trimStart") => Ok(ConcreteType::String),
            (ConcreteType::String, "trimEnd") => Ok(ConcreteType::String),
            (ConcreteType::String, "substring") => Ok(ConcreteType::String),
            (ConcreteType::String, "replace") => Ok(ConcreteType::String),
            (ConcreteType::String, "replaceAll") => Ok(ConcreteType::String),
            (ConcreteType::String, "charAt") => Ok(ConcreteType::String),
            (ConcreteType::String, "concat") => Ok(ConcreteType::String),
            (ConcreteType::String, "padStart") => Ok(ConcreteType::String),
            (ConcreteType::String, "padEnd") => Ok(ConcreteType::String),
            (ConcreteType::String, "indexOf") => Ok(ConcreteType::Integer),
            (ConcreteType::String, "lastIndexOf") => Ok(ConcreteType::Integer),
            (ConcreteType::String, "charCodeAt") => Ok(ConcreteType::Integer),
            (ConcreteType::String, "toInteger") => Ok(ConcreteType::Integer),
            (ConcreteType::String, "toNumber") => Ok(ConcreteType::Number),
            (ConcreteType::String, "toBoolean") => Ok(ConcreteType::Boolean),
            (ConcreteType::String, "contains")
            | (ConcreteType::String, "startsWith")
            | (ConcreteType::String, "endsWith")
            | (ConcreteType::String, "isEmpty")
            | (ConcreteType::String, "isBlank")
            | (ConcreteType::String, "isNotEmpty") => Ok(ConcreteType::Boolean),
            (ConcreteType::String, "split") => {
                Ok(ConcreteType::Array(Box::new(ConcreteType::String)))
            }

            // Pairs methods
            (ConcreteType::Pairs(_, value_type), "get") => Ok((**value_type).clone()),
            (ConcreteType::Pairs(_, _), "has") => Ok(ConcreteType::Boolean),
            (ConcreteType::Pairs(_, _), "len" | "size") => Ok(ConcreteType::Integer),
            (ConcreteType::Pairs(_, _), "set" | "remove") => Ok(ConcreteType::Undefined),

            // Array methods
            (ConcreteType::Array(_), "length") => Ok(ConcreteType::Integer),
            (ConcreteType::Array(_), "size") => Ok(ConcreteType::Integer), // Alias for length
            (ConcreteType::Array(_), "push") => Ok(ConcreteType::Undefined), // void return
            (ConcreteType::Array(element_type), "pop") => Ok((**element_type).clone()),
            (ConcreteType::Array(element_type), "removeLast") => Ok((**element_type).clone()), // canonical name for pop
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
            // abs is numeric-polymorphic: integer in → integer out, number in → number out.
            ("math", "abs") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                let arg_type = arguments.first().map(|a| &a.expr_type);
                Ok(match arg_type {
                    Some(ConcreteType::Integer) => ConcreteType::Integer,
                    _ => ConcreteType::Number,
                })
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
                let both_int = arguments
                    .iter()
                    .all(|a| matches!(a.expr_type, ConcreteType::Integer));
                Ok(if both_int {
                    ConcreteType::Integer
                } else {
                    ConcreteType::Number
                })
            }
            ("math", "min") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                let both_int = arguments
                    .iter()
                    .all(|a| matches!(a.expr_type, ConcreteType::Integer));
                Ok(if both_int {
                    ConcreteType::Integer
                } else {
                    ConcreteType::Number
                })
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

            // list static methods - NOTE: Add return types for list namespace functions
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
            ("list", "pop") | ("list", "shift") | ("list", "removeLast") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                // Returns T - the element type of the list
                // list.pop(list<T>) -> T, list.removeLast(list<T>) -> T
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
            ("json", "encode") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::String)
            }
            ("json", "decode") => {
                validate_arg_count(1, arg_count, &full_method_name)?;
                Ok(ConcreteType::Any)
            }
            ("json", "get") => {
                validate_arg_count(2, arg_count, &full_method_name)?;
                Ok(ConcreteType::Any)
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

            // NOTE: For class types, look up the field in the class definition
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
                            format!(
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
                    format!(
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
                format!(
                    "Type {:?} does not have field '{}'",
                    object_type, field_name
                ),
                None,
                None,
            )),
        }
    }

    #[allow(dead_code)] // Convenience wrapper — callers currently use infer_field_type_and_symbol directly
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

            // NOTE: For class types, look up the field in the class definition
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
                            format!(
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
                    format!(
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

    /// Check whether `child_id` is the same as or a subclass (transitively) of `parent_id`.
    fn class_is_subclass_of(&self, child_id: SymbolId, parent_id: SymbolId) -> bool {
        if child_id == parent_id {
            return true;
        }
        let mut current = child_id;
        let mut visited = std::collections::HashSet::new();
        loop {
            if !visited.insert(current) {
                break;
            }
            if let Some(sym) = self.symbol_table.get_symbol(current) {
                if let crate::resolver::symbol_table::SymbolKind::Class {
                    parent: Some(p), ..
                } = &sym.kind
                {
                    if *p == parent_id {
                        return true;
                    }
                    current = *p;
                    continue;
                }
            }
            break;
        }
        false
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

    /// Gap 1 helper: Determine whether a TAST block unconditionally terminates
    /// with a `return` statement on every reachable path.
    ///
    /// The analysis is intentionally conservative (may produce false-positive
    /// warnings) rather than false-negatives that would mask bugs:
    ///   - A block definitively returns if its last statement is `Return`.
    ///   - A block definitively returns if its last statement is an `If` that
    ///     has an `else` branch and both branches definitively return.
    ///   - All other cases are considered *not* definitively returning.
    ///
    /// This keeps the implementation simple and avoids rejecting valid code.
    /// Returns true if the block contains any `return none` path (a return statement
    /// where the value's type is ConcreteType::Null coming from a `none` literal).
    /// Recurses into if/else branches and try/catch blocks.
    fn block_has_none_return(block: &TastBlock) -> bool {
        for stmt in &block.statements {
            match stmt {
                TastStatement::Return {
                    value, return_type, ..
                } => {
                    // A return is a "none return" when the value is a Null literal
                    // (i.e., the source was `return none`) OR return_type is Null and value is None.
                    let is_none_return = match value {
                        Some(expr) => matches!(
                            expr.kind,
                            TastExpressionKind::Literal {
                                value: TastLiteral::Null
                            }
                        ),
                        None => false,
                    };
                    // Also accept explicit return_type == Null with a literal null value
                    let is_null_typed_return = *return_type == ConcreteType::Null
                        && value
                            .as_ref()
                            .map(|e| {
                                matches!(
                                    e.kind,
                                    TastExpressionKind::Literal {
                                        value: TastLiteral::Null
                                    }
                                )
                            })
                            .unwrap_or(false);

                    if is_none_return || is_null_typed_return {
                        return true;
                    }
                }
                TastStatement::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    if Self::block_has_none_return(then_block) {
                        return true;
                    }
                    if let Some(else_b) = else_block {
                        if Self::block_has_none_return(else_b) {
                            return true;
                        }
                    }
                }
                TastStatement::Try {
                    body,
                    catch_clause,
                    finally_clause,
                    ..
                } => {
                    if Self::block_has_none_return(body) {
                        return true;
                    }
                    if let Some(catch) = catch_clause {
                        if Self::block_has_none_return(&catch.body) {
                            return true;
                        }
                    }
                    if let Some(finally) = finally_clause {
                        if Self::block_has_none_return(finally) {
                            return true;
                        }
                    }
                }
                TastStatement::While { body, .. } | TastStatement::For { body, .. }
                    if Self::block_has_none_return(body) =>
                {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Returns true if the block contains any non-none return (i.e., a return whose
    /// value is NOT a Null literal).
    fn block_has_non_none_return(block: &TastBlock) -> bool {
        for stmt in &block.statements {
            match stmt {
                TastStatement::Return { value, .. } => {
                    let is_none_return = match value {
                        Some(expr) => matches!(
                            expr.kind,
                            TastExpressionKind::Literal {
                                value: TastLiteral::Null
                            }
                        ),
                        None => false,
                    };
                    if !is_none_return {
                        return true;
                    }
                }
                TastStatement::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    if Self::block_has_non_none_return(then_block) {
                        return true;
                    }
                    if let Some(else_b) = else_block {
                        if Self::block_has_non_none_return(else_b) {
                            return true;
                        }
                    }
                }
                TastStatement::Try {
                    body,
                    catch_clause,
                    finally_clause,
                    ..
                } => {
                    if Self::block_has_non_none_return(body) {
                        return true;
                    }
                    if let Some(catch) = catch_clause {
                        if Self::block_has_non_none_return(&catch.body) {
                            return true;
                        }
                    }
                    if let Some(finally) = finally_clause {
                        if Self::block_has_non_none_return(finally) {
                            return true;
                        }
                    }
                }
                TastStatement::While { body, .. } | TastStatement::For { body, .. }
                    if Self::block_has_non_none_return(body) =>
                {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn block_definitely_returns(block: &TastBlock) -> bool {
        match block.statements.last() {
            None => false,
            Some(TastStatement::Return { .. }) => true,
            Some(TastStatement::If {
                then_block,
                else_block: Some(else_b),
                ..
            }) => {
                // Both branches must unconditionally return; if there is no else
                // branch the function may fall through after the if.
                Self::block_definitely_returns(then_block) && Self::block_definitely_returns(else_b)
            }
            Some(TastStatement::If {
                else_block: None, ..
            }) => false,
            Some(TastStatement::Try {
                body, catch_clause, ..
            }) => {
                // Conservatively: both try body and catch clause must return.
                let catch_returns = catch_clause
                    .as_ref()
                    .map(|c| Self::block_definitely_returns(&c.body))
                    .unwrap_or(false);
                Self::block_definitely_returns(body) && catch_returns
            }
            _ => false,
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
                is_background: false,
            },
        );

        let mut array_methods = HashMap::new();
        array_methods.insert("length".to_string(), ConcreteType::Integer);
        array_methods.insert("size".to_string(), ConcreteType::Integer);
        array_methods.insert(
            "push".to_string(),
            ConcreteType::Function {
                parameters: vec![ConcreteType::Generic {
                    name: "T".to_string(),
                    bounds: vec![],
                }],
                return_type: Box::new(ConcreteType::Integer),
                is_background: false,
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

// Extension trait to get location from HIR statements — available for error reporting
#[allow(dead_code)] // Not yet used; kept for future structured diagnostics
trait StatementLocation {
    fn location(&self) -> &SourceLocation;
}

#[allow(dead_code)] // Impl for dead trait — retained alongside the trait above
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
            ResolvedHirStatement::Break { location } => location,
            ResolvedHirStatement::Continue { location } => location,
            ResolvedHirStatement::Print { location, .. } => location,
            ResolvedHirStatement::LaterAssignment { location, .. } => location,
            ResolvedHirStatement::Require { location, .. } => location,
            ResolvedHirStatement::Ensure { location, .. } => location,
            ResolvedHirStatement::Background { location, .. } => location,
        }
    }
}

/// Walk a `TastExpression` tree and return the name of the first I/O call found,
/// or `None` if the expression is pure.
///
/// An I/O call is any `MethodCall` or `FunctionCall` whose resolved name starts
/// with one of: `file.`, `db.`, `http.`, `console.`.
/// This is used by the STATE001 guard purity check.
fn find_io_call_in_expression(expr: &TastExpression) -> Option<String> {
    const IO_PREFIXES: &[&str] = &["file.", "db.", "http.", "console."];

    match &expr.kind {
        TastExpressionKind::MethodCall {
            receiver,
            method_name,
            arguments,
            ..
        } => {
            // Check if receiver is a namespace variable with an I/O prefix name.
            if let TastExpressionKind::Variable { name: ns_name, .. } = &receiver.kind {
                let qualified = format!("{}.{}", ns_name, method_name);
                if IO_PREFIXES.iter().any(|p| qualified.starts_with(p)) {
                    return Some(qualified);
                }
            }
            // Recurse into receiver and arguments.
            if let Some(name) = find_io_call_in_expression(receiver) {
                return Some(name);
            }
            for arg in arguments {
                if let Some(name) = find_io_call_in_expression(arg) {
                    return Some(name);
                }
            }
            None
        }
        TastExpressionKind::FunctionCall {
            function,
            arguments,
            ..
        } => {
            // Check if the function expression resolves to an I/O-prefixed name.
            if let TastExpressionKind::Variable { name, .. } = &function.kind {
                if IO_PREFIXES.iter().any(|p| name.starts_with(p)) {
                    return Some(name.clone());
                }
            }
            if let Some(name) = find_io_call_in_expression(function) {
                return Some(name);
            }
            for arg in arguments {
                if let Some(name) = find_io_call_in_expression(arg) {
                    return Some(name);
                }
            }
            None
        }
        TastExpressionKind::BinaryOperation { left, right, .. } => {
            find_io_call_in_expression(left).or_else(|| find_io_call_in_expression(right))
        }
        TastExpressionKind::UnaryOperation { operand, .. } => find_io_call_in_expression(operand),
        TastExpressionKind::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => find_io_call_in_expression(condition)
            .or_else(|| find_io_call_in_expression(then_expr))
            .or_else(|| find_io_call_in_expression(else_expr)),
        TastExpressionKind::Cast { expression, .. } => find_io_call_in_expression(expression),
        TastExpressionKind::TypeCheck { expression, .. } => find_io_call_in_expression(expression),
        TastExpressionKind::Await { expression } => find_io_call_in_expression(expression),
        TastExpressionKind::OnError {
            expression,
            fallback,
        } => {
            find_io_call_in_expression(expression).or_else(|| find_io_call_in_expression(fallback))
        }
        TastExpressionKind::PropertyAccess { object, .. } => find_io_call_in_expression(object),
        TastExpressionKind::ArrayLiteral { elements, .. } => {
            elements.iter().find_map(find_io_call_in_expression)
        }
        TastExpressionKind::ArrayAccess { array, index } => {
            find_io_call_in_expression(array).or_else(|| find_io_call_in_expression(index))
        }
        TastExpressionKind::StaticMethodCall { arguments, .. } => {
            arguments.iter().find_map(find_io_call_in_expression)
        }
        TastExpressionKind::Range {
            start, end, step, ..
        } => find_io_call_in_expression(start)
            .or_else(|| find_io_call_in_expression(end))
            .or_else(|| step.as_ref().and_then(|s| find_io_call_in_expression(s))),
        // Literals, variables, base calls, lambda, object literal, async block
        // are all pure or do not contain I/O call sites that can be detected here.
        _ => None,
    }
}
