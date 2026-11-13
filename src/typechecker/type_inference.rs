//! Type inference engine for Clean Language
//!
//! Generates type constraints from resolved HIR and performs type inference
//! using constraint-based approach with Hindley-Milner algorithm.

use super::constraint_solver::ConstraintSolver;
use super::tast::{
    BinaryOperator, ConcreteType, TastBlock, TastClass, TastComputedDeclaration, TastExpression,
    TastExpressionKind, TastField, TastFunction, TastGuardClause, TastLiteral, TastObjectField,
    TastParameter, TastProgram, TastStateBlock, TastStateDeclaration, TastStatement,
    TastWatchBlock, TypeConstraint, UnaryOperator, Visibility, LIST_WIDEN_F64, OPTIONAL_BOX,
};
use crate::ast::SourceLocation;
use crate::error::CompilerError;
use crate::hir::{HirBinaryOp, HirType, HirUnaryOp};
use crate::resolver::{
    GlobalSymbolTable, ResolvedHirBlock, ResolvedHirClass, ResolvedHirExpression,
    ResolvedHirFunction, ResolvedHirLValue, ResolvedHirMethod, ResolvedHirParameter,
    ResolvedHirProgram, ResolvedHirStateBlock, ResolvedHirStatement, ResolvedHirWatchBlock,
    SymbolId, SymbolKind, MODULE_OWNER,
};
use std::collections::HashMap;

/// Type inference engine
#[derive(Debug)]
pub struct TypeInference<'a> {
    /// Current type environment mapping symbols to types
    type_env: HashMap<SymbolId, ConcreteType>,

    /// Generated type constraints
    constraints: Vec<TypeConstraint>,

    /// Symbol table from resolution phase
    symbol_table: &'a GlobalSymbolTable,

    /// Map from function SymbolId to minimum required parameter count
    /// (parameters without defaults)
    required_param_counts: HashMap<SymbolId, usize>,

    /// Typed default value of each parameter of a function, method or
    /// constructor (None for a required parameter), keyed by its symbol.
    param_defaults: HashMap<SymbolId, Vec<Option<TastExpression>>>,

    /// The explicit constructor (with parameters) of each class, for
    /// checking `base(...)` calls against the parent's constructor.
    class_constructors: HashMap<SymbolId, SymbolId>,

    /// Capability conformance closure per class, derived from the resolver's
    /// vtable_descriptors. `class_conformance[class_sym]` is the full set of
    /// capabilities that class satisfies (its own declared `can C1, C2, ...`
    /// plus every ancestor's). Consulted by the constraint solver's
    /// Class → Interface unification rule.
    class_conformance: HashMap<SymbolId, std::collections::HashSet<SymbolId>>,

    /// Files of plugin-injected modules; their code may call private
    /// functions of user modules (SEM005).
    plugin_generated_files: std::collections::HashSet<String>,

    /// Current context for inference
    current_function: Option<SymbolId>,
    current_class: Option<SymbolId>,
    current_return_type: Option<ConcreteType>,

    /// Errors encountered during inference
    errors: Vec<CompilerError>,
    warnings: Vec<CompilerError>,

    /// Recursion depth counter to prevent stack overflow
    recursion_depth: usize,

    /// Location of the statement being inferred. Some expressions (operators,
    /// literals) carry no position of their own; their diagnostics point at
    /// the statement instead of `:0:0`.
    statement_location: Option<SourceLocation>,
}

/// A built-in method's signature (see `TypeInference::builtin_method_signature`).
struct BuiltinMethod {
    params: Vec<ConcreteType>,
    /// The first `required` parameters must be passed; the rest are optional.
    required: usize,
    return_type: ConcreteType,
}

/// Result of type inference
#[derive(Debug)]
pub struct InferenceResult {
    pub tast: TastProgram,
    pub errors: Vec<CompilerError>,
    pub warnings: Vec<CompilerError>,
}

impl<'a> TypeInference<'a> {
    /// Create a new type inference engine
    pub fn new(symbol_table: &'a GlobalSymbolTable) -> Self {
        Self {
            type_env: HashMap::new(),
            constraints: Vec::new(),
            symbol_table,
            required_param_counts: HashMap::new(),
            param_defaults: HashMap::new(),
            class_constructors: HashMap::new(),
            class_conformance: HashMap::new(),
            plugin_generated_files: std::collections::HashSet::new(),
            current_function: None,
            current_class: None,
            current_return_type: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            recursion_depth: 0,
            statement_location: None,
        }
    }

    /// Pre-seed required parameter counts for functions that are known to have
    /// optional parameters (e.g. language alias bridge functions with param_defaults).
    pub fn seed_required_param_counts(&mut self, counts: HashMap<SymbolId, usize>) {
        for (id, count) in counts {
            self.required_param_counts.insert(id, count);
        }
    }

    /// Name the files of plugin-injected modules (see `plugin_generated_files`).
    pub fn set_plugin_generated_files(&mut self, files: std::collections::HashSet<String>) {
        self.plugin_generated_files = files;
    }

    /// Perform type inference on a resolved HIR program
    pub fn infer_types(mut self, program: ResolvedHirProgram) -> InferenceResult {
        // Initialize built-in types in type environment
        self.initialize_builtins();

        // Register state variables from symbol table
        self.register_state_variables();

        // Derive per-class capability conformance closure from the resolver's
        // vtable_descriptors. Keys of that map are already (class_sym, cap_sym)
        // pairs where the class transitively conforms to the capability, so
        // one grouping pass gives us the class → set-of-capabilities lookup
        // the constraint solver uses for Class → Interface unification.
        for (class_sym, cap_sym) in program.vtable_descriptors.keys() {
            self.class_conformance
                .entry(*class_sym)
                .or_default()
                .insert(*cap_sym);
        }
        // Infer types for all program elements
        let tast_program = self.infer_program(&program);

        // Solve the generated constraints. The conformance map lets the
        // solver validate Class → Interface (capability) unifications.
        let mut solver = ConstraintSolver::with_symbol_table(self.symbol_table);
        solver.set_class_conformance(std::mem::take(&mut self.class_conformance));
        solver.add_constraints(std::mem::take(&mut self.constraints));
        let solver_result = solver.solve();

        // Collect errors
        self.errors.extend(solver_result.errors);

        // `T?` is checked; from here on it is stored as `T`.
        let mut tast_program = tast_program;
        super::erase_optional::erase_optional(&mut tast_program);

        InferenceResult {
            tast: tast_program,
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

        // Generic registration: Register all builtin functions from symbol table
        // This automatically handles namespace functions and other builtins
        let builtin_symbols: Vec<_> = self.symbol_table.accessible_symbols();
        for symbol_id in builtin_symbols {
            if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                if self.symbol_table.is_builtin(symbol_id) {
                    match &symbol.kind {
                        // Skip if already registered (to preserve manual overrides above)
                        crate::resolver::SymbolKind::Function {
                            parameters,
                            return_type,
                        } if !self.type_env.contains_key(&symbol_id) => {
                            let concrete_params: Vec<ConcreteType> = parameters
                                .iter()
                                .map(|t| self.hir_type_to_concrete(t))
                                .collect();
                            let concrete_return = return_type
                                .as_ref()
                                .map(|t| Box::new(self.hir_type_to_concrete(t)))
                                .unwrap_or_else(|| Box::new(ConcreteType::Null));

                            self.type_env.insert(
                                symbol_id,
                                ConcreteType::Function {
                                    parameters: concrete_params,
                                    return_type: concrete_return,
                                    is_background: false,
                                },
                            );
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
                    let concrete_type = self.hir_type_to_concrete(var_type);
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
            tests: tast_tests,
            state: tast_state,
            watch_blocks: tast_watch_blocks,
            type_env: self.type_env.clone(),
            location: program.location.clone(),
            // NOTE: Pass symbol table through to MIR for dynamic SymbolId resolution
            symbol_table: std::sync::Arc::new(program.symbol_table.clone()),
            externals: tast_externals,
            // Forward the resolver's per-class capability vtable descriptors
            // through to MIR so it can synthesize class-id dispatch tables.
            vtable_descriptors: program.vtable_descriptors.clone(),
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
            is_background: false,
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
        self.register_parameter_defaults(function.symbol_id, &function.parameters);
    }

    /// Type each parameter default once, where it is declared: it is stored
    /// into the parameter like an argument, so `integer a = "x"` is SEM001.
    /// Calls that leave the parameter out pass this value.
    fn register_parameter_defaults(
        &mut self,
        callee: SymbolId,
        parameters: &[ResolvedHirParameter],
    ) {
        if parameters.iter().all(|p| p.default_value.is_none()) {
            return;
        }
        let mut defaults = Vec::with_capacity(parameters.len());
        for param in parameters {
            let Some(default_expr) = &param.default_value else {
                defaults.push(None);
                continue;
            };
            let param_type = self.hir_type_to_concrete(&param.param_type);
            match self.infer_expression(default_expr) {
                Ok(mut value) => {
                    let location = Self::argument_location(&value, &param.location);
                    self.require_assignable(&mut value, &param_type, &location);
                    defaults.push(Some(value));
                }
                Err(error) => {
                    self.errors.push(error);
                    defaults.push(None);
                }
            }
        }
        self.param_defaults.insert(callee, defaults);
    }

    /// The typed default of `callee`'s parameter `index`, if it has one.
    fn parameter_default(&self, callee: SymbolId, index: usize) -> Option<TastExpression> {
        self.param_defaults
            .get(&callee)
            .and_then(|defaults| defaults.get(index).cloned().flatten())
    }

    /// A call that leaves out trailing parameters passes their defaults.
    /// The argument count was checked by `check_call_arguments`.
    fn append_default_arguments(&self, callee: SymbolId, arguments: &mut Vec<TastExpression>) {
        let Some(defaults) = self.param_defaults.get(&callee) else {
            return;
        };
        for default in defaults.iter().skip(arguments.len()) {
            match default {
                Some(value) => arguments.push(value.clone()),
                None => break,
            }
        }
    }

    /// Register method signature in type environment
    fn register_method_signature(&mut self, method: &ResolvedHirMethod) {
        let return_type = self.hir_type_to_concrete(&method.return_type);
        self.register_callable_signature(method.symbol_id, &method.parameters, return_type);
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

        // An explicit constructor's signature checks `Class(args)` calls. A
        // parameterless constructor is not checked: entity classes (frame.data
        // `data` blocks) declare one and are built positionally from their
        // fields, `Widget(1, "x")`.
        if let Some(constructor) = class
            .constructor
            .as_ref()
            .filter(|c| !c.parameters.is_empty())
        {
            self.class_constructors
                .insert(class.symbol_id, constructor.symbol_id);
            self.register_callable_signature(
                constructor.symbol_id,
                &constructor.parameters,
                ConcreteType::Class {
                    symbol_id: class.symbol_id,
                    type_args: Vec::new(),
                },
            );
        }
    }

    /// Record the parameter types and the number of required (no default)
    /// parameters of a method or constructor, keyed by its symbol.
    fn register_callable_signature(
        &mut self,
        symbol_id: SymbolId,
        parameters: &[ResolvedHirParameter],
        return_type: ConcreteType,
    ) {
        let function_type = ConcreteType::Function {
            parameters: parameters
                .iter()
                .map(|p| self.hir_type_to_concrete(&p.param_type))
                .collect(),
            return_type: Box::new(return_type),
            is_background: false,
        };
        self.type_env.insert(symbol_id, function_type);
        let required = parameters
            .iter()
            .filter(|p| p.default_value.is_none())
            .count();
        self.required_param_counts.insert(symbol_id, required);
        self.register_parameter_defaults(symbol_id, parameters);
    }

    /// FUNC002 / SEM001: check the arguments of a constructor or method call
    /// against the callee's registered signature. Callees without a
    /// registered signature (built-ins, implicit constructors) are skipped.
    fn check_call_arguments(
        &mut self,
        callee: SymbolId,
        callee_name: &str,
        arguments: &mut [TastExpression],
        location: &SourceLocation,
    ) {
        let Some(ConcreteType::Function { parameters, .. }) = self.type_env.get(&callee).cloned()
        else {
            return;
        };
        let required = self
            .required_param_counts
            .get(&callee)
            .copied()
            .unwrap_or(parameters.len());
        if arguments.len() < required || arguments.len() > parameters.len() {
            let expected = if required == parameters.len() {
                required.to_string()
            } else {
                format!("{} to {}", required, parameters.len())
            };
            self.errors.push(CompilerError::semantic_error_with_code(
                format!(
                    "'{}' expects {} argument(s), got {}",
                    callee_name,
                    expected,
                    arguments.len()
                ),
                None,
                Some(location.clone()),
                "FUNC002",
            ));
            return;
        }
        for (param_type, arg) in parameters.iter().zip(arguments.iter_mut()) {
            let location = Self::argument_location(arg, location);
            self.require_assignable(arg, param_type, &location);
        }
        self.check_sized_arguments(callee, callee_name, arguments, location);
    }

    /// SEM001: `validator.run` / `validator.validate` look each field up by
    /// name in a `pairs` (or JSON object) input (stdlib-reference §15). A
    /// class instance has no named entries, so every field would be absent.
    fn check_validator_input(
        &mut self,
        callee_name: &str,
        arguments: &[TastExpression],
        location: &SourceLocation,
    ) {
        if !matches!(callee_name, "validator.run" | "validator.validate") {
            return;
        }
        let Some(input) = arguments.get(1) else {
            return;
        };
        if matches!(
            input.expr_type,
            ConcreteType::Class { .. } | ConcreteType::Interface { .. }
        ) {
            self.errors.push(CompilerError::semantic_error_with_code(
                format!(
                    "{} checks a pairs value, not a {} instance",
                    callee_name,
                    input.expr_type.source_name(Some(self.symbol_table))
                ),
                Some(
                    "Pass a pairs<string, any> whose keys are the field names, \
                     e.g. {\"name\": person.name}"
                        .to_string(),
                ),
                Some(location.clone()),
                "SEM001",
            ));
        }
    }

    /// SEM004: sized arguments follow the same widening rules as an
    /// assignment to the declared parameter (see `check_sized_conversion`).
    fn check_sized_arguments(
        &mut self,
        callee: SymbolId,
        callee_name: &str,
        arguments: &[TastExpression],
        location: &SourceLocation,
    ) {
        let declared = match self.symbol_table.get_symbol(callee).map(|s| &s.kind) {
            Some(
                SymbolKind::Function { parameters, .. }
                | SymbolKind::Method { parameters, .. }
                | SymbolKind::Constructor { parameters, .. },
            ) => parameters.clone(),
            _ => return,
        };
        for (index, (param_type, arg)) in declared.iter().zip(arguments).enumerate() {
            let arg_location = Self::argument_location(arg, location);
            self.check_sized_conversion(
                param_type,
                Self::value_source_symbol(arg),
                &format!("argument {} of '{}'", index + 1, callee_name),
                &arg_location,
            );
        }
    }

    /// Where to report a problem with a call argument: the argument itself,
    /// or the call when the argument carries no position (literals do not).
    fn argument_location(arg: &TastExpression, call: &SourceLocation) -> SourceLocation {
        if arg.location.line == 0 {
            call.clone()
        } else {
            arg.location.clone()
        }
    }

    /// `value` is stored where a `target` is expected (assignment, argument,
    /// return): widen a literal to the target type and require the value's
    /// type to be assignable to it (directional, type-system.md §5).
    fn require_assignable(
        &mut self,
        value: &mut TastExpression,
        target: &ConcreteType,
        location: &SourceLocation,
    ) {
        self.fit_to_expected_type(value, target);
        Self::widen_literal_to_declared_type(value, target);
        Self::widen_integer_list_to_number_list(value, target);
        // The elements of a list literal and the values of a pairs literal
        // are stored into the target's element slots: `["a", none]` is not a
        // `list<string>` (type-system.md §8), and each element stored into
        // an optional primitive slot is boxed.
        match (&mut value.kind, target) {
            (
                TastExpressionKind::ArrayLiteral {
                    elements,
                    element_type,
                },
                ConcreteType::Array(element),
            ) => {
                let element = (**element).clone();
                let boxes = element.optional_primitive().is_some();
                for item in elements.iter_mut() {
                    if item.expr_type == ConcreteType::Null || boxes {
                        let location = item.location.clone();
                        self.require_assignable(item, &element, &location);
                    }
                }
                if boxes {
                    *element_type = element;
                    value.expr_type = target.clone();
                }
            }
            (TastExpressionKind::ObjectLiteral { fields }, ConcreteType::Pairs(_, value_type)) => {
                let value_type = (**value_type).clone();
                let boxes = value_type.optional_primitive().is_some();
                for field in fields.iter_mut() {
                    if field.value.expr_type == ConcreteType::Null || boxes {
                        let location = field.value.location.clone();
                        self.require_assignable(&mut field.value, &value_type, &location);
                    }
                }
                if boxes {
                    value.expr_type = target.clone();
                }
            }
            _ => {}
        }
        // The value's own type is checked against the target, not the type
        // of the box it is wrapped in.
        let value_type = value.expr_type.clone();
        Self::box_into_optional_primitive(value, target);
        // The result of a call to a function that returns nothing (`void`)
        // is none: say so rather than "none cannot be used as ...".
        if value.expr_type == ConcreteType::Null && !ConstraintSolver::accepts_none(target) {
            let callee = match &value.kind {
                TastExpressionKind::FunctionCall { function, .. } => match &function.kind {
                    TastExpressionKind::Variable { name, .. } => Some(name.clone()),
                    _ => Some("the function".to_string()),
                },
                TastExpressionKind::MethodCall { method_name, .. }
                | TastExpressionKind::StaticMethodCall { method_name, .. } => {
                    Some(method_name.clone())
                }
                _ => None,
            };
            if let Some(callee) = callee {
                self.errors.push(CompilerError::type_error(
                    format!(
                        "`{}` returns nothing (void), so its result cannot be used as {}",
                        callee,
                        target.source_name(Some(self.symbol_table))
                    ),
                    Some(format!(
                        "Give `{}` a return type and return a value, or call it as a statement",
                        callee
                    )),
                    Some(location.clone()),
                ));
                return;
            }
        }
        self.add_constraint(TypeConstraint::Subtype {
            subtype: value_type,
            supertype: target.clone(),
            location: location.clone(),
        });
    }

    /// The condition of an `if` or `while` is a boolean. A `T?` is not one,
    /// even a `boolean?`: it may be none (type-system.md §8).
    fn require_condition(
        &mut self,
        keyword: &str,
        condition: &TastExpression,
        location: &SourceLocation,
    ) {
        if let ConcreteType::Optional(inner) = &condition.expr_type {
            let name = inner.source_name(Some(self.symbol_table));
            let help = if **inner == ConcreteType::Boolean {
                "Use `value!` when it cannot be none, or `value default false`".to_string()
            } else {
                "Compare with none to get a boolean, e.g. `value != none`".to_string()
            };
            self.errors.push(CompilerError::type_error(
                format!(
                    "The `{}` condition must be a boolean, not a {}? (it may be none)",
                    keyword, name
                ),
                Some(help),
                Some(location.clone()),
            ));
            return;
        }
        if condition.expr_type != ConcreteType::Boolean {
            self.add_constraint(TypeConstraint::Equality {
                left: condition.expr_type.clone(),
                right: ConcreteType::Boolean,
                location: location.clone(),
            });
        }
    }

    /// An empty `[]` or `{}` stored where a `target` is expected takes the
    /// target's type, so `list<integer> l = []` allocates an empty list of
    /// integers. `[]` is only a list (or matrix) and `{}` only a pairs
    /// (SEM001). Returns None when `expr` is not an empty literal, or is an
    /// empty object literal `{}`, which is inferred as written.
    fn empty_collection_literal(
        &mut self,
        expr: &ResolvedHirExpression,
        target: &ConcreteType,
        location: &SourceLocation,
    ) -> Option<TastExpression> {
        let (is_list, typed_here) = match expr {
            ResolvedHirExpression::Array { elements, .. } if elements.is_empty() => (true, true),
            ResolvedHirExpression::Literal {
                value: crate::ast::Value::List(elements),
                ..
            } if elements.is_empty() => (true, true),
            ResolvedHirExpression::ObjectLiteral { fields, .. } if fields.is_empty() => {
                (false, false)
            }
            _ => return None,
        };
        fn holds(target: &ConcreteType, is_list: bool) -> bool {
            match target {
                ConcreteType::Unknown | ConcreteType::Any | ConcreteType::Generic { .. } => true,
                ConcreteType::Optional(inner) => holds(inner, is_list),
                ConcreteType::Array(_) | ConcreteType::Matrix(_) => is_list,
                ConcreteType::Pairs(_, _) => !is_list,
                _ => false,
            }
        }
        if !holds(target, is_list) {
            let (literal, kind) = if is_list {
                ("[]", "list")
            } else {
                ("{}", "pairs")
            };
            self.errors.push(CompilerError::type_error(
                format!(
                    "An empty {} `{}` cannot be used as {}",
                    kind,
                    literal,
                    target.source_name(Some(self.symbol_table))
                ),
                Some(if is_list {
                    "`[]` is an empty list; `{}` is an empty pairs".to_string()
                } else {
                    "`{}` is an empty pairs; `[]` is an empty list".to_string()
                }),
                Some(self.located(location)),
            ));
        }
        if !typed_here {
            return None;
        }
        let element_type = match target {
            ConcreteType::Array(element) => (**element).clone(),
            _ => ConcreteType::Unknown,
        };
        Some(TastExpression {
            kind: TastExpressionKind::ArrayLiteral {
                elements: vec![],
                element_type,
            },
            expr_type: target.clone(),
            location: expr.location().clone(),
        })
    }

    /// A `T?` may be none, so its members are only reachable through
    /// `value!` (or a `default` fallback): reading a field, calling a method
    /// or indexing it directly is an error (04-type-system.md, Nullable
    /// Types).
    fn reject_optional_receiver(
        &self,
        receiver_type: &ConcreteType,
        what: &str,
        location: &SourceLocation,
    ) -> Result<(), CompilerError> {
        if let ConcreteType::Optional(inner) = receiver_type {
            let name = inner.source_name(Some(self.symbol_table));
            return Err(CompilerError::type_error(
                format!(
                    "A value of type {}? may be none, so {} needs it unwrapped",
                    name, what
                ),
                Some(format!(
                    "Write `value!.…` when it cannot be none, or check `value == none` first and then use `value!`; `value default fallback` gives a {}",
                    name
                )),
                Some(self.located(location)),
            ));
        }
        Ok(())
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
        for (index, param) in function.parameters.iter().enumerate() {
            let param_type = self.hir_type_to_concrete(&param.param_type);
            self.type_env.insert(param.symbol_id, param_type.clone());

            tast_parameters.push(TastParameter {
                symbol_id: param.symbol_id,
                name: param.name.clone(),
                param_type,
                default_value: self.parameter_default(function.symbol_id, index),
                is_variadic: false,
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
            // Explicit return type declared: every `return` was checked
            // against it while the body was inferred.
            self.hir_type_to_concrete(return_type)
        } else {
            // No return type declared - use inferred type
            // This allows functions to have their return type inferred from their body
            // Update current return type for any remaining processing
            self.current_return_type = Some(inferred_return_type.clone());
            inferred_return_type.clone()
        };

        if function.return_type.is_some() {
            self.check_missing_return(
                "Function",
                &function.name,
                &declared_return_type,
                &tast_body,
                &function.location,
            );
        }

        // A function without a declared return type that returns none on
        // some paths and a value on others returns a `T?`. A declared return
        // type is the function's type: `return none` from a plain `T` was
        // reported where it stands (type-system.md §8).
        let final_return_type = if function.return_type.is_none()
            && Self::block_has_none_return(&tast_body)
            && Self::block_has_non_none_return(&tast_body)
        {
            match &declared_return_type {
                ConcreteType::Optional(_) => declared_return_type.clone(),
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
            is_background: false,
            is_static: false, // Top-level functions are not static methods
            visibility: self.visibility_of(function.symbol_id),
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
        for (index, param) in constructor.parameters.iter().enumerate() {
            let param_type = self.hir_type_to_concrete(&param.param_type);
            self.type_env.insert(param.symbol_id, param_type.clone());

            tast_parameters.push(TastParameter {
                symbol_id: param.symbol_id,
                name: param.name.clone(),
                param_type,
                default_value: self.parameter_default(constructor.symbol_id, index),
                is_variadic: false,
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
        has_capabilities: bool,
    ) -> Result<TastFunction, CompilerError> {
        self.current_function = Some(method.symbol_id);
        self.current_return_type = Some(self.hir_type_to_concrete(&method.return_type));

        // Add parameters to type environment
        let mut tast_parameters = Vec::new();
        for (index, param) in method.parameters.iter().enumerate() {
            let param_type = self.hir_type_to_concrete(&param.param_type);
            self.type_env.insert(param.symbol_id, param_type.clone());

            tast_parameters.push(TastParameter {
                symbol_id: param.symbol_id,
                name: param.name.clone(),
                param_type,
                default_value: self.parameter_default(method.symbol_id, index),
                is_variadic: false,
                location: param.location.clone(),
            });
        }

        // Infer body
        let tast_body = self.infer_block(&method.body)?;

        let declared_return_type = self.hir_type_to_concrete(&method.return_type);
        self.check_missing_return(
            "Method",
            &method.name,
            &declared_return_type,
            &tast_body,
            &method.location,
        );

        // Determine if method should be static based on class context.
        // For classes with inheritance OR instance fields OR declared capabilities:
        //   Always use instance methods.
        // - Inheritance: method signatures must match across the hierarchy.
        // - Fields: there is state for `this` to point at.
        // - Capabilities: `CallCapability` dispatch always passes a receiver as
        //   the first argument, so any method that might participate in a
        //   capability vtable slot must accept `this`. Marking such a method
        //   static drops the `this` parameter from the WASM signature; the
        //   dispatcher then leaves `this` unconsumed on the stack, breaking
        //   the `if (Result T)` block's stack invariant (COM001 fingerprints
        //   93df7710de8f / 40d63810 / f9b020dc / 8d4f74b1, 2026-07-19).
        //
        // For utility classes with no state and no capability role:
        //   Methods are static. This preserves the earlier fix for the
        //   body_uses_this() false-positive on sibling calls
        //   (CODEGEN_STACK_REMAINING fp df5b8c9b1021).
        let is_static = !(has_parent || has_fields || has_capabilities);

        Ok(TastFunction {
            symbol_id: method.symbol_id,
            name: method.name.clone(),
            parameters: tast_parameters,
            return_type: declared_return_type,
            body: tast_body,
            generic_params: Vec::new(), // Would handle generics here
            constraints: Vec::new(),
            is_background: false, // Methods are typically not async
            is_static,
            visibility: self.visibility_of(method.symbol_id),
            location: method.location.clone(),
        })
    }

    /// A declaration outside a `public:` sub-section is private.
    fn visibility_of(&self, symbol_id: SymbolId) -> Visibility {
        match self.symbol_table.get_symbol(symbol_id) {
            Some(symbol) if symbol.is_private => Visibility::Private,
            _ => Visibility::Public,
        }
    }

    /// FUNC004: a non-void function or method must return a value on every
    /// path (semantic-rules.md §FUNC004, an error).
    fn check_missing_return(
        &mut self,
        kind: &str,
        name: &str,
        declared_return_type: &ConcreteType,
        body: &TastBlock,
        location: &SourceLocation,
    ) {
        let is_void_return = matches!(
            declared_return_type,
            ConcreteType::Null | ConcreteType::Undefined
        );
        if is_void_return || Self::body_always_returns(body) {
            return;
        }
        self.errors.push(CompilerError::Validation {
            context: Box::new(
                crate::error::ErrorContext::new(
                    format!("{kind} '{name}' does not return a value on every path"),
                    Some(
                        "End every path with a return statement (or a final expression), or raise an error with error(...)"
                            .to_string(),
                    ),
                    crate::error::ErrorType::Validation,
                    Some(location.clone()),
                )
                .with_error_code("FUNC004"),
            ),
        });
    }

    /// Infer types for a class
    fn infer_class(&mut self, class: &ResolvedHirClass) -> Result<TastClass, CompilerError> {
        self.current_class = Some(class.symbol_id);

        // Convert fields
        let mut tast_fields = Vec::new();
        for field in &class.fields {
            let field_type = self.hir_type_to_concrete(&field.field_type);

            let default_value = if let Some(init_expr) = &field.initializer {
                if let Some(empty) =
                    self.empty_collection_literal(init_expr, &field_type, &field.location)
                {
                    Some(empty)
                } else {
                    // The initializer is stored into the field like an
                    // assignment: none only into a `T?` field
                    // (type-system.md §8).
                    let mut value = self.infer_expression(init_expr)?;
                    let location = field.location.clone();
                    self.require_assignable(&mut value, &field_type, &location);
                    Some(value)
                }
            } else {
                None
            };

            tast_fields.push(TastField {
                symbol_id: field.symbol_id,
                name: field.name.clone(),
                field_type,
                default_value,
                is_static: false, // ResolvedHirField doesn't have is_static field
                visibility: self.visibility_of(field.symbol_id),
                location: field.location.clone(),
            });
        }

        // Convert constructors — errors propagate like a method's do.
        let mut tast_constructors = Vec::new();
        if let Some(constructor) = &class.constructor {
            tast_constructors.push(self.infer_constructor(constructor, class.symbol_id)?);
        }

        // Convert methods — propagate errors so codegen never sees a missing method body.
        let mut tast_methods = Vec::new();
        // A class with subclasses passes `this` like its subclasses do: a
        // call bound to its method may run an override (`CallVirtual`).
        let in_hierarchy = class.parent.is_some()
            || self.symbol_table.all_symbols().values().any(|symbol| {
                matches!(symbol.kind, SymbolKind::Class { parent: Some(p), .. } if p == class.symbol_id)
            });
        let has_fields = !class.fields.is_empty();
        let has_capabilities = !class.capabilities.is_empty();
        for method in &class.methods {
            match self.infer_method(method, in_hierarchy, has_fields, has_capabilities) {
                Ok(tast_method) => tast_methods.push(tast_method),
                Err(e) => {
                    tracing::warn!(
                        class = %class.name,
                        method = %method.name,
                        error = ?e,
                        "Method type inference failed — propagating error"
                    );
                    return Err(e);
                }
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
            // Classes carry no visibility; their members do.
            visibility: Visibility::Public,
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

            // none only initializes a `T?` state variable (type-system.md §8).
            if initializer.expr_type == ConcreteType::Null
                && !ConstraintSolver::accepts_none(&state_type)
            {
                return Err(ConstraintSolver::none_into_plain(
                    &state_type.source_name(Some(self.symbol_table)),
                    &decl.location,
                ));
            }

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

        Ok(TastStateBlock {
            declarations: tast_declarations,
            computed: tast_computed,
            rules: tast_rules,
            location: state_block.location.clone(),
        })
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
        let saved_statement_location = self.statement_location.clone();
        if let Some(location) = Self::statement_location_of(statement) {
            if location.line != 0 {
                self.statement_location = Some(location.clone());
            }
        }
        let result = self.infer_statement_kind(statement);
        self.statement_location = saved_statement_location;
        self.recursion_depth -= 1;
        result
    }

    /// The source location of a statement.
    fn statement_location_of(statement: &ResolvedHirStatement) -> Option<&SourceLocation> {
        match statement {
            ResolvedHirStatement::VariableDeclaration { location, .. }
            | ResolvedHirStatement::Assignment { location, .. }
            | ResolvedHirStatement::Expression { location, .. }
            | ResolvedHirStatement::Return { location, .. }
            | ResolvedHirStatement::If { location, .. }
            | ResolvedHirStatement::For { location, .. }
            | ResolvedHirStatement::While { location, .. }
            | ResolvedHirStatement::Break { location }
            | ResolvedHirStatement::Continue { location }
            | ResolvedHirStatement::Print { location, .. }
            | ResolvedHirStatement::LaterAssignment { location, .. }
            | ResolvedHirStatement::Background { location, .. }
            | ResolvedHirStatement::Require { location, .. }
            | ResolvedHirStatement::Ensure { location, .. } => Some(location),
        }
    }

    /// `location`, or the enclosing statement's location when `location`
    /// carries no position.
    fn located(&self, location: &SourceLocation) -> SourceLocation {
        match &self.statement_location {
            Some(statement) if location.line == 0 => statement.clone(),
            _ => location.clone(),
        }
    }

    /// Infer one statement (see `infer_statement`).
    fn infer_statement_kind(
        &mut self,
        statement: &ResolvedHirStatement,
    ) -> Result<TastStatement, CompilerError> {
        match statement {
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
                if let Some(ResolvedHirExpression::Variable {
                    symbol_id: source, ..
                }) = initializer
                {
                    self.check_sized_conversion(
                        var_type,
                        Some(*source),
                        &format!("variable '{}'", name),
                        location,
                    );
                }

                let tast_initializer = if let Some(init_expr) = initializer {
                    // none (alias null) initializes only a `T?` or an `any`
                    // (type-system.md §8); `require_assignable` below reports
                    // `string s = none`.
                    let tast_init = if let Some(empty) =
                        self.empty_collection_literal(init_expr, &declared_type, location)
                    {
                        empty
                    } else {
                        // Normal inference for non-empty literals
                        let mut tast_init = self.infer_expression(init_expr)?;

                        // Widen literal type to match a sized integer destination so the MIR
                        // builder emits the correct constant width. Without this, an
                        // `integer:64 x = 9223372036854775807` literal keeps the typechecker's
                        // default `ConcreteType::Integer` tag and codegen narrows it to i32 in
                        // `load_constant`, truncating the high bits.
                        Self::widen_literal_to_declared_type(&mut tast_init, &declared_type);
                        self.fit_to_expected_type(&mut tast_init, &declared_type);

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
                        // A `T?` initializer is reported by the assignability
                        // check below (it may be none), not as SEM004.
                        let types_are_concrete = !matches!(
                            init_ty,
                            ConcreteType::Unknown
                                | ConcreteType::Undefined
                                | ConcreteType::Any
                                | ConcreteType::Generic { .. }
                                | ConcreteType::Optional(_)
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
                            // A class value in a capability-typed variable: the
                            // constraint solver checks conformance (SEM011).
                            (ConcreteType::Class { .. }, ConcreteType::Interface { .. }) => true,
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
                            self.require_assignable(&mut tast_init, &declared_type, location);
                        }

                        tast_init
                    };

                    Some(tast_init)
                } else {
                    None
                };

                // The variable has its declared type: a `T?` initializer
                // stored into a plain `T` was reported above.
                let effective_type = declared_type;

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
                // An empty `[]` / `{}` takes the declared return type, as in a
                // declaration.
                let empty_literal = match (value, self.current_return_type.clone()) {
                    (Some(_), Some(ConcreteType::Null | ConcreteType::Undefined)) => None,
                    (Some(return_expr), Some(expected)) => {
                        self.empty_collection_literal(return_expr, &expected, location)
                    }
                    _ => None,
                };
                let return_type = if let Some(empty) = empty_literal {
                    Some(empty)
                } else if let Some(return_expr) = value {
                    let mut tast_expr = self.infer_expression(return_expr)?;

                    // The returned value must be assignable to the declared
                    // return type; a `void` method returns no value.
                    match self.current_return_type.clone() {
                        Some(ConcreteType::Null | ConcreteType::Undefined)
                            if !matches!(
                                tast_expr.expr_type,
                                ConcreteType::Null
                                    | ConcreteType::Undefined
                                    | ConcreteType::Unknown
                            ) =>
                        {
                            self.errors.push(CompilerError::type_error(
                                format!(
                                    "A void function cannot return a value (got {})",
                                    tast_expr.expr_type.source_name(Some(self.symbol_table))
                                ),
                                Some(
                                    "Declare the return type, or write `return` without a value"
                                        .to_string(),
                                ),
                                Some(location.clone()),
                            ));
                        }
                        Some(expected_return_type) => {
                            self.require_assignable(
                                &mut tast_expr,
                                &expected_return_type,
                                location,
                            );
                        }
                        None => {}
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
                self.require_condition("if", &tast_condition, location);

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

                // `iterate x in items` walks a list (a range is a list of
                // integers); the loop variable has the element type. A
                // string is walked one character at a time, each a
                // one-character `string` (control-flow spec, `iterate ch in
                // "Clean"`). An `any` is not type-checked (type-system.md
                // §9): it is walked as the `list<any>` a JSON array is
                // (stdlib-reference.md §8), and a value that is not a list
                // raises at run time.
                let element_type = match &tast_iterable.expr_type {
                    ConcreteType::Array(element_type) => (**element_type).clone(),
                    ConcreteType::String => ConcreteType::String,
                    ConcreteType::Any => ConcreteType::Any,
                    // Already reported where the type became unknown.
                    ConcreteType::Unknown => ConcreteType::Unknown,
                    other => {
                        return Err(CompilerError::semantic_error_with_code(
                            format!("Cannot iterate over a value of type {}", other),
                            Some(
                                "`iterate` walks the elements of a list, the characters of a string or a range (`iterate i in 1 to 10`)"
                                    .to_string(),
                            ),
                            Some(tast_iterable.location.clone()),
                            "SEM004",
                        ));
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
                self.require_condition("while", &tast_condition, location);

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
                    // The element type is determined (and the index checked)
                    // with the target expressions below.
                    ResolvedHirLValue::Index { .. } => ConcreteType::Unknown,
                };

                // An empty `[]` / `{}` takes the target's type when it is known.
                let empty_literal = if matches!(target_type, ConcreteType::Unknown) {
                    None
                } else {
                    self.empty_collection_literal(value, &target_type, location)
                };
                let is_empty_literal = empty_literal.is_some();
                let mut tast_value = match empty_literal {
                    Some(empty) => empty,
                    None => self.infer_expression(value)?,
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

                        self.check_private_state_access(*symbol_id, name, var_location);

                        // Only add constraint if not an empty literal (already handled)
                        if !is_empty_literal {
                            self.require_assignable(&mut tast_value, &target_type, location);
                        }
                        if let Some(declared) = self.declared_hir_type(*symbol_id) {
                            self.check_sized_conversion(
                                &declared,
                                Self::value_source_symbol(&tast_value),
                                &format!("variable '{}'", name),
                                location,
                            );
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

                        // SEM004: a list has no assignable properties. Its behavior
                        // (`.line`, `.pile`, `.unique`) is part of the type, declared
                        // with the variable (type-system: "Behavior Declaration").
                        if let ConcreteType::Array(element_type) = &tast_object.expr_type {
                            return Err(CompilerError::Type {
                                context: Box::new(
                                    crate::error::ErrorContext::new(
                                        format!("Cannot assign to '{}' on a list", field),
                                        Some(format!(
                                            "List behavior is part of the type: declare the list as `list<{}>.unique name = []` (or `.line`, `.pile`)",
                                            element_type
                                        )),
                                        crate::error::ErrorType::Type,
                                        Some(field_location.clone()),
                                    )
                                    .with_error_code("SEM004"),
                                ),
                            });
                        }

                        // The resolver leaves `obj.field` l-values unbound
                        // (SymbolId(0)) because the field depends on the
                        // object's type; bind it here the same way field reads
                        // are bound.
                        let (target_type, field_symbol_id) = if *field_symbol_id == SymbolId(0) {
                            match self.infer_field_type_and_symbol(
                                &tast_object.expr_type,
                                field,
                                field_location,
                            ) {
                                Ok(found) => found,
                                Err(error) => {
                                    self.errors.push(error);
                                    (ConcreteType::Unknown, *field_symbol_id)
                                }
                            }
                        } else {
                            (target_type, *field_symbol_id)
                        };

                        // SEM005: a private field is written only inside its class.
                        if field_symbol_id.0 != 0 {
                            self.check_private_member_access(
                                field_symbol_id,
                                field,
                                None,
                                field_location,
                            );
                        }

                        // Only add constraint if not an empty literal (already handled)
                        if !is_empty_literal {
                            self.require_assignable(&mut tast_value, &target_type, location);
                        }
                        if let Some(declared) = self.declared_hir_type(field_symbol_id) {
                            self.check_sized_conversion(
                                &declared,
                                Self::value_source_symbol(&tast_value),
                                &format!("field '{}'", field),
                                location,
                            );
                        }

                        TastExpression {
                            kind: TastExpressionKind::PropertyAccess {
                                object: Box::new(tast_object),
                                property_name: field.clone(),
                                property_symbol: field_symbol_id,
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
                        // `list[i] = v`, `pairs[key] = v`: the same container and
                        // index checks as a read, and the value must be
                        // assignable to the element type.
                        let tast_array = self.infer_expression(array)?;
                        let tast_index = self.infer_expression(index)?;
                        let element_type = self.index_element_type(
                            &tast_array.expr_type,
                            &tast_index.expr_type,
                            index_location,
                        );
                        self.require_assignable(&mut tast_value, &element_type, location);

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
        }
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
                self.infer_literal_expression(value, location)?
            }
            ResolvedHirExpression::Variable {
                symbol_id,
                name,
                location,
            } => self.infer_variable_expression(symbol_id, name, location)?,
            ResolvedHirExpression::BinaryOp {
                left,
                op,
                right,
                location,
            } => self.infer_binary_op_expression(left, op, right, location)?,
            ResolvedHirExpression::Call {
                function,
                function_symbol_id,
                arguments,
                location,
            } => self.infer_call_expression(function, function_symbol_id, arguments, location)?,
            ResolvedHirExpression::Index {
                array,
                index,
                location,
            } => self.infer_index_expression(array, index, location)?,
            ResolvedHirExpression::MethodCall {
                receiver,
                method,
                method_symbol_id,
                arguments,
                location,
            } => self.infer_method_call_expression(
                receiver,
                method,
                method_symbol_id,
                arguments,
                location,
            )?,
            ResolvedHirExpression::StaticMethodCall {
                namespace,
                class_name,
                class_symbol_id: _,
                method,
                method_symbol_id,
                arguments,
                location,
            } => self.infer_static_method_call_expression(
                namespace,
                class_name,
                method,
                method_symbol_id,
                arguments,
                location,
            )?,
            ResolvedHirExpression::FieldAccess {
                object,
                field,
                field_symbol_id: _hir_field_symbol_id,
                location,
            } => self.infer_field_access_expression(object, field, location)?,
            ResolvedHirExpression::Array {
                elements,
                element_type,
                location,
            } => self.infer_array_expression(elements, element_type, location)?,
            ResolvedHirExpression::UnaryOp {
                op,
                operand,
                location,
            } => self.infer_unary_op_expression(op, operand, location)?,
            ResolvedHirExpression::Constructor {
                class_name,
                class_symbol_id,
                constructor_symbol_id,
                arguments,
                location,
            } => self.infer_constructor_expression(
                class_name,
                class_symbol_id,
                constructor_symbol_id,
                arguments,
                location,
            )?,
            ResolvedHirExpression::This {
                class_symbol_id,
                location,
            } => self.infer_this_expression(class_symbol_id, location)?,
            ResolvedHirExpression::Cast {
                expression,
                target_type,
                location,
            } => self.infer_cast_expression(expression, target_type, location)?,
            ResolvedHirExpression::OnError {
                expression,
                fallback,
                location,
            } => self.infer_on_error_expression(expression, fallback, location)?,
            ResolvedHirExpression::Conditional {
                condition,
                then_expr,
                else_expr,
                location,
            } => self.infer_conditional_expression(condition, then_expr, else_expr, location)?,
            ResolvedHirExpression::BaseCall {
                parent_class_symbol_id,
                arguments,
                location,
            } => self.infer_base_call_expression(parent_class_symbol_id, arguments, location)?,
            ResolvedHirExpression::Range {
                start,
                end,
                step,
                inclusive,
                location,
            } => self.infer_range_expression(start, end, step, inclusive, location)?,
            ResolvedHirExpression::ObjectLiteral { fields, location } => {
                self.infer_object_literal_expression(fields, location)?
            }
        };

        self.recursion_depth -= 1;
        Ok(TastExpression {
            kind,
            expr_type,
            location: self.located(&location),
        })
    }

    // -------------------------------------------------------------------------
    // Per-expression-kind inference helpers
    // -------------------------------------------------------------------------

    /// Helper: infer a `Literal` expression.
    fn infer_literal_expression(
        &mut self,
        value: &crate::ast::Value,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        // List literals embedded inside an object literal (or any other literal
        // context) must produce `ArrayLiteral` with each element preserved as
        // its own TastExpression. The same `infer_literal_expression` recursion
        // handles arbitrarily-nested lists and lists-of-objects.
        if let crate::ast::Value::List(elements) = value {
            let mut tast_elements = Vec::with_capacity(elements.len());
            let mut elem_type = ConcreteType::Unknown;
            for elem in elements {
                let (kind, ty, loc) = self.infer_literal_expression(elem, location)?;
                if matches!(elem_type, ConcreteType::Unknown) {
                    elem_type = ty.clone();
                }
                tast_elements.push(TastExpression {
                    kind,
                    expr_type: ty,
                    location: loc,
                });
            }
            return Ok((
                TastExpressionKind::ArrayLiteral {
                    elements: tast_elements,
                    element_type: elem_type.clone(),
                },
                ConcreteType::Array(Box::new(elem_type)),
                location.clone(),
            ));
        }
        let (tast_literal, literal_type) = self.infer_literal(value);
        Ok((
            TastExpressionKind::Literal {
                value: tast_literal,
            },
            literal_type,
            location.clone(),
        ))
    }

    /// Helper: infer an `ObjectLiteral` expression — `{ key: expr, ... }`.
    /// Mirrors the `Value::Pairs` branch in `infer_literal_expression` but
    /// recurses on `ResolvedHirExpression` for each field value (so variables,
    /// calls, binary ops are inferred and pass through to MIR as proper
    /// expressions instead of being stringified).
    fn infer_object_literal_expression(
        &mut self,
        fields: &[(crate::ast::Value, ResolvedHirExpression)],
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let mut tast_fields = Vec::with_capacity(fields.len());
        for (key_val, value_expr) in fields {
            let key = match key_val {
                crate::ast::Value::String(s) => s.clone(),
                other => format!("{}", other),
            };
            let value_tast = self.infer_expression(value_expr)?;
            tast_fields.push(TastObjectField {
                key,
                value: value_tast,
                location: location.clone(),
            });
        }
        Ok((
            TastExpressionKind::ObjectLiteral {
                fields: tast_fields,
            },
            ConcreteType::Any,
            location.clone(),
        ))
    }

    /// Helper: infer a `Variable` expression.
    fn infer_variable_expression(
        &mut self,
        symbol_id: &SymbolId,
        name: &str,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        self.check_private_state_access(*symbol_id, name, location);
        let var_type = self.type_env.get(symbol_id).cloned().unwrap_or_else(|| {
            self.errors.push(CompilerError::type_error(
                format!("Variable {} not found in type environment", name),
                None,
                Some(location.clone()),
            ));
            ConcreteType::Unknown
        });

        Ok((
            TastExpressionKind::Variable {
                symbol_id: *symbol_id,
                name: name.to_string(),
            },
            var_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `BinaryOp` expression.
    fn infer_binary_op_expression(
        &mut self,
        left: &ResolvedHirExpression,
        op: &HirBinaryOp,
        right: &ResolvedHirExpression,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let tast_left = self.infer_expression(left)?;
        let tast_right = self.infer_expression(right)?;

        let resolved_left_type = tast_left.expr_type.clone();
        let resolved_right_type = tast_right.expr_type.clone();

        let result_type =
            self.infer_binary_operation(op, &resolved_left_type, &resolved_right_type, location)?;

        Ok((
            TastExpressionKind::BinaryOperation {
                operator: self.convert_binary_operator(op),
                left: Box::new(tast_left),
                right: Box::new(tast_right),
            },
            result_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `Call` (free function call) expression.
    fn infer_call_expression(
        &mut self,
        function: &str,
        function_symbol_id: &SymbolId,
        arguments: &[ResolvedHirExpression],
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let mut tast_arguments = Vec::new();
        for arg in arguments {
            tast_arguments.push(self.infer_expression(arg)?);
        }

        let is_builtin =
            function_symbol_id.0 == 0 || self.symbol_table.is_builtin(*function_symbol_id);
        if let Some(list_fn) = function.strip_prefix("list.").filter(|_| is_builtin) {
            if !matches!(list_fn, "concat" | "range" | "fill" | "join") {
                return Err(Self::list_namespace_error(list_fn, location));
            }
        }
        if let Some(list_fn) = function
            .strip_prefix("list.")
            .or_else(|| function.strip_prefix("list_"))
            .filter(|_| is_builtin)
        {
            self.check_list_namespace_arguments(list_fn, &mut tast_arguments, location);
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
                &mut tast_arguments,
                *function_symbol_id,
                location,
            )?;
        }

        // SEM005: a private function or method is called only from its owner.
        if function_symbol_id.0 != 0 && !is_builtin {
            self.check_private_member_access(*function_symbol_id, function, None, location);
        }

        // Look up function type and determine return type
        let return_type = self.infer_function_return_type(
            *function_symbol_id,
            function,
            &tast_arguments,
            location,
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

        Ok((
            TastExpressionKind::FunctionCall {
                function: Box::new(TastExpression {
                    kind: TastExpressionKind::Variable {
                        symbol_id: resolved_symbol_id,
                        name: function.to_string(),
                    },
                    expr_type: ConcreteType::Function {
                        parameters: tast_arguments.iter().map(|a| a.expr_type.clone()).collect(),
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
        ))
    }

    /// Helper: infer an `Index` (bracket access) expression.
    fn infer_index_expression(
        &mut self,
        array: &ResolvedHirExpression,
        index: &ResolvedHirExpression,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let tast_array = self.infer_expression(array)?;
        let tast_index = self.infer_expression(index)?;
        self.reject_optional_receiver(&tast_array.expr_type, "indexing it", location)?;

        let element_type =
            self.index_element_type(&tast_array.expr_type, &tast_index.expr_type, location);

        Ok((
            TastExpressionKind::ArrayAccess {
                array: Box::new(tast_array),
                index: Box::new(tast_index),
            },
            element_type,
            location.clone(),
        ))
    }

    /// The element type of `container[index]` (list, matrix, pairs or any),
    /// reporting IDX001–IDX004 when the container cannot be indexed or the
    /// index has the wrong type. Shared by reads and bracket assignments.
    fn index_element_type(
        &mut self,
        container_type: &ConcreteType,
        index_type: &ConcreteType,
        location: &SourceLocation,
    ) -> ConcreteType {
        // IDX001–IDX004 are errors (error-codes.md) when both sides have a
        // definite type; when either side is still unresolved (Unknown, Any,
        // generic, union, optional, sized integer) the mismatch cannot be
        // proven and is reported as a warning instead.
        let index_is_definite = Self::is_definite_index_type(index_type);
        match container_type {
            // Any type supports both string (object access) and integer (array access)
            ConcreteType::Any => {
                if !matches!(index_type, ConcreteType::String | ConcreteType::Integer) {
                    self.report_index_diagnostic(
                        false,
                        "IDX004",
                        format!(
                            "Index type {} is unusual for Any: expected string (object access) or integer (array access)",
                            index_type
                        ),
                        "Use data[\"field\"] for object access or data[0] for array access",
                        location,
                    );
                }
                ConcreteType::Any
            }
            // Array requires integer index (IDX001)
            ConcreteType::Array(element_type) => {
                if !matches!(index_type, ConcreteType::Integer) {
                    self.report_index_diagnostic(
                        index_is_definite,
                        "IDX001",
                        format!("List index must be integer, found {}", index_type),
                        "Use an integer expression to index into a list",
                        location,
                    );
                }
                (**element_type).clone()
            }
            // Matrix indexing: matrix<T>[i] returns Array<T> (IDX002)
            ConcreteType::Matrix(element_type) => {
                if !matches!(index_type, ConcreteType::Integer) {
                    self.report_index_diagnostic(
                        index_is_definite,
                        "IDX002",
                        format!("Matrix index must be integer, found {}", index_type),
                        "Use an integer expression to index into a matrix",
                        location,
                    );
                }
                ConcreteType::Array(Box::new((**element_type).clone()))
            }
            // Pairs bracket access uses the declared key type (IDX003)
            ConcreteType::Pairs(key_type, value_type) => {
                if !index_type.is_assignable_to(key_type) {
                    self.report_index_diagnostic(
                        index_is_definite && Self::is_definite_index_type(key_type),
                        "IDX003",
                        format!("Pairs key must be {}, found {}", key_type, index_type),
                        "Use a key of the type the pairs declares, e.g. pairs[\"key\"] for pairs<string, V>",
                        location,
                    );
                }
                (**value_type).clone()
            }
            other_type => {
                // IDX004: indexing into a non-indexable type
                let receiver_is_definite = Self::is_definite_index_type(other_type);
                self.report_index_diagnostic(
                    receiver_is_definite,
                    "IDX004",
                    format!("Cannot index into type: {}", other_type),
                    "Bracket access is only supported on list, matrix, pairs, or any types",
                    location,
                );
                ConcreteType::Unknown
            }
        }
    }

    /// True when `t` is a fully-known type, so an index-type mismatch
    /// involving it is certain rather than an artefact of unfinished inference.
    fn is_definite_index_type(t: &ConcreteType) -> bool {
        matches!(
            t,
            ConcreteType::Integer
                | ConcreteType::Number
                | ConcreteType::NumberSized { .. }
                | ConcreteType::String
                | ConcreteType::Boolean
                | ConcreteType::Array(_)
                | ConcreteType::Matrix(_)
                | ConcreteType::Pairs(_, _)
                | ConcreteType::Class { .. }
                | ConcreteType::Interface { .. }
                | ConcreteType::Function { .. }
        )
    }

    /// A fully-known scalar or string type: two such types that are not
    /// assignable either way can never hold the same value.
    fn is_primitive_value_type(t: &ConcreteType) -> bool {
        matches!(
            t,
            ConcreteType::Integer
                | ConcreteType::Number
                | ConcreteType::IntegerSized { .. }
                | ConcreteType::NumberSized { .. }
                | ConcreteType::String
                | ConcreteType::Boolean
        )
    }

    /// Record an IDX00x diagnostic: an error when the mismatch is certain,
    /// a warning otherwise.
    fn report_index_diagnostic(
        &mut self,
        is_error: bool,
        code: &str,
        message: String,
        help: &str,
        location: &SourceLocation,
    ) {
        let mut context = crate::error::ErrorContext::new(
            message,
            Some(help.to_string()),
            crate::error::ErrorType::Validation,
            Some(self.located(location)),
        )
        .with_error_code(code);
        if is_error {
            self.errors.push(CompilerError::Validation {
                context: Box::new(context),
            });
        } else {
            context = context.with_severity(crate::error::ErrorSeverity::Warning);
            self.warnings.push(CompilerError::Validation {
                context: Box::new(context),
            });
        }
    }

    /// Methods every value answers regardless of its class declaration.
    fn is_universal_method(method: &str) -> bool {
        matches!(method, "toString")
    }

    /// Names of every method declared by `class_id` or one of its ancestors.
    fn class_method_names(&self, class_id: SymbolId) -> Vec<String> {
        use crate::resolver::symbol_table::SymbolKind;
        let mut names = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut current = Some(class_id);
        while let Some(cid) = current {
            if !visited.insert(cid) {
                break;
            }
            current = None;
            if let Some(SymbolKind::Class {
                methods, parent, ..
            }) = self.symbol_table.get_symbol(cid).map(|s| &s.kind)
            {
                for method_id in methods {
                    if let Some(m) = self.symbol_table.get_symbol(*method_id) {
                        if !names.contains(&m.name) {
                            names.push(m.name.clone());
                        }
                    }
                }
                current = *parent;
            }
        }
        names
    }

    /// FUNC001 error for a method that `class_id`'s hierarchy does not declare,
    /// with a did-you-mean suggestion when a similarly named method exists.
    fn unknown_class_method_error(
        &self,
        class_id: SymbolId,
        method: &str,
        location: &SourceLocation,
    ) -> CompilerError {
        let class_name = self
            .symbol_table
            .get_symbol(class_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "<unknown>".to_string());
        let candidates = self.class_method_names(class_id);
        let candidate_refs: Vec<&str> = candidates.iter().map(String::as_str).collect();
        let suggestions =
            crate::error::ErrorUtils::suggest_similar_names(method, &candidate_refs, 3);
        CompilerError::Type {
            context: Box::new(
                crate::error::ErrorContext::new(
                    format!("Method '{}' not found on class '{}'", method, class_name),
                    Some(format!(
                        "Declare `{method}` in the `functions:` block of `{class_name}` or one of its parent classes"
                    )),
                    crate::error::ErrorType::Type,
                    Some(location.clone()),
                )
                .with_suggestions(suggestions)
                .with_error_code("FUNC001"),
            ),
        }
    }

    /// SEM005: a private member may only be used from inside its owner.
    ///
    /// * A private class field or method is used only inside the class that
    ///   declares it. Private members are not inherited: a subclass that
    ///   reads, writes or calls one of its parent's private members is
    ///   outside the owner too.
    /// * A private module function or `state:` variable is used only inside
    ///   the file that declares it. Plugin-generated code (plugin-origin
    ///   locations and plugin-injected modules) and compiler-synthesized code
    ///   may call them.
    ///
    /// `method_receiver` is the receiver type of an instance method call. The
    /// call is checked only when the receiver is the owning class or one of
    /// its subclasses; for any other receiver (a capability, `any`, an
    /// unrelated class) the method symbol is not the receiver's own member.
    fn check_private_member_access(
        &mut self,
        member_symbol: SymbolId,
        member_name: &str,
        method_receiver: Option<&ConcreteType>,
        location: &SourceLocation,
    ) {
        let (owner, owner_class, is_method, declared_in) =
            match self.symbol_table.get_symbol(member_symbol) {
                Some(sym) if sym.is_private => {
                    let Some(owner) = sym.owner_scope_name.clone() else {
                        return;
                    };
                    let (owner_class, is_method) = match &sym.kind {
                        SymbolKind::Method { class_id, .. } => (Some(*class_id), true),
                        SymbolKind::Field { class_id, .. } => (Some(*class_id), false),
                        _ => (None, false),
                    };
                    (owner, owner_class, is_method, sym.location.file.clone())
                }
                _ => return,
            };

        if owner == MODULE_OWNER {
            self.check_private_module_member(member_symbol, member_name, &declared_in, location);
            return;
        }

        let current_class_name = self
            .current_class
            .and_then(|cid| self.symbol_table.get_symbol(cid))
            .map(|s| s.name.clone());
        if current_class_name.as_deref() == Some(owner.as_str()) {
            return;
        }
        if let Some(receiver) = method_receiver {
            let receiver_in_owner_hierarchy = match (receiver, owner_class) {
                (ConcreteType::Class { symbol_id, .. }, Some(owner_id)) => {
                    self.class_is_subclass_of(*symbol_id, owner_id)
                }
                _ => false,
            };
            if !receiver_in_owner_hierarchy {
                return;
            }
        }

        let section = if is_method {
            format!("a `public:` sub-section of `{owner}`'s `functions:` block")
        } else {
            format!("the `public:` sub-section of `{owner}`'s class body")
        };
        let kind = if is_method { "method" } else { "field" };
        let inherited_into = match (self.current_class, owner_class) {
            (Some(current), Some(owner_id)) if self.class_is_subclass_of(current, owner_id) => {
                current_class_name.clone()
            }
            _ => None,
        };
        let help = match inherited_into {
            Some(child) => format!(
                "Private members are not inherited: `{child}` cannot use `{owner}`'s private \
                 {kind} `{member_name}`. Move `{member_name}` under {section}."
            ),
            None if is_method => format!(
                "Class methods are private by default (semantic-rules.md §SEM005). \
                 To call `{member_name}` from outside `{owner}`, move its declaration under \
                 {section}."
            ),
            None => format!(
                "Class fields are private by default (semantic-rules.md §SEM005). \
                 To read or write `{member_name}` from outside `{owner}`, move its declaration \
                 under {section}."
            ),
        };
        self.push_access_violation(member_name, &owner, help, location);
    }

    /// SEM005 for a private `state:` variable named from another file.
    fn check_private_state_access(
        &mut self,
        symbol_id: SymbolId,
        name: &str,
        location: &SourceLocation,
    ) {
        if matches!(
            self.symbol_table.get_symbol(symbol_id).map(|s| &s.kind),
            Some(SymbolKind::StateVariable { .. })
        ) {
            self.check_private_member_access(symbol_id, name, None, location);
        }
    }

    /// SEM005 for a private module function or `state:` variable used from a
    /// file other than the one that declares it.
    fn check_private_module_member(
        &mut self,
        member_symbol: SymbolId,
        member_name: &str,
        declared_in: &str,
        location: &SourceLocation,
    ) {
        let declared_in_source = self
            .symbol_table
            .get_symbol(member_symbol)
            .is_some_and(|s| crate::ast::is_user_source_location(&s.location));
        if !declared_in_source
            || !crate::ast::is_user_source_location(location)
            || self.plugin_generated_files.contains(&location.file)
            || location.file == declared_in
        {
            return;
        }
        let module = std::path::Path::new(declared_in)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| declared_in.to_string());
        let is_state = matches!(
            self.symbol_table.get_symbol(member_symbol).map(|s| &s.kind),
            Some(SymbolKind::StateVariable { .. })
        );
        let help = if is_state {
            format!(
                "State variables are private to the file that declares them \
                 (semantic-rules.md §SEM005). To read or write `{member_name}` from another \
                 file, move its declaration under the `public:` sub-section of the `state:` \
                 block in module `{module}`."
            )
        } else {
            format!(
                "Module functions are private to the file that declares them \
                 (semantic-rules.md §SEM005). To call `{member_name}` from another file, \
                 move its declaration under the `public:` sub-section of the `functions:` \
                 block in module `{module}`."
            )
        };
        self.push_access_violation(member_name, &module, help, location);
    }

    fn push_access_violation(
        &mut self,
        member_name: &str,
        scope: &str,
        help: String,
        location: &SourceLocation,
    ) {
        self.errors.push(CompilerError::Validation {
            context: Box::new(
                crate::error::ErrorContext::new(
                    format!(
                        "'{}' is private and cannot be accessed from outside '{}'",
                        member_name, scope
                    ),
                    Some(help),
                    crate::error::ErrorType::Validation,
                    Some(location.clone()),
                )
                .with_error_code("SEM005"),
            ),
        });
    }

    /// Helper: infer a `MethodCall` (instance method call) expression.
    fn infer_method_call_expression(
        &mut self,
        receiver: &ResolvedHirExpression,
        method: &str,
        method_symbol_id: &Option<SymbolId>,
        arguments: &[ResolvedHirExpression],
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let tast_receiver = self.infer_expression(receiver)?;
        self.reject_optional_receiver(
            &tast_receiver.expr_type,
            &format!("calling its method '{}'", method),
            location,
        )?;

        let mut tast_arguments = Vec::new();
        for arg in arguments {
            tast_arguments.push(self.infer_expression(arg)?);
        }

        // SEM010: string.matches() argument must be a known pattern name literal
        // (semantic-rules.md §SEM010). The table is shared with the MIR builder.
        if method == "matches" && matches!(tast_receiver.expr_type, ConcreteType::String) {
            let message = match tast_arguments.first().map(|a| &a.kind) {
                Some(TastExpressionKind::Literal {
                    value: TastLiteral::String(pattern_name),
                }) => super::tast::string_match_pattern_id(pattern_name)
                    .is_none()
                    .then(|| super::tast::string_match_pattern_error(pattern_name)),
                Some(_) => {
                    Some("string.matches() takes the pattern name as a string literal".to_string())
                }
                None => Some(super::tast::string_match_pattern_error("")),
            };
            if let Some(message) = message {
                self.errors.push(CompilerError::Validation {
                    context: Box::new(
                        crate::error::ErrorContext::new(
                            message,
                            None,
                            crate::error::ErrorType::Validation,
                            Some(location.clone()),
                        )
                        .with_error_code("SEM010"),
                    ),
                });
            }
        }

        let resolved_receiver_type = tast_receiver.expr_type.clone();

        // Use resolved type for method resolution
        let return_type = self.infer_method_return_type(method, &resolved_receiver_type)?;

        // NOTE: Resolve method symbol from receiver's class type or primitive type
        // The resolver sets method_symbol_id = None because it doesn't have type info
        // Now we have the receiver's type, so we can look up the method
        let resolved_method_symbol = method_symbol_id
            .or_else(|| {
                match &resolved_receiver_type {
                    ConcreteType::Class { symbol_id, .. } => {
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
                            None
                        }
                    }
                    _ => {
                        // Not a class type - might be built-in method on primitive type
                        let type_name = Self::get_builtin_type_name(&resolved_receiver_type);
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

        // Capability-typed receivers dispatch dynamically. The MIR builder
        // emits `MirOperation::CallCapability` for these, which codegen
        // lowers to a runtime class-id switch. No compile-time error needed.

        // FUNC001: a method called on a class instance must be declared by the
        // class or one of its ancestors. Without this check the call compiled
        // against the SymbolId(0) placeholder and dispatched to an unrelated
        // function at runtime.
        if method_symbol_id.is_none() && resolved_method_symbol.0 == 0 {
            if let ConcreteType::Class { symbol_id, .. } = &resolved_receiver_type {
                if !Self::is_universal_method(method) {
                    let error = self.unknown_class_method_error(*symbol_id, method, location);
                    self.errors.push(error);
                }
            }
        }

        self.check_builtin_method_call(
            &resolved_receiver_type,
            method,
            &mut tast_arguments,
            location,
        );
        if matches!(resolved_receiver_type, ConcreteType::Any) {
            self.check_any_method_call(method, tast_arguments.len(), location);
        }

        // SEM004: joining a list is a namespace utility (`list.join(items, sep)`);
        // the method form is not part of the language (method-style syntax:
        // "every operation has one name").
        if matches!(resolved_receiver_type, ConcreteType::Array(_)) && method == "join" {
            self.errors.push(CompilerError::Type {
                context: Box::new(
                    crate::error::ErrorContext::new(
                        "`join` is not a list method".to_string(),
                        Some(
                            "Join a list with the namespace function: `list.join(items, sep)`"
                                .to_string(),
                        ),
                        crate::error::ErrorType::Type,
                        Some(location.clone()),
                    )
                    .with_error_code("SEM004"),
                ),
            });
        }

        // FUNC002 / SEM001: a class method's arguments must match its parameters.
        if matches!(resolved_receiver_type, ConcreteType::Class { .. })
            && matches!(
                self.symbol_table
                    .get_symbol(resolved_method_symbol)
                    .map(|s| &s.kind),
                Some(SymbolKind::Method { .. })
            )
        {
            self.check_call_arguments(
                resolved_method_symbol,
                method,
                &mut tast_arguments,
                location,
            );
            self.append_default_arguments(resolved_method_symbol, &mut tast_arguments);
        }

        // SEM005: Private method access check.
        if resolved_method_symbol.0 != 0 {
            self.check_private_member_access(
                resolved_method_symbol,
                method,
                Some(&resolved_receiver_type),
                location,
            );
        }

        Ok((
            TastExpressionKind::MethodCall {
                receiver: Box::new(tast_receiver),
                method_name: method.to_string(),
                method_symbol: resolved_method_symbol,
                arguments: tast_arguments,
                type_args: Vec::new(),
            },
            return_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `StaticMethodCall` expression.
    fn infer_static_method_call_expression(
        &mut self,
        namespace: &[String],
        class_name: &str,
        method: &str,
        method_symbol_id: &SymbolId,
        arguments: &[ResolvedHirExpression],
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        tracing::debug!(
            "DEBUG STATIC METHOD CALL: class_name='{}', method='{}', method_symbol_id=SymbolId({})",
            class_name,
            method,
            method_symbol_id.0
        );

        let mut tast_arguments = Vec::new();
        for arg in arguments {
            tast_arguments.push(self.infer_expression(arg)?);
        }

        // Handle namespace.class.method() calls
        let full_class_name = if !namespace.is_empty() {
            format!("{}.{}", namespace.join("."), class_name)
        } else {
            class_name.to_string()
        };

        tracing::debug!(
            "DEBUG TypeChecker: Calling infer_static_method_return_type for {}.{} with {} args",
            full_class_name,
            method,
            tast_arguments.len()
        );
        let return_type = self.infer_static_method_return_type(
            &full_class_name,
            method,
            &tast_arguments,
            location,
        )?;

        // Emit argument-vs-parameter type constraints for user-defined static
        // methods so the constraint solver can reject non-conforming classes
        // when a parameter is capability-typed (SEM011) and enforce ordinary
        // type equality on all other parameters. Without this, invalid calls
        // like `Registry.save(NonPersist())` compiled and only trapped at
        // codegen with "no class conforms to capability" — see fingerprint
        // #838c14541ba7 (0.33.86).
        // The resolver leaves the method unbound (SymbolId(0)) when the class
        // is declared after the caller; find it by class and name.
        let checked_method_symbol = if method_symbol_id.0 != 0 {
            Some(*method_symbol_id)
        } else {
            self.symbol_table
                .lookup_symbol(&full_class_name)
                .filter(|id| {
                    matches!(
                        self.symbol_table.get_symbol(*id).map(|s| &s.kind),
                        Some(SymbolKind::Class { .. })
                    )
                })
                .and_then(|class_id| self.symbol_table.lookup_class_member(class_id, method))
        };
        if full_class_name == "list" {
            self.check_list_namespace_arguments(method, &mut tast_arguments, location);
        }

        // FUNC002 / SEM001: the arguments must match the method's parameters;
        // left-out trailing parameters take their defaults.
        if let Some(method_symbol) = checked_method_symbol.filter(|id| {
            matches!(
                self.symbol_table.get_symbol(*id).map(|s| &s.kind),
                Some(SymbolKind::Method { .. })
            )
        }) {
            self.check_call_arguments(method_symbol, method, &mut tast_arguments, location);
            self.append_default_arguments(method_symbol, &mut tast_arguments);
            // SEM005: `Class.method()` reaches a private method only inside `Class`.
            self.check_private_member_access(method_symbol, method, None, location);
        }

        // NOTE: Use SymbolId(0) for built-in namespace methods
        // (string.*, math.*, list.*, etc.) so MIR builder creates NamedFunction operands
        // For user-defined static methods, keep the actual method_symbol_id
        let is_builtin_namespace = Self::is_builtin_namespace(&full_class_name);

        tracing::debug!(
            "DEBUG TYPE INF STATIC: full_class_name='{}', method='{}', is_builtin={}, method_symbol_id=SymbolId({})",
            full_class_name,
            method,
            is_builtin_namespace,
            method_symbol_id.0
        );

        let resolved_method_symbol = if is_builtin_namespace {
            tracing::trace!(
                "DEBUG TYPE INF STATIC: Setting SymbolId(0) for namespace method {}.{}",
                full_class_name,
                method
            );
            crate::resolver::symbol_table::SymbolId(0) // Force NamedFunction in MIR
        } else {
            tracing::debug!(
                "DEBUG TYPE INF STATIC: Keeping SymbolId({}) for user-defined static method",
                method_symbol_id.0
            );
            *method_symbol_id // Use actual symbol for user-defined static methods
        };

        Ok((
            TastExpressionKind::StaticMethodCall {
                class_name: full_class_name,
                method_name: method.to_string(),
                method_symbol: resolved_method_symbol,
                arguments: tast_arguments,
                type_args: Vec::new(),
            },
            return_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `FieldAccess` expression.
    fn infer_field_access_expression(
        &mut self,
        object: &ResolvedHirExpression,
        field: &str,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let tast_object = self.infer_expression(object)?;
        self.reject_optional_receiver(
            &tast_object.expr_type,
            &format!("reading its field '{}'", field),
            location,
        )?;

        // NOTE: Resolve the field type AND symbol ID based on the object's actual type
        // This enables inherited field access - we look up the field in the object's class hierarchy
        let (field_type, resolved_field_symbol_id) =
            self.infer_field_type_and_symbol(&tast_object.expr_type, field, location)?;

        // SEM005: Private field access check.
        if resolved_field_symbol_id.0 != 0 {
            self.check_private_member_access(resolved_field_symbol_id, field, None, location);
        }

        Ok((
            TastExpressionKind::PropertyAccess {
                object: Box::new(tast_object),
                property_name: field.to_string(),
                property_symbol: resolved_field_symbol_id,
            },
            field_type,
            location.clone(),
        ))
    }

    /// Helper: infer an `Array` literal expression.
    fn infer_array_expression(
        &mut self,
        elements: &[ResolvedHirExpression],
        element_type: &HirType,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let mut tast_elements = Vec::new();
        for element in elements {
            let tast_element = self.infer_expression(element)?;
            tast_elements.push(tast_element);
        }
        // A list literal with runtime elements (`[title]`) has no element type
        // from the HIR; it is the first element's type.
        // Elements of different classes share their nearest common parent.
        let concrete_element_type = match (element_type, tast_elements.first()) {
            (HirType::Void, Some(first)) => {
                let classes: Option<Vec<SymbolId>> = tast_elements
                    .iter()
                    .map(|e| match e.expr_type {
                        ConcreteType::Class { symbol_id, .. } => Some(symbol_id),
                        _ => None,
                    })
                    .collect();
                match classes.and_then(|ids| self.nearest_common_class(&ids)) {
                    Some(symbol_id) => ConcreteType::Class {
                        symbol_id,
                        type_args: Vec::new(),
                    },
                    None => first.expr_type.clone(),
                }
            }
            _ => self.hir_type_to_concrete(element_type),
        };

        // SEM001: the elements of a list literal share one type. Only
        // primitive element types are compared here; class and capability
        // elements are checked where the list is used.
        if let Some(first) = tast_elements.first() {
            let first_type = first.expr_type.clone();
            for element in &tast_elements[1..] {
                let element_type = &element.expr_type;
                if Self::is_primitive_value_type(&first_type)
                    && Self::is_primitive_value_type(element_type)
                    && !element_type.is_assignable_to(&first_type)
                    && !first_type.is_assignable_to(element_type)
                {
                    self.errors.push(CompilerError::type_error(
                        format!(
                            "List elements must share one type: found {} and {}",
                            first_type, element_type
                        ),
                        None,
                        Some(element.location.clone()),
                    ));
                    break;
                }
            }
        }

        let array_type = ConcreteType::Array(Box::new(concrete_element_type.clone()));

        Ok((
            TastExpressionKind::ArrayLiteral {
                elements: tast_elements,
                element_type: concrete_element_type,
            },
            array_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `UnaryOp` expression.
    fn infer_unary_op_expression(
        &mut self,
        op: &HirUnaryOp,
        operand: &ResolvedHirExpression,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let tast_operand = self.infer_expression(operand)?;
        let result_type = self.infer_unary_operation(op, &tast_operand.expr_type, location)?;

        Ok((
            TastExpressionKind::UnaryOperation {
                operator: self.convert_unary_operator(op),
                operand: Box::new(tast_operand),
            },
            result_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `Constructor` expression.
    fn infer_constructor_expression(
        &mut self,
        class_name: &str,
        class_symbol_id: &SymbolId,
        constructor_symbol_id: &SymbolId,
        arguments: &[ResolvedHirExpression],
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let mut tast_arguments = Vec::new();
        for arg in arguments {
            tast_arguments.push(self.infer_expression(arg)?);
        }
        self.check_call_arguments(
            *constructor_symbol_id,
            class_name,
            &mut tast_arguments,
            location,
        );
        self.append_default_arguments(*constructor_symbol_id, &mut tast_arguments);

        // The constructor returns an instance of the class
        let instance_type = ConcreteType::Class {
            symbol_id: *class_symbol_id,
            type_args: Vec::new(),
        };

        // Represent constructor calls as function calls
        Ok((
            TastExpressionKind::FunctionCall {
                function: Box::new(TastExpression {
                    kind: TastExpressionKind::Variable {
                        symbol_id: *constructor_symbol_id, // Use constructor's SymbolId, not class's
                        name: format!("{}.constructor", class_name),
                    },
                    expr_type: ConcreteType::Function {
                        parameters: tast_arguments.iter().map(|a| a.expr_type.clone()).collect(),
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
        ))
    }

    /// Helper: infer a `This` expression.
    fn infer_this_expression(
        &mut self,
        class_symbol_id: &SymbolId,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        // `this` refers to the current instance of the class
        let instance_type = ConcreteType::Class {
            symbol_id: *class_symbol_id,
            type_args: Vec::new(),
        };

        Ok((
            TastExpressionKind::Variable {
                symbol_id: *class_symbol_id,
                name: "this".to_string(),
            },
            instance_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `Cast` expression.
    fn infer_cast_expression(
        &mut self,
        expression: &ResolvedHirExpression,
        target_type: &HirType,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let tast_expression = self.infer_expression(expression)?;
        let target_concrete_type = self.hir_type_to_concrete(target_type);

        // A cast is lowered as a call to `cast_to_<type>`; the result has
        // the target type.
        Ok((
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
        ))
    }

    /// Helper: infer an `OnError` expression.
    fn infer_on_error_expression(
        &mut self,
        expression: &ResolvedHirExpression,
        fallback: &ResolvedHirExpression,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
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

        Ok((
            TastExpressionKind::OnError {
                expression: Box::new(tast_expression),
                fallback: Box::new(tast_fallback),
            },
            result_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `Conditional` (ternary) expression.
    fn infer_conditional_expression(
        &mut self,
        condition: &ResolvedHirExpression,
        then_expr: &ResolvedHirExpression,
        else_expr: &ResolvedHirExpression,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
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

        // The value has the common type of both branches (`if c then 1 else
        // 2.5` is a number); the MIR builder widens the narrower branch.
        let result_type = self.find_common_type(&tast_then.expr_type, &tast_else.expr_type);

        Ok((
            TastExpressionKind::Conditional {
                condition: Box::new(tast_condition),
                then_expr: Box::new(tast_then),
                else_expr: Box::new(tast_else),
            },
            result_type,
            location.clone(),
        ))
    }

    /// Helper: infer a `BaseCall` expression.
    fn infer_base_call_expression(
        &mut self,
        parent_class_symbol_id: &SymbolId,
        arguments: &[ResolvedHirExpression],
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        let mut tast_arguments = Vec::new();
        for arg in arguments {
            let tast_arg = self.infer_expression(arg)?;
            tast_arguments.push(tast_arg);
        }

        // FUNC002 / SEM001: `base(...)` calls the parent's constructor.
        if let Some(constructor) = self.class_constructors.get(parent_class_symbol_id).copied() {
            let parent_name = self
                .symbol_table
                .get_symbol(*parent_class_symbol_id)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "the parent class".to_string());
            self.check_call_arguments(
                constructor,
                &format!("base ({} constructor)", parent_name),
                &mut tast_arguments,
                location,
            );
            self.append_default_arguments(constructor, &mut tast_arguments);
        }

        // BaseCall returns null/void (it's a statement-like expression)
        Ok((
            TastExpressionKind::BaseCall {
                parent_class_symbol_id: *parent_class_symbol_id,
                arguments: tast_arguments,
            },
            ConcreteType::Null,
            location.clone(),
        ))
    }

    /// Helper: infer a `Range` expression.
    fn infer_range_expression(
        &mut self,
        start: &ResolvedHirExpression,
        end: &ResolvedHirExpression,
        step: &Option<Box<ResolvedHirExpression>>,
        inclusive: &bool,
        location: &SourceLocation,
    ) -> Result<(TastExpressionKind, ConcreteType, SourceLocation), CompilerError> {
        // Infer types for start, end, and optionally step
        let tast_start = self.infer_expression(start)?;
        let tast_end = self.infer_expression(end)?;
        let tast_step = if let Some(s) = step {
            Some(Box::new(self.infer_expression(s)?))
        } else {
            None
        };

        // SEM001: a range counts in integers, so its bounds and step are
        // integers (an `any` is checked at run time).
        let bounds = [("start", Some(&tast_start)), ("end", Some(&tast_end))]
            .into_iter()
            .chain(std::iter::once(("step", tast_step.as_deref())));
        let mut errors = Vec::new();
        for (role, bound) in bounds {
            let Some(bound) = bound else { continue };
            if !matches!(
                bound.expr_type,
                ConcreteType::Integer
                    | ConcreteType::IntegerSized { .. }
                    | ConcreteType::Any
                    | ConcreteType::Unknown
                    | ConcreteType::Generic { .. }
            ) {
                errors.push(CompilerError::type_error(
                    format!(
                        "The {} of a range must be an integer, not {}",
                        role,
                        bound.expr_type.source_name(Some(self.symbol_table))
                    ),
                    Some(
                        "A range counts in integers: `iterate i in 1 to 10 step 2`; convert a value with `.toInteger()`"
                            .to_string(),
                    ),
                    Some(self.located(&Self::argument_location(bound, location))),
                ));
            }
        }
        self.errors.extend(errors);

        // The range is typed as a list of integers (of `integer:64` when a
        // bound or the step is one, so the loop variable holds every value);
        // the MIR/codegen generates actual range values
        let is_int64 =
            |e: &TastExpression| matches!(e.expr_type, ConcreteType::IntegerSized { bits: 64, .. });
        let element_type = if is_int64(&tast_start)
            || is_int64(&tast_end)
            || tast_step.as_deref().is_some_and(is_int64)
        {
            ConcreteType::IntegerSized {
                bits: 64,
                unsigned: false,
            }
        } else {
            ConcreteType::Integer
        };
        Ok((
            TastExpressionKind::Range {
                start: Box::new(tast_start),
                end: Box::new(tast_end),
                step: tast_step,
                inclusive: *inclusive,
            },
            ConcreteType::Array(Box::new(element_type)),
            location.clone(),
        ))
    }

    /// Widen an integer-literal expression's `expr_type` from the default `Integer` to a
    /// sized integer type when the surrounding declaration demands it. This is what tells the
    /// MIR builder to emit `MirConstant::Integer64` for an `integer:64 x = 999999999999`
    /// instead of the default `MirConstant::Integer` (which codegen narrows to i32).
    ///
    /// Only widens when:
    /// - the initializer is a plain integer literal (not a binary op, call, etc.)
    /// - the typechecker tagged it with the default `ConcreteType::Integer`
    /// - the declared type is `IntegerSized { bits, unsigned: false }`
    ///
    /// Non-literal initializers (e.g. arithmetic results) are not retagged here — they keep
    /// their inferred type and rely on the existing assignment-side cast logic.
    ///
    /// A list literal of integers declared as `list<number>` or
    /// `list<integer:64>` takes the declared element type, so the MIR builder
    /// lays its elements out as 8-byte values and converts each one.
    fn widen_literal_to_declared_type(
        tast_init: &mut TastExpression,
        declared_type: &ConcreteType,
    ) {
        if let (
            TastExpressionKind::ArrayLiteral {
                elements,
                element_type,
            },
            ConcreteType::Array(declared_element),
        ) = (&mut tast_init.kind, declared_type)
        {
            let widens = matches!(
                **declared_element,
                ConcreteType::Number
                    | ConcreteType::IntegerSized {
                        bits: 64,
                        unsigned: false
                    }
            ) && matches!(element_type, ConcreteType::Integer);
            if widens {
                for element in elements.iter_mut() {
                    Self::widen_literal_to_declared_type(element, declared_element);
                }
                *element_type = (**declared_element).clone();
                tast_init.expr_type = declared_type.clone();
            }
            return;
        }
        if !matches!(
            tast_init.kind,
            TastExpressionKind::Literal {
                value: TastLiteral::Integer(_)
            }
        ) {
            return;
        }
        if !matches!(tast_init.expr_type, ConcreteType::Integer) {
            return;
        }
        if let ConcreteType::IntegerSized {
            bits,
            unsigned: false,
        } = declared_type
        {
            if *bits == 64 {
                tast_init.expr_type = declared_type.clone();
            }
        }
    }

    /// A value stored where an optional primitive `T?` is expected becomes
    /// the boxed form of `T?` (type-system.md §8, "Representation"): it is
    /// wrapped in `__optional_box`, which the MIR builder lowers to a box of
    /// the plain value, or to a conversion of an `any` or of another
    /// optional primitive (`integer?` into `number?`). `none` is already the
    /// null pointer, and a value of the target type is already boxed. The
    /// branches of a conditional expression, and the value and fallback of
    /// `onError`, are wrapped one by one.
    fn box_into_optional_primitive(value: &mut TastExpression, target: &ConcreteType) {
        if target.optional_primitive().is_none()
            || value.expr_type == *target
            || matches!(value.expr_type, ConcreteType::Null)
        {
            return;
        }
        match &mut value.kind {
            TastExpressionKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                Self::box_into_optional_primitive(then_expr, target);
                Self::box_into_optional_primitive(else_expr, target);
                value.expr_type = target.clone();
                return;
            }
            // `risky() onError none`: the fallback is stored too.
            TastExpressionKind::OnError {
                expression,
                fallback,
            } => {
                Self::box_into_optional_primitive(expression, target);
                Self::box_into_optional_primitive(fallback, target);
                value.expr_type = target.clone();
                return;
            }
            _ => {}
        }
        let boxes = value.expr_type.is_primitive_value()
            || value.expr_type.optional_primitive().is_some()
            || matches!(
                value.expr_type,
                ConcreteType::Any | ConcreteType::Unknown | ConcreteType::Generic { .. }
            );
        if !boxes {
            return;
        }
        let location = value.location.clone();
        let inner = std::mem::replace(
            value,
            TastExpression {
                kind: TastExpressionKind::Literal {
                    value: TastLiteral::Null,
                },
                expr_type: ConcreteType::Null,
                location: location.clone(),
            },
        );
        *value = TastExpression {
            kind: TastExpressionKind::FunctionCall {
                function: Box::new(TastExpression {
                    kind: TastExpressionKind::Variable {
                        symbol_id: SymbolId(0),
                        name: OPTIONAL_BOX.to_string(),
                    },
                    expr_type: ConcreteType::Unknown,
                    location: location.clone(),
                }),
                arguments: vec![inner],
                type_args: Vec::new(),
            },
            expr_type: target.clone(),
            location,
        };
    }

    /// A `list<integer>` value stored where a `list<number>` is expected
    /// becomes a new list of numbers (type-system.md §5): the elements are
    /// stored in 4-byte slots in the one and 8-byte slots in the other, so
    /// the value is copied element by element (`__list_widen_f64`). The copy
    /// is a separate list; changing one does not change the other.
    fn widen_integer_list_to_number_list(value: &mut TastExpression, target: &ConcreteType) {
        let is_list_of = |t: &ConcreteType, element: &ConcreteType| matches!(t, ConcreteType::Array(e) if **e == *element);
        if !is_list_of(&value.expr_type, &ConcreteType::Integer)
            || !is_list_of(target, &ConcreteType::Number)
        {
            return;
        }
        let location = value.location.clone();
        let list = std::mem::replace(
            value,
            TastExpression {
                kind: TastExpressionKind::Literal {
                    value: TastLiteral::Null,
                },
                expr_type: ConcreteType::Null,
                location: location.clone(),
            },
        );
        *value = TastExpression {
            kind: TastExpressionKind::FunctionCall {
                function: Box::new(TastExpression {
                    kind: TastExpressionKind::Variable {
                        symbol_id: SymbolId(0),
                        name: LIST_WIDEN_F64.to_string(),
                    },
                    expr_type: ConcreteType::Unknown,
                    location: location.clone(),
                }),
                arguments: vec![list],
                type_args: Vec::new(),
            },
            expr_type: target.clone(),
            location,
        };
    }

    /// Infer type for a literal
    fn infer_literal(&self, literal: &crate::ast::Value) -> (TastLiteral, ConcreteType) {
        match literal {
            // `integer` is 32-bit (type-system.md §2): a literal outside its
            // range is an `integer:64` value, which is how the MIR builder
            // already emits it.
            crate::ast::Value::Integer(value) if i32::try_from(*value).is_err() => (
                TastLiteral::Integer(*value),
                ConcreteType::IntegerSized {
                    bits: 64,
                    unsigned: false,
                },
            ),
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
            // `none` literal — typed as Null at the expression level.
            // The function-level return-type analysis in `infer_function` will later wrap
            // the function's declared return type in Optional(T) when it sees a Null return.
            crate::ast::Value::None => (TastLiteral::Null, ConcreteType::Null),
            crate::ast::Value::List(elements) => {
                // Infer list type from elements
                if elements.is_empty() {
                    // An empty list literal takes its element type from the
                    // declaration or parameter it is assigned to.
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
        let location = &self.located(location);
        // SEM004: `and` and `or` take boolean operands (06-expressions.md
        // §Logical Operators). `or` is not a fallback for a `T?` — that is
        // `default`, which only replaces none.
        if matches!(operator, HirBinaryOp::And | HirBinaryOp::Or) {
            let keyword = if matches!(operator, HirBinaryOp::And) {
                "and"
            } else {
                "or"
            };
            self.check_logical_operand(keyword, left_type, true, location)?;
            self.check_logical_operand(keyword, right_type, false, location)?;
            return Ok(ConcreteType::Boolean);
        }

        // Optional guarded-use enforcement (spec Step 5):
        // When an Optional(T) value appears in a context that requires T, the compiler must
        // emit an error.  The following operators are "safe" contexts for Optional:
        //   - Equal / NotEqual / Is  →  comparison against none  (`x == none`)
        //   - NullCoalesce                   →  null coalescing           (`x default fallback`)
        // All other operators (arithmetic, string concatenation, ordering)
        // require the operand to be unwrapped first.
        let operator_allows_optional = matches!(
            operator,
            HirBinaryOp::Equal
                | HirBinaryOp::NotEqual
                | HirBinaryOp::Is
                | HirBinaryOp::NullCoalesce
        );

        if !operator_allows_optional {
            // Check left operand
            if let ConcreteType::Optional(inner) = left_type {
                return Err(CompilerError::type_error(
                    format!(
                        "value of type '{}?' might be none — use 'default' for a fallback or \
                         'if' to check before using it",
                        inner
                    ),
                    Some(
                        "Use: value default fallback, value! when it cannot be none, or guard \
                         with: if value == none"
                            .to_string(),
                    ),
                    Some(location.clone()),
                ));
            }
            // Check right operand (only if not a none-literal comparison, which is fine)
            if let ConcreteType::Optional(inner) = right_type {
                return Err(CompilerError::type_error(
                    format!(
                        "value of type '{}?' might be none — use 'default' for a fallback or \
                         'if' to check before using it",
                        inner
                    ),
                    Some(
                        "Use: value default fallback, value! when it cannot be none, or guard \
                         with: if value == none"
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
                // SEM004: arithmetic on a non-numeric operand.
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
                            .with_error_code("SEM004"),
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

            HirBinaryOp::Equal | HirBinaryOp::NotEqual | HirBinaryOp::Is => {
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

            // Checked above.
            HirBinaryOp::And | HirBinaryOp::Or => Ok(ConcreteType::Boolean),

            // BOOK: null-coalescing - NullCoalesce operator (a default b)
            // Returns left if not null, otherwise returns right
            // Both sides should have compatible types
            HirBinaryOp::NullCoalesce => {
                // `value default fallback` is `value` when it is not null,
                // otherwise `fallback` (04-type-system.md): the result has the
                // operands' common type without the `?` of `value`, so
                // `integer n = maybe default 0` is a plain integer. A `null`
                // operand gives the other operand's type.
                let value_type = match left_type {
                    ConcreteType::Optional(inner) => (**inner).clone(),
                    other => other.clone(),
                };
                if value_type == ConcreteType::Null {
                    return Ok(right_type.clone());
                }
                if *right_type == ConcreteType::Null {
                    return Ok(left_type.clone());
                }
                self.add_constraint(TypeConstraint::Equality {
                    left: value_type.clone(),
                    right: right_type.clone(),
                    location: location.clone(),
                });
                if matches!(right_type, ConcreteType::Optional(_)) {
                    Ok(right_type.clone())
                } else {
                    Ok(value_type)
                }
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
        arguments: &mut [TastExpression],
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
                    "list.fill" | "list.concat" | "list.join" | "list.range"
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
                    return Err(CompilerError::semantic_error_with_code(
                        format!(
                            "Function requires at least {} arguments, got {}",
                            required_count,
                            arguments.len()
                        ),
                        None,
                        Some(location.clone()),
                        "FUNC002",
                    ));
                }

                // Skip argument count check for variadic print functions
                if !is_variadic_print_fn && arguments.len() > parameters.len() {
                    let help = self
                        .symbol_table
                        .get_symbol(function_symbol_id)
                        .and_then(|s| crate::stdlib::validator::too_many_arguments_help(&s.name))
                        .map(str::to_string);
                    return Err(CompilerError::semantic_error_with_code(
                        format!(
                            "Function accepts at most {} arguments, got {}",
                            parameters.len(),
                            arguments.len()
                        ),
                        help,
                        Some(location.clone()),
                        "FUNC002",
                    ));
                }

                // Check argument types match parameters (only for provided arguments)
                // Skip type checking for generic list functions and variadic print functions
                // `toString(value)` converts any number: the MIR builder sends a
                // `number` argument to `float_to_string` instead of the
                // registered `(integer) -> string` built-in.
                let is_to_string = self
                    .symbol_table
                    .get_symbol(function_symbol_id)
                    .is_some_and(|s| s.name == "toString");
                if !is_generic_list_fn && !is_variadic_print_fn {
                    for (param_type, arg) in parameters.iter().zip(arguments.iter_mut()) {
                        if is_to_string && arg.expr_type.is_numeric() {
                            continue;
                        }
                        self.require_assignable(arg, param_type, location);
                    }
                }
                if !is_generic_list_fn && !is_variadic_print_fn {
                    let name = self
                        .symbol_table
                        .get_symbol(function_symbol_id)
                        .map(|s| s.name.clone())
                        .unwrap_or_default();
                    self.check_sized_arguments(function_symbol_id, &name, arguments, location);
                    self.check_validator_input(&name, arguments, location);
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
        location: &SourceLocation,
    ) -> Result<ConcreteType, CompilerError> {
        // Numeric-polymorphic math functions: preserve the argument type so that
        // `integer x = math.abs(integer_val)` does not produce a false SEM004 error.
        // These checks MUST run before the type_env lookup which would return Number.
        if function_name == "math.abs" || function_name == "math_abs" {
            let arg_type = arguments.first().map(|a| a.expr_type.clone());
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
                .all(|a| matches!(a.expr_type, ConcreteType::Integer));
            return Ok(if both_int {
                ConcreteType::Integer
            } else {
                ConcreteType::Number
            });
        }

        // `list.<fn>(items, ...)` (and its `list_<fn>` alias): one set of
        // return-type rules, shared with the `list.` namespace call form.
        if let Some(list_fn) = function_name
            .strip_prefix("list.")
            .or_else(|| function_name.strip_prefix("list_"))
        {
            if Self::is_typed_list_namespace_function(list_fn) {
                return self.infer_static_method_return_type("list", list_fn, arguments, location);
            }
        }

        // time.now (frame.server / frame.auth alias) returns a Unix timestamp.
        // The typechecker needs to know this returns Integer (not Unknown/Any)
        // so `integer now = time.now()` doesn't unbox the raw i32 as a
        // pointer and OOB on the toString path. Fingerprints 95c8a7cd
        // (TIME-TOSTRING-001), ba77c082 (CODEGEN-I64-TOSTRING-OOB),
        // 97d07985 (COD001).
        if function_name == "time.now" || function_name == "time_now" || function_name == "now" {
            return Ok(ConcreteType::Integer);
        }

        // Bridge functions that always return String (length-prefixed pointer).
        // Without a typechecker entry, these fall through to Unknown → MirType::Any
        // in the MIR builder. Downstream operations (string concat, .length(),
        // toString) then emit AnyToString / UnboxAny tag-dispatch codegen that
        // reads the length prefix as a type tag → produces garbage output or
        // memory OOB. Fingerprints c2a2285c (ENV-GET-NULL-STRING-001),
        // eb2b423c (HTTP-GET-RESP-HEADERS-CORRUPTED-001), d45f7c28
        // (CODEGEN-BRIDGE-PTR-LENGTH-CONSTANT-4 — length side of same bug).
        //
        // NOTE: json.get / json_get is INTENTIONALLY excluded here because it
        // returns a boxed Any pointer (tag byte + inner pointer, from
        // `__json_get_path`), NOT a raw length-prefixed string. Typing it as
        // String would tell the concat/length paths to read `mem[ptr+0]` as
        // length — that reads the type tag byte instead, producing garbage
        // output (`name:P` for "Alice"). The correct typing is Any, so the
        // AnyToString runtime dispatch reads the tag and unwraps to the inner
        // string. Fingerprint a89cf930 (COD002).
        if function_name == "env.get"
            || function_name == "env_get"
            || function_name == "http.getResponseHeaders"
            || function_name == "http_get_response_headers"
            || function_name == "http.getResponseBody"
            || function_name == "http_get_response_body"
            || function_name == "http.responseBody"
            || function_name == "http.responseHeaders"
        {
            return Ok(ConcreteType::String);
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
                    return self.infer_static_method_return_type(
                        class_name,
                        method_name,
                        arguments,
                        location,
                    );
                }
            }

            // Look up the symbol in the symbol table to get its name
            if let Some(symbol) = self.symbol_table.get_symbol(function_symbol_id) {
                let function_name = &symbol.name;

                // A `Class.method` name is typed as a static method call,
                // with its own errors (FUNC001 / FUNC002).
                if let Some(dot_pos) = function_name.find('.') {
                    let class_name = &function_name[..dot_pos];
                    let method_name = &function_name[dot_pos + 1..];
                    return self.infer_static_method_return_type(
                        class_name,
                        method_name,
                        arguments,
                        location,
                    );
                }
            }

            Err(CompilerError::semantic_error_with_code(
                format!("Function '{}' not found", function_name),
                None,
                Some(location.clone()),
                "FUNC001",
            ))
        }
    }

    /// `list.` namespace functions whose result type depends on their list
    /// argument (see the `("list", _)` arms of `infer_static_method_return_type`).
    fn is_typed_list_namespace_function(name: &str) -> bool {
        matches!(name, "concat" | "fill" | "range")
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

    /// Signature of a built-in method of a primitive, list or pairs value
    /// (stdlib-reference.md §3–§5): its parameter types, how many of them
    /// are required, and its return type.
    fn builtin_method_signature(receiver: &ConcreteType, method: &str) -> Option<BuiltinMethod> {
        use ConcreteType as T;
        let sig = |params: Vec<ConcreteType>, required: usize, return_type: ConcreteType| {
            Some(BuiltinMethod {
                params,
                required,
                return_type,
            })
        };
        let none = |return_type: ConcreteType| sig(Vec::new(), 0, return_type);
        match (receiver, method) {
            (T::Integer, "toString") => none(T::String),
            (T::Integer, "abs" | "toInteger") => none(T::Integer),
            (T::Integer, "toNumber") => none(T::Number),
            (T::Integer, "toBoolean") => none(T::Boolean),

            (T::Number, "toString") => none(T::String),
            (T::Number, "abs" | "toNumber") => none(T::Number),
            (T::Number, "floor" | "ceil" | "round" | "toInteger") => none(T::Integer),
            (T::Number, "toBoolean") => none(T::Boolean),

            (T::Boolean, "toString") => none(T::String),

            (
                T::String,
                "toString" | "toUpperCase" | "toLowerCase" | "trim" | "trimStart" | "trimEnd",
            ) => none(T::String),
            (T::String, "length" | "size" | "toInteger") => none(T::Integer),
            (T::String, "toNumber") => none(T::Number),
            (T::String, "toBoolean" | "isEmpty" | "isNotEmpty" | "isBlank") => none(T::Boolean),
            (T::String, "substring") => sig(vec![T::Integer, T::Integer], 2, T::String),
            (T::String, "charAt") => sig(vec![T::Integer], 1, T::String),
            (T::String, "charCodeAt") => sig(vec![T::Integer], 1, T::Integer),
            (T::String, "indexOf" | "lastIndexOf") => {
                sig(vec![T::String, T::Integer], 1, T::Integer)
            }
            (T::String, "contains" | "startsWith" | "endsWith") => {
                sig(vec![T::String], 1, T::Boolean)
            }
            (T::String, "replace" | "replaceAll") => sig(vec![T::String, T::String], 2, T::String),
            (T::String, "concat") => sig(vec![T::String], 1, T::String),
            (T::String, "split") => sig(vec![T::String], 1, T::Array(Box::new(T::String))),
            (T::String, "padStart" | "padEnd") => sig(vec![T::Integer, T::String], 2, T::String),
            (T::String, "repeat") => sig(vec![T::Integer], 1, T::String),
            (T::String, "matches") => sig(vec![T::String], 1, T::Boolean),

            (T::Array(_), "length" | "size") => none(T::Integer),
            (T::Array(element), "add" | "push") => sig(vec![(**element).clone()], 1, T::Undefined),
            (T::Array(element), "insert") => {
                sig(vec![T::Integer, (**element).clone()], 2, T::Undefined)
            }
            (T::Array(element), "set") => {
                sig(vec![T::Integer, (**element).clone()], 2, T::Undefined)
            }
            (T::Array(element), "get") => sig(vec![T::Integer], 1, (**element).clone()),
            // `remove()` without an index takes the element the list's
            // behavior (`.line`, `.pile`) designates.
            (T::Array(element), "remove") => sig(vec![T::Integer], 0, (**element).clone()),
            (T::Array(element), "pop" | "removeLast" | "peek" | "first" | "last") => {
                none((**element).clone())
            }
            (T::Array(element), "contains") => sig(vec![(**element).clone()], 1, T::Boolean),
            (T::Array(element), "indexOf" | "lastIndexOf") => {
                sig(vec![(**element).clone()], 1, T::Integer)
            }
            (T::Array(_), "isEmpty" | "isNotEmpty") => none(T::Boolean),
            (T::Array(_), "toString") => none(T::String),
            (T::Array(_), "reverse" | "sort") => none(receiver.clone()),
            (T::Array(_), "slice") => sig(vec![T::Integer, T::Integer], 2, receiver.clone()),
            (T::Array(_), "concat") => sig(vec![receiver.clone()], 1, receiver.clone()),

            (T::Pairs(key, value), "get") => sig(vec![(**key).clone()], 1, (**value).clone()),
            (T::Pairs(key, _), "has") => sig(vec![(**key).clone()], 1, T::Boolean),
            (T::Pairs(key, value), "set") => {
                sig(vec![(**key).clone(), (**value).clone()], 2, T::Undefined)
            }
            (T::Pairs(key, _), "remove") => sig(vec![(**key).clone()], 1, T::Undefined),
            (T::Pairs(_, _), "len" | "size") => none(T::Integer),

            _ => None,
        }
    }

    /// The method-style utilities every integer, number, string and boolean
    /// answers (`src/stdlib/method_style.rs`, 17-method-style-syntax.md).
    fn is_method_style_utility(receiver: &ConcreteType, method: &str) -> bool {
        let scalar = matches!(
            receiver,
            ConcreteType::Integer
                | ConcreteType::Number
                | ConcreteType::String
                | ConcreteType::Boolean
        );
        let utility = matches!(
            method,
            "isDefined"
                | "isNotDefined"
                | "isEmpty"
                | "isNotEmpty"
                | "mustBeTrue"
                | "mustBeFalse"
                | "mustBeEqual"
                | "mustNotBeEqual"
                | "toString"
                | "toInteger"
                | "toNumber"
                | "toBoolean"
                | "length"
        ) || (method == "keepBetween"
            && matches!(receiver, ConcreteType::Integer | ConcreteType::Number));
        scalar && utility
    }

    /// The methods an `any` value has, with their argument count. Each one
    /// dispatches on the type the value holds at run time; any other method
    /// has no meaning for an unknown value (FUNC001).
    const ANY_METHODS: [(&'static str, usize); 10] = [
        ("toString", 0),
        ("toInteger", 0),
        ("toNumber", 0),
        ("toBoolean", 0),
        ("length", 0),
        ("get", 1),
        ("isDefined", 0),
        ("isNotDefined", 0),
        ("isEmpty", 0),
        ("isNotEmpty", 0),
    ];

    /// FUNC001 / FUNC002 for a method called on an `any` value.
    fn check_any_method_call(&mut self, method: &str, count: usize, location: &SourceLocation) {
        let Some((_, expected)) = Self::ANY_METHODS.iter().find(|(name, _)| *name == method) else {
            let names: Vec<&str> = Self::ANY_METHODS.iter().map(|(name, _)| *name).collect();
            self.errors.push(CompilerError::semantic_error_with_code(
                format!("Method '{}' not found on an `any` value", method),
                Some(format!(
                    "An `any` has {}. Assign the value to a typed variable first, \
                     e.g. `string text = value`, then call `text.{}()`",
                    names.join(", "),
                    method
                )),
                Some(location.clone()),
                "FUNC001",
            ));
            return;
        };
        if count != *expected {
            self.errors.push(CompilerError::semantic_error_with_code(
                format!(
                    "'any.{}' expects {} argument(s), got {}",
                    method, expected, count
                ),
                None,
                Some(location.clone()),
                "FUNC002",
            ));
        }
    }

    /// FUNC001 / FUNC002 / SEM001 for a method called on an integer, number,
    /// boolean, string, list or pairs value: the method must exist (in the
    /// signature table, or as a `type.method` built-in registered by the
    /// standard library), and its arguments must match the signature.
    fn check_builtin_method_call(
        &mut self,
        receiver: &ConcreteType,
        method: &str,
        arguments: &mut [TastExpression],
        location: &SourceLocation,
    ) {
        let type_name = match receiver {
            ConcreteType::Integer => "integer",
            ConcreteType::Number => "number",
            ConcreteType::Boolean => "boolean",
            ConcreteType::String => "string",
            ConcreteType::Array(_) => "list",
            ConcreteType::Pairs(_, _) => "pairs",
            _ => return,
        };
        let Some(signature) = Self::builtin_method_signature(receiver, method) else {
            let registered = Self::is_method_style_utility(receiver, method)
                || self
                    .symbol_table
                    .lookup_symbol(&format!("{}.{}", type_name, method))
                    .is_some();
            if !registered {
                let qualified = format!("{}.{}", type_name, method);
                let hint = crate::error::ErrorUtils::legacy_builtin_hint(&qualified)
                    .unwrap_or_else(|| {
                        format!(
                            "See the {} methods in the standard library reference",
                            type_name
                        )
                    });
                self.errors.push(CompilerError::semantic_error_with_code(
                    format!("Method '{}' not found on a {} value", method, type_name),
                    Some(hint),
                    Some(location.clone()),
                    "FUNC001",
                ));
            }
            return;
        };
        let count = arguments.len();
        if count < signature.required || count > signature.params.len() {
            let expected = if signature.required == signature.params.len() {
                signature.required.to_string()
            } else {
                format!("{} to {}", signature.required, signature.params.len())
            };
            self.errors.push(CompilerError::semantic_error_with_code(
                format!(
                    "'{}.{}' expects {} argument(s), got {}",
                    type_name, method, expected, count
                ),
                None,
                Some(location.clone()),
                "FUNC002",
            ));
            return;
        }
        for (param, arg) in signature.params.iter().zip(arguments.iter_mut()) {
            let arg_location = Self::argument_location(arg, location);
            self.require_assignable(arg, param, &arg_location);
        }
    }

    /// FUNC001: the `list.` namespace has only the functions of
    /// stdlib-reference.md §5 (`list.concat`, `list.range`, `list.fill`,
    /// `list.join`); every other list operation is a method on the list
    /// (`items.add(x)`), which the help names.
    fn list_namespace_error(function: &str, location: &SourceLocation) -> CompilerError {
        let method = match function {
            "size" => Some("length"),
            "push" => Some("add"),
            "pop" => Some("removeLast"),
            "length" | "add" | "remove" | "removeLast" | "peek" | "insert" | "get" | "set"
            | "contains" | "indexOf" | "lastIndexOf" | "isEmpty" | "isNotEmpty" | "first"
            | "last" | "reverse" | "sort" | "slice" => Some(function),
            _ => None,
        };
        let help = match method {
            Some(method) => format!(
                "Call it as a method on the list: `items.{method}(...)` instead of \
                 `list.{function}(items, ...)`"
            ),
            None => "The `list.` namespace has only `list.concat`, `list.range`, `list.fill` \
                     and `list.join`; other list operations are methods (`items.add(x)`)"
                .to_string(),
        };
        CompilerError::semantic_error_with_code(
            format!("Function 'list.{}' not found", function),
            Some(help),
            Some(location.clone()),
            "FUNC001",
        )
    }

    /// SEM001: `list.f(items, args…)` is the namespace form of
    /// `items.f(args…)`, so its arguments are checked against the list's
    /// element type the same way (`list.add(numbers, "x")` is an error).
    /// The argument count is checked with the return type.
    fn check_list_namespace_arguments(
        &mut self,
        function: &str,
        arguments: &mut [TastExpression],
        location: &SourceLocation,
    ) {
        let Some((items, rest)) = arguments.split_first_mut() else {
            return;
        };
        if !matches!(items.expr_type, ConcreteType::Array(_)) {
            return;
        }
        let Some(signature) = Self::builtin_method_signature(&items.expr_type, function) else {
            return;
        };
        if rest.len() < signature.required || rest.len() > signature.params.len() {
            return;
        }
        for (param, arg) in signature.params.iter().zip(rest.iter_mut()) {
            let arg_location = Self::argument_location(arg, location);
            self.require_assignable(arg, param, &arg_location);
        }
    }

    /// Infer return type of method call
    fn infer_method_return_type(
        &self,
        method_name: &str,
        receiver_type: &ConcreteType,
    ) -> Result<ConcreteType, CompilerError> {
        if let Some(signature) = Self::builtin_method_signature(receiver_type, method_name) {
            return Ok(signature.return_type);
        }
        match (receiver_type, method_name) {
            // `any` converts to text (type-system.md §9).
            (ConcreteType::Any, "toString") => Ok(ConcreteType::String),
            // The length of the array, object or string the `any` holds,
            // and a member or element, as an `any` (type-system.md §9).
            (ConcreteType::Any, "length" | "get") => Ok(ConcreteType::Any),
            // The explicit conversions give their output type
            // (type-system.md §7).
            (ConcreteType::Any, "toInteger") => Ok(ConcreteType::Integer),
            (ConcreteType::Any, "toNumber") => Ok(ConcreteType::Number),
            (ConcreteType::Any, "toBoolean") => Ok(ConcreteType::Boolean),
            (ConcreteType::Any, "isDefined" | "isNotDefined" | "isEmpty" | "isNotEmpty") => {
                Ok(ConcreteType::Boolean)
            }

            // Capability-typed receiver (`Draw thing; thing.draw()`).
            // Method resolution goes through the capability's method contract,
            // not any concrete class — dispatch is dynamic and codegen will
            // vtable-lookup the actual implementation at runtime.
            // See Clean Language Specification §Capabilities.
            (ConcreteType::Interface { symbol_id, .. }, _) => {
                if let Some(cap_sym) = self.symbol_table.get_symbol(*symbol_id) {
                    if let crate::resolver::symbol_table::SymbolKind::Capability { methods } =
                        &cap_sym.kind
                    {
                        if let Some(m) = methods.iter().find(|m| m.name == method_name) {
                            // HirType::Void → ConcreteType::Undefined so the
                            // auto-return path in MIR treats void-method calls
                            // as truly void (no local push before return);
                            // hir_type_to_concrete maps Void to Null, which
                            // triggers a spurious implicit-return-value branch.
                            let ret = match &m.return_type {
                                crate::hir::HirType::Void => ConcreteType::Undefined,
                                other => self.hir_type_to_concrete(other),
                            };
                            return Ok(ret);
                        }
                        // Named method is not part of this capability's contract.
                        return Err(CompilerError::type_error(
                            format!(
                                "Capability '{}' has no method '{}'",
                                cap_sym.name, method_name
                            ),
                            None,
                            None,
                        ));
                    }
                }
                Ok(ConcreteType::Unknown)
            }

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
                            let concrete_type = self.hir_type_to_concrete(return_type);
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

                // Method not found: infer_method_call_expression reports FUNC001
                tracing::debug!(
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

    /// A compiler built-in namespace (`string.`, `math.`, `list.`, ...),
    /// whose functions are lowered by name.
    fn is_builtin_namespace(name: &str) -> bool {
        [
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
        .any(|ns| name == *ns || name.starts_with(&format!("{}.", ns)))
    }

    /// Infer return type for static method calls
    fn infer_static_method_return_type(
        &self,
        class_name: &str,
        method_name: &str,
        arguments: &[TastExpression],
        location: &SourceLocation,
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
                    let error = CompilerError::semantic_error_with_code(
                        format!(
                            "{}() expects {} argument(s), but {} were provided",
                            method_full_name, expected, actual
                        ),
                        Some(format!("Provide exactly {} argument(s)", expected)),
                        Some(location.clone()),
                        "FUNC002",
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

            // The `list.` namespace functions (stdlib-reference.md §5).
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

            // User-defined classes: look up the method's declared return type
            // from the symbol table. Without this, every `ClassName.method(...)`
            // call on a user-defined class reported `Unknown` as its result
            // type, which the codegen then treated as a boxed `any` value —
            // dereferencing the actual i32 result as a pointer and reading
            // garbage memory. See CODEGEN_STACK_REMAINING (fp df5b8c9b1021).
            _ => {
                let class_symbol_id = self.symbol_table.lookup_symbol(class_name).filter(|id| {
                    matches!(
                        self.symbol_table.get_symbol(*id).map(|s| &s.kind),
                        Some(SymbolKind::Class { .. })
                    )
                });
                if let Some(class_symbol_id) = class_symbol_id {
                    if let Some(method_symbol_id) = self
                        .symbol_table
                        .lookup_class_member(class_symbol_id, method_name)
                    {
                        if let Some(method_symbol) = self.symbol_table.get_symbol(method_symbol_id)
                        {
                            if let SymbolKind::Method { return_type, .. } = &method_symbol.kind {
                                return Ok(self.hir_type_to_concrete(return_type));
                            }
                        }
                    }
                    return Err(self.unknown_class_method_error(
                        class_symbol_id,
                        method_name,
                        location,
                    ));
                }
                // A dotted plugin language function (`auth.csrf.generate`) is
                // registered under its full name; use its declared return type.
                if let Some(function_symbol) = self
                    .symbol_table
                    .lookup_symbol(&full_method_name)
                    .and_then(|id| self.symbol_table.get_symbol(id))
                {
                    if let SymbolKind::Function { return_type, .. } = &function_symbol.kind {
                        return Ok(match return_type {
                            Some(t) => self.hir_type_to_concrete(t),
                            None => ConcreteType::Null,
                        });
                    }
                }
                // `string.f(text, args…)` is the namespace form of the method
                // call `text.f(args…)` and has the same result type. Without
                // this, a `string.` function missing from the symbol table
                // (e.g. `string.size`) typed as unknown and its integer
                // result was unboxed as if it were an `any` pointer.
                if class_name == "string" {
                    let method = arguments.first().and_then(|receiver| {
                        Self::builtin_method_signature(&receiver.expr_type, method_name)
                    });
                    return match method {
                        Some(method) => {
                            validate_arg_count(
                                arg_count.clamp(method.required + 1, method.params.len() + 1),
                                arg_count,
                                &full_method_name,
                            )?;
                            Ok(method.return_type)
                        }
                        None => Err(CompilerError::semantic_error_with_code(
                            format!("Function '{}' not found", full_method_name),
                            Some(
                                "See the `string.` functions in the standard library reference"
                                    .to_string(),
                            ),
                            Some(location.clone()),
                            "FUNC001",
                        )),
                    };
                }
                // The `json.` namespace has exactly the functions above
                // (stdlib-reference.md §8), and `math.` / `list.` the ones
                // above and in the symbol table. Other built-in namespaces (`http.`,
                // `input.`, `validator.`) also have functions the code
                // generator provides by name.
                if matches!(class_name, "json" | "math" | "list") {
                    return Err(CompilerError::semantic_error_with_code(
                        format!("Function '{}' not found", full_method_name),
                        Some(format!(
                            "See the `{}.` functions in the standard library reference",
                            class_name
                        )),
                        Some(location.clone()),
                        "FUNC001",
                    ));
                }
                Ok(ConcreteType::Unknown)
            }
        }
    }

    /// SEM004: an operand of `and`, `or` or `not` must be a boolean
    /// (06-expressions.md §Logical Operators). An `any` holding a boolean is
    /// checked at run time; a `T?` is not a boolean, and the fallback for
    /// one is `default`.
    fn check_logical_operand(
        &mut self,
        keyword: &str,
        operand_type: &ConcreteType,
        is_left: bool,
        location: &SourceLocation,
    ) -> Result<(), CompilerError> {
        match operand_type {
            ConcreteType::Boolean | ConcreteType::Any | ConcreteType::Unknown => Ok(()),
            // An unresolved type variable becomes a boolean.
            ConcreteType::Generic { .. } => {
                self.add_constraint(TypeConstraint::Equality {
                    left: operand_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });
                Ok(())
            }
            _ => Err(Self::logical_operand_error(
                keyword,
                operand_type,
                is_left,
                Some(self.symbol_table),
                location,
            )),
        }
    }

    /// The SEM004 error for a non-boolean operand of `and`, `or` or `not`.
    fn logical_operand_error(
        keyword: &str,
        operand_type: &ConcreteType,
        is_left: bool,
        symbol_table: Option<&crate::resolver::GlobalSymbolTable>,
        location: &SourceLocation,
    ) -> CompilerError {
        let type_name = match operand_type {
            ConcreteType::Optional(inner) => format!("{}?", inner.source_name(symbol_table)),
            other => other.source_name(symbol_table),
        };
        let help = match operand_type {
            ConcreteType::Optional(_) if is_left && keyword == "or" => {
                "use `x default y` for a fallback value".to_string()
            }
            ConcreteType::Optional(inner) if **inner == ConcreteType::Boolean => {
                "use `x!` when it cannot be none, or `x default false`".to_string()
            }
            ConcreteType::Optional(_) if keyword == "not" => {
                "compare with none to get a boolean, e.g. `x == none`".to_string()
            }
            ConcreteType::Optional(_) => {
                "compare with none to get a boolean, e.g. `x != none`".to_string()
            }
            _ if keyword == "not" => {
                "`not` takes a boolean; compare the value to get one, e.g. `x == 0`".to_string()
            }
            _ => format!(
                "`{keyword}` takes boolean operands; compare the value to get one, e.g. `x != 0`"
            ),
        };
        CompilerError::semantic_error_with_code(
            format!("'{keyword}' cannot be applied to a value of type {type_name}"),
            Some(help),
            Some(location.clone()),
            "SEM004",
        )
    }

    /// Infer the result type of a unary operation
    fn infer_unary_operation(
        &self,
        operator: &HirUnaryOp,
        operand_type: &ConcreteType,
        location: &SourceLocation,
    ) -> Result<ConcreteType, CompilerError> {
        let location = &self.located(location);
        match operator {
            HirUnaryOp::Negate => match operand_type {
                ConcreteType::Integer
                | ConcreteType::Number
                | ConcreteType::IntegerSized { .. }
                | ConcreteType::NumberSized { .. } => Ok(operand_type.clone()),
                // An `any` is checked at run time; an unknown type was
                // already reported.
                ConcreteType::Any | ConcreteType::Unknown | ConcreteType::Generic { .. } => {
                    Ok(ConcreteType::Unknown)
                }
                // SEM004: unary minus is defined for numbers only; a `T?`
                // may be none and is unwrapped first.
                ConcreteType::Optional(inner) => Err(CompilerError::semantic_error_with_code(
                    format!(
                        "Unary '-' cannot be applied to a value of type {}?",
                        inner.source_name(Some(self.symbol_table))
                    ),
                    Some("The value may be none: negate `value!` or `value default 0`".to_string()),
                    Some(location.clone()),
                    "SEM004",
                )),
                _ => Err(CompilerError::semantic_error_with_code(
                    format!(
                        "Unary '-' cannot be applied to a value of type {}",
                        operand_type.source_name(Some(self.symbol_table))
                    ),
                    Some("Negation is defined for integer and number values".to_string()),
                    Some(location.clone()),
                    "SEM004",
                )),
            },
            // SEM004: `not` is defined for booleans only (an `any` holding
            // a boolean is checked at runtime).
            HirUnaryOp::Not => match operand_type {
                ConcreteType::Boolean | ConcreteType::Any | ConcreteType::Unknown => {
                    Ok(ConcreteType::Boolean)
                }
                _ => Err(Self::logical_operand_error(
                    "not",
                    operand_type,
                    false,
                    Some(self.symbol_table),
                    location,
                )),
            },
            // BOOK: required-operator - Postfix ! assertion for null check
            // Required operator returns the same type, just adds runtime null check
            // `value!` traps when the value is null and otherwise is the
            // underlying T of a T? (04-type-system.md, Nullable Types).
            HirUnaryOp::Required => match operand_type {
                ConcreteType::Optional(inner) => Ok((**inner).clone()),
                other => Ok(other.clone()),
            },
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

    /// The type and symbol of field `field_name` on a value of `object_type`.
    ///
    /// A class field is declared by the class or one of its ancestors
    /// (SEM002 when it is not); an `any` value accepts every field name (its
    /// fields are read at runtime). Lists and strings have no fields: their
    /// length is the `length()` method.
    fn infer_field_type_and_symbol(
        &self,
        object_type: &ConcreteType,
        field_name: &str,
        location: &SourceLocation,
    ) -> Result<(ConcreteType, SymbolId), CompilerError> {
        match object_type {
            ConcreteType::Class { symbol_id, .. } => {
                let class_name = self
                    .symbol_table
                    .get_symbol(*symbol_id)
                    .map(|s| s.name.clone())
                    .unwrap_or_else(|| format!("#{}", symbol_id.0));
                if let Some((field_id, field_type)) =
                    self.symbol_table.lookup_class_field(*symbol_id, field_name)
                {
                    return Ok((self.hir_type_to_concrete(field_type), field_id));
                }
                Err(CompilerError::semantic_error(
                    format!("Field '{}' not found in class '{}'", field_name, class_name),
                    Some(format!(
                        "Declare `{}` in `{}` or one of its parent classes",
                        field_name, class_name
                    )),
                    Some(location.clone()),
                ))
            }

            ConcreteType::Any => Ok((ConcreteType::Any, SymbolId(0))),

            ConcreteType::Array(_) | ConcreteType::String if field_name == "length" => {
                Err(CompilerError::semantic_error_with_code(
                    format!("A {} has no field 'length'", object_type),
                    Some("Use the method: `length()`".to_string()),
                    Some(location.clone()),
                    "SEM004",
                ))
            }

            // Unresolved types were reported where they arose.
            ConcreteType::Unknown => Ok((ConcreteType::Unknown, SymbolId(0))),

            // A `T?` may be none: its fields are reached through a `T`.
            ConcreteType::Optional(inner) => {
                let name = inner.source_name(Some(self.symbol_table));
                Err(CompilerError::type_error(
                    format!(
                        "A value of type {}? may be none, so its field '{}' needs it unwrapped",
                        name, field_name
                    ),
                    Some(format!(
                        "Read it as `value!.{}`; to change it, unwrap first: `{} plain = value!`",
                        field_name, name
                    )),
                    Some(location.clone()),
                ))
            }

            _ => Err(CompilerError::semantic_error_with_code(
                format!(
                    "A value of type {} has no field '{}'",
                    object_type, field_name
                ),
                Some(
                    "Fields belong to class instances; `any` values (decoded JSON) accept any field name"
                        .to_string(),
                ),
                Some(location.clone()),
                "SEM004",
            )),
        }
    }

    /// Explicit precision of a sized HIR type: (is_number, bits, unsigned).
    /// Unsized `integer` / `number` return None.
    fn explicit_precision(hir_type: &HirType) -> Option<(bool, u8, bool)> {
        match hir_type {
            HirType::Integer8 => Some((false, 8, false)),
            HirType::Integer8u => Some((false, 8, true)),
            HirType::Integer16 => Some((false, 16, false)),
            HirType::Integer16u => Some((false, 16, true)),
            HirType::Integer32 => Some((false, 32, false)),
            HirType::Integer32u => Some((false, 32, true)),
            HirType::Integer64 => Some((false, 64, false)),
            HirType::Integer64u => Some((false, 64, true)),
            HirType::Number32 => Some((true, 32, false)),
            HirType::Number64 => Some((true, 64, false)),
            _ => None,
        }
    }

    /// The declared type of a variable, parameter, state variable or field.
    fn declared_hir_type(&self, symbol_id: SymbolId) -> Option<HirType> {
        match &self.symbol_table.get_symbol(symbol_id)?.kind {
            SymbolKind::Variable { var_type, .. } | SymbolKind::StateVariable { var_type, .. } => {
                Some(var_type.clone())
            }
            SymbolKind::Parameter { param_type } => Some(param_type.clone()),
            SymbolKind::Field { field_type, .. } => Some(field_type.clone()),
            _ => None,
        }
    }

    /// The variable or field a value is read from, if it is one.
    fn value_source_symbol(value: &TastExpression) -> Option<SymbolId> {
        match &value.kind {
            TastExpressionKind::Variable { symbol_id, .. } => Some(*symbol_id),
            TastExpressionKind::PropertyAccess {
                property_symbol, ..
            } => Some(*property_symbol),
            _ => None,
        }
    }

    /// type-system.md §5: `integer:N → integer:M` only when M >= N and the
    /// signedness is the same, and `number:64 → number:32` is precision
    /// loss. 8–32-bit integers and both number widths share one concrete
    /// type after inference, so the check compares the declared types of a
    /// variable, parameter or field stored where a narrower (or differently
    /// signed) explicit precision is declared: a declaration, assignment or
    /// argument (SEM004).
    fn check_sized_conversion(
        &mut self,
        target: &HirType,
        source: Option<SymbolId>,
        what: &str,
        location: &SourceLocation,
    ) {
        let Some(source_type) = source.and_then(|id| self.declared_hir_type(id)) else {
            return;
        };
        let Some(message) = Self::sized_conversion_error(target, &source_type) else {
            return;
        };
        self.errors.push(CompilerError::Validation {
            context: Box::new(
                crate::error::ErrorContext::new(
                    format!("{} for {}", message, what),
                    Some(
                        "Declare the target with the wider type, or convert the value explicitly."
                            .to_string(),
                    ),
                    crate::error::ErrorType::Validation,
                    Some(self.located(location)),
                )
                .with_error_code("SEM004"),
            ),
        });
    }

    /// Why a `source` sized type may not be stored as a `target` sized type
    /// (see `check_sized_conversion`), or None when it may.
    fn sized_conversion_error(target: &HirType, source: &HirType) -> Option<String> {
        let (target_is_number, target_bits, target_unsigned) = Self::explicit_precision(target)?;
        let (source_is_number, source_bits, source_unsigned) = Self::explicit_precision(source)?;
        if source_is_number != target_is_number {
            return None;
        }
        let name = |bits: u8, unsigned: bool| {
            let kind = if source_is_number {
                "number"
            } else {
                "integer"
            };
            format!("{kind}:{bits}{}", if unsigned { "u" } else { "" })
        };
        let from = name(source_bits, source_unsigned);
        let to = name(target_bits, target_unsigned);
        if source_bits > target_bits {
            return Some(format!(
                "Narrowing assignment: {from} cannot be assigned to {to} (precision loss)"
            ));
        }
        // 64-bit integers keep their signedness in their concrete type; the
        // constraint solver reports that mismatch.
        if !source_is_number
            && source_unsigned != target_unsigned
            && !(source_bits == 64 && target_bits == 64)
        {
            return Some(format!(
                "Signedness mismatch: {from} cannot be assigned to {to}"
            ));
        }
        None
    }

    /// Convert HIR type to concrete type
    fn hir_type_to_concrete(&self, hir_type: &HirType) -> ConcreteType {
        match hir_type {
            HirType::Integer => ConcreteType::Integer,
            HirType::Number => ConcreteType::Number,
            HirType::String => ConcreteType::String,
            HirType::Boolean => ConcreteType::Boolean,
            HirType::Void => ConcreteType::Null,
            HirType::Handler => ConcreteType::Handler,
            // 8/16/32-bit integers collapse to `Integer` (i32 in WASM) — no precision loss.
            // 64-bit integers preserve their precision modifier so the MIR builder can route
            // literals to `MirConstant::Integer64` and codegen can emit i64 ops; otherwise
            // `integer:64 x = 9223372036854775807` silently truncates to -1.
            HirType::Integer8 => ConcreteType::Integer,
            HirType::Integer8u => ConcreteType::Integer,
            HirType::Integer16 => ConcreteType::Integer,
            HirType::Integer16u => ConcreteType::Integer,
            HirType::Integer32 => ConcreteType::Integer,
            HirType::Integer32u => ConcreteType::Integer,
            HirType::Integer64 => ConcreteType::IntegerSized {
                bits: 64,
                unsigned: false,
            },
            HirType::Integer64u => ConcreteType::IntegerSized {
                bits: 64,
                unsigned: true,
            },
            HirType::Number32 => ConcreteType::Number,
            HirType::Number64 => ConcreteType::Number,
            HirType::Optional(inner) => {
                ConcreteType::Optional(Box::new(self.hir_type_to_concrete(inner)))
            }
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
                                // Classes have no type parameters (a generic
                                // placeholder is written `any`), so a class type
                                // carries no type arguments.
                                ConcreteType::Class {
                                    symbol_id,
                                    type_args: Vec::new(),
                                }
                            }
                            SymbolKind::Capability { .. } => {
                                // A `can` capability used as a type — parameters, return
                                // types, and variable declarations may all name a capability.
                                // Reuses the existing `Interface` variant (semantically
                                // identical: nominal contract of methods) so MIR/codegen
                                // treat capability values as object pointers uniformly.
                                // See Clean Language Specification §Capabilities and
                                // grammar.ebnf §6.4a.
                                ConcreteType::Interface {
                                    symbol_id,
                                    type_args: Vec::new(),
                                }
                            }
                            _ => {
                                // A name that is not a type (a function or a
                                // variable) has no type; the resolver reports it.
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
                // The symbol's type is set where its value is bound (a loop
                // variable takes the list's element type, a `later` binding
                // its expression's type); the placeholder itself has none.
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

    /// The nearest class that every one of `classes` is (or inherits from):
    /// the first ancestor of the first class that the others descend from.
    fn nearest_common_class(&self, classes: &[SymbolId]) -> Option<SymbolId> {
        let (first, rest) = classes.split_first()?;
        let mut candidate = Some(*first);
        let mut visited = std::collections::HashSet::new();
        while let Some(id) = candidate {
            if !visited.insert(id) {
                break;
            }
            if rest.iter().all(|c| self.class_is_subclass_of(*c, id)) {
                return Some(id);
            }
            candidate = match self.symbol_table.get_symbol(id).map(|s| &s.kind) {
                Some(SymbolKind::Class { parent, .. }) => *parent,
                _ => None,
            };
        }
        None
    }

    /// Give a value the type of the slot it is stored into where the value's
    /// own type is only a stand-in for it:
    ///
    /// - `value onError fallback` with an `any` value converts the value to
    ///   the slot's type inside the protected region, so a failed conversion
    ///   gives the fallback (14-error-handling.md);
    /// - a list literal of subclasses of the slot's element class is a list
    ///   of that class (type-system.md §Assignability), and one of classes
    ///   conforming to the slot's element capability a list of that
    ///   capability.
    fn fit_to_expected_type(&mut self, value: &mut TastExpression, target: &ConcreteType) {
        match (&mut value.kind, target) {
            (TastExpressionKind::OnError { expression, .. }, _)
                if matches!(expression.expr_type, ConcreteType::Any)
                    && matches!(
                        target,
                        ConcreteType::Integer
                            | ConcreteType::Number
                            | ConcreteType::IntegerSized { bits: 64, .. }
                            | ConcreteType::String
                            | ConcreteType::Boolean
                    ) =>
            {
                value.expr_type = target.clone();
            }
            (
                TastExpressionKind::ArrayLiteral {
                    elements,
                    element_type,
                },
                ConcreteType::Array(target_element),
            ) if !elements.is_empty() => match **target_element {
                ConcreteType::Class {
                    symbol_id: parent, ..
                } => {
                    let all_subclasses = elements.iter().all(|e| {
                        matches!(e.expr_type, ConcreteType::Class { symbol_id, .. }
                            if self.class_is_subclass_of(symbol_id, parent))
                    });
                    if all_subclasses {
                        *element_type = (**target_element).clone();
                        value.expr_type = target.clone();
                    }
                }
                // A list of a capability: every class instance must conform
                // to it, checked as for a capability-typed parameter (SEM011).
                ConcreteType::Interface { .. }
                    if elements
                        .iter()
                        .all(|e| matches!(e.expr_type, ConcreteType::Class { .. })) =>
                {
                    for element in elements.iter() {
                        self.add_constraint(TypeConstraint::Subtype {
                            subtype: element.expr_type.clone(),
                            supertype: (**target_element).clone(),
                            location: element.location.clone(),
                        });
                    }
                    *element_type = (**target_element).clone();
                    value.expr_type = target.clone();
                }
                _ => {}
            },
            _ => {}
        }
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

    /// FUNC004: a function body returns a value on every path. A body whose
    /// last statement is an expression returns that expression's value
    /// (automatic return, 10-functions.md), as the MIR builder lowers it.
    fn body_always_returns(body: &TastBlock) -> bool {
        Self::never_falls_through(&body.statements)
            || matches!(
                body.statements.last(),
                Some(TastStatement::Expression { expression, .. })
                    if !Self::is_runtime_call(expression, "__err_region_end")
            )
    }

    /// Whether running `statements` can never reach the end of the block:
    /// every path returns, raises an error (`error(...)` is a call to
    /// `__err_raise`, which traps or unwinds to an `onError` handler), or
    /// stays in a loop no `break` leaves.
    ///
    /// The HIR builder lowers an `onError` handler to a region —
    /// `__err_region_begin()`, the protected statements, `__err_region_end()`
    /// — followed by `if __err_take(): <handler>`. An error raised in the
    /// protected statements continues in the handler, so the region ends the
    /// block when both the protected statements and the handler do.
    fn never_falls_through(statements: &[TastStatement]) -> bool {
        let mut index = 0;
        while index < statements.len() {
            if Self::is_runtime_call_statement(&statements[index], "__err_region_begin") {
                if let Some(end) = Self::error_region_end(statements, index) {
                    let handler = statements.get(end + 1).and_then(Self::error_handler_body);
                    if let Some(handler) = handler {
                        if Self::never_falls_through(&statements[index + 1..end])
                            && Self::never_falls_through(&handler.statements)
                        {
                            return true;
                        }
                        index = end + 2;
                        continue;
                    }
                }
            }
            if Self::statement_never_falls_through(&statements[index]) {
                return true;
            }
            index += 1;
        }
        false
    }

    fn statement_never_falls_through(statement: &TastStatement) -> bool {
        match statement {
            TastStatement::Return { .. } => true,
            TastStatement::Expression { expression, .. } => {
                Self::is_runtime_call(expression, "__err_raise")
            }
            TastStatement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let then_ends = Self::never_falls_through(&then_block.statements);
                match else_block {
                    Some(else_block) => {
                        then_ends && Self::never_falls_through(&else_block.statements)
                    }
                    // `if true` (plugin-expanded `html:` blocks) always runs
                    // its body.
                    None => then_ends && Self::is_literal_true(condition),
                }
            }
            TastStatement::While {
                condition, body, ..
            } => Self::is_literal_true(condition) && !Self::breaks_out(&body.statements),
            _ => false,
        }
    }

    /// Whether a `break` in `statements` leaves the loop whose body they are
    /// (a `break` inside a nested loop leaves only that loop).
    fn breaks_out(statements: &[TastStatement]) -> bool {
        statements.iter().any(|statement| match statement {
            TastStatement::Break { .. } => true,
            TastStatement::If {
                then_block,
                else_block,
                ..
            } => {
                Self::breaks_out(&then_block.statements)
                    || else_block
                        .as_ref()
                        .is_some_and(|b| Self::breaks_out(&b.statements))
            }
            _ => false,
        })
    }

    fn is_literal_true(expression: &TastExpression) -> bool {
        matches!(
            expression.kind,
            TastExpressionKind::Literal {
                value: TastLiteral::Boolean(true)
            }
        )
    }

    /// A call to the compiler's runtime function `name` (`__err_raise`,
    /// `__err_region_begin`, ...).
    fn is_runtime_call(expression: &TastExpression, name: &str) -> bool {
        matches!(
            &expression.kind,
            TastExpressionKind::FunctionCall { function, .. }
                if matches!(&function.kind, TastExpressionKind::Variable { name: n, .. } if n == name)
        )
    }

    fn is_runtime_call_statement(statement: &TastStatement, name: &str) -> bool {
        matches!(statement, TastStatement::Expression { expression, .. } if Self::is_runtime_call(expression, name))
    }

    /// Index of the `__err_region_end()` closing the region opened at
    /// `begin` (regions nest).
    fn error_region_end(statements: &[TastStatement], begin: usize) -> Option<usize> {
        let mut depth = 0usize;
        for (offset, statement) in statements[begin + 1..].iter().enumerate() {
            if Self::is_runtime_call_statement(statement, "__err_region_begin") {
                depth += 1;
            } else if Self::is_runtime_call_statement(statement, "__err_region_end") {
                if depth == 0 {
                    return Some(begin + 1 + offset);
                }
                depth -= 1;
            }
        }
        None
    }

    /// The handler of `if __err_take(): <handler>`.
    fn error_handler_body(statement: &TastStatement) -> Option<&TastBlock> {
        match statement {
            TastStatement::If {
                condition,
                then_block,
                else_block: None,
                ..
            } if Self::is_runtime_call(condition, "__err_take") => Some(then_block),
            _ => None,
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
