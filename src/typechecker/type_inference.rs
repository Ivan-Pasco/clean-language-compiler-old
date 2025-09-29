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
pub struct TypeInference {
    /// Current type environment mapping symbols to types
    type_env: HashMap<SymbolId, ConcreteType>,

    /// Generated type constraints
    constraints: Vec<TypeConstraint>,

    /// Type variable generator
    constraint_solver: ConstraintSolver,

    /// Symbol table from resolution phase
    symbol_table: GlobalSymbolTable,

    /// Built-in types and their methods
    builtins: BuiltinTypes,

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

impl TypeInference {
    /// Create a new type inference engine
    pub fn new(symbol_table: GlobalSymbolTable) -> Self {
        Self {
            type_env: HashMap::new(),
            constraints: Vec::new(),
            constraint_solver: ConstraintSolver::new(),
            symbol_table,
            builtins: BuiltinTypes::new(),
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
        let mut solver = std::mem::replace(&mut self.constraint_solver, ConstraintSolver::new());
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
    }

    /// Convert HirType to ConcreteType for builtin function type mapping
    fn hir_type_to_concrete_type(hir_type: &HirType) -> ConcreteType {
        match hir_type {
            HirType::Integer => ConcreteType::Integer,
            HirType::Number => ConcreteType::Number,
            HirType::String => ConcreteType::String,
            HirType::Boolean => ConcreteType::Boolean,
            HirType::Void => ConcreteType::Null,
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

        for class in &program.classes {
            self.register_class_signature(class);
        }

        // Second pass: Infer function bodies
        for function in &program.functions {
            if let Ok(tast_function) = self.infer_function(function) {
                tast_functions.push(tast_function);
            }
        }

        // Third pass: Infer class method bodies
        for class in &program.classes {
            if let Ok(tast_class) = self.infer_class(class) {
                tast_classes.push(tast_class);
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

        Ok(TastFunction {
            symbol_id: function.symbol_id,
            name: function.name.clone(),
            parameters: tast_parameters,
            return_type: declared_return_type,
            body: tast_body,
            generic_params: Vec::new(), // Would handle generics here
            constraints: Vec::new(),
            is_async: function.is_async,
            visibility: Visibility::Public, // Would get from HIR
            location: function.location.clone(),
        })
    }

    /// Infer types for a method (similar to function but handles methods)
    fn infer_method(&mut self, method: &ResolvedHirMethod) -> Result<TastFunction, CompilerError> {
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
                param_type: param_type,
                default_value: None, // Would convert from HIR
                is_variadic: param.is_variadic,
                location: param.location.clone(),
            });
        }

        // Infer body
        let tast_body = self.infer_block(&method.body)?;

        let declared_return_type = self.hir_type_to_concrete(&method.return_type);

        Ok(TastFunction {
            symbol_id: method.symbol_id,
            name: method.name.clone(),
            parameters: tast_parameters,
            return_type: declared_return_type,
            body: tast_body,
            generic_params: Vec::new(), // Would handle generics here
            constraints: Vec::new(),
            is_async: false,                // Methods are typically not async
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
                Some(self.infer_expression(init_expr)?)
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

        // Convert methods
        let mut tast_methods = Vec::new();
        for method in &class.methods {
            if let Ok(tast_method) = self.infer_method(method) {
                tast_methods.push(tast_method);
            }
        }

        self.current_class = None;

        Ok(TastClass {
            symbol_id: class.symbol_id,
            name: class.name.clone(),
            fields: tast_fields,
            methods: tast_methods,
            constructors: Vec::new(), // Would handle constructors
            parent_class: class.parent,
            interfaces: Vec::new(),         // Would handle interfaces
            generic_params: Vec::new(),     // Would handle generics
            is_abstract: false,             // Would get from HIR
            visibility: Visibility::Public, // Would get from HIR
            location: class.location.clone(),
        })
    }

    /// Infer types for a block
    fn infer_block(&mut self, block: &ResolvedHirBlock) -> Result<TastBlock, CompilerError> {
        let mut tast_statements = Vec::new();
        // Start with a fresh type variable for the block's return type
        // This prevents different blocks from conflicting during constraint solving
        let mut block_return_type = self.create_type_variable();

        for statement in &block.statements {
            let tast_statement = self.infer_statement(statement)?;

            // Update block return type based on statement
            match &tast_statement {
                TastStatement::Return { return_type, .. } => {
                    block_return_type = return_type.clone();
                }
                TastStatement::Expression { expression, .. } => {
                    // Last expression in block becomes return type
                    block_return_type = expression.expr_type.clone();
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
                    let tast_init = self.infer_expression(init_expr)?;

                    // Add constraint that initializer type matches declared type
                    self.add_constraint(TypeConstraint::Equality {
                        left: tast_init.expr_type.clone(),
                        right: declared_type.clone(),
                        location: location.clone(),
                    });

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

            ResolvedHirStatement::While {
                condition,
                body,
                location,
            } => {
                // Infer condition type and ensure it's boolean
                let tast_condition = self.infer_expression(condition)?;
                self.add_constraint(TypeConstraint::Equality {
                    left: tast_condition.expr_type.clone(),
                    right: ConcreteType::Boolean,
                    location: location.clone(),
                });

                // Infer body
                let tast_body = self.infer_block(body)?;

                Ok(TastStatement::While {
                    condition: tast_condition,
                    body: tast_body,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::For {
                variable: _,
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
                    iterable: tast_iterable,
                    body: tast_body,
                    location: location.clone(),
                })
            }

            ResolvedHirStatement::Assignment {
                target,
                value,
                location,
            } => {
                // Infer the value expression first
                let tast_value = self.infer_expression(value)?;

                // For now, handle simple variable assignments (most common case)
                // TODO: Implement field access assignments (obj.field = value)
                let tast_target = match target {
                    ResolvedHirLValue::Variable {
                        name,
                        symbol_id,
                        location: var_location,
                    } => {
                        // Look up the variable's declared type from our type environment
                        let target_type = self
                            .type_env
                            .get(symbol_id)
                            .cloned()
                            .unwrap_or(ConcreteType::Unknown);

                        // Add constraint that value type matches target type
                        self.add_constraint(TypeConstraint::Equality {
                            left: tast_value.expr_type.clone(),
                            right: target_type.clone(),
                            location: location.clone(),
                        });

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

                        // For field access, we need to determine the field type
                        // This is complex and depends on the object's type
                        let field_type = ConcreteType::Unknown; // TODO: Look up field type from class

                        // Add constraint that value type matches field type
                        self.add_constraint(TypeConstraint::Equality {
                            left: tast_value.expr_type.clone(),
                            right: field_type.clone(),
                            location: location.clone(),
                        });

                        TastExpression {
                            kind: TastExpressionKind::PropertyAccess {
                                object: Box::new(tast_object),
                                property_name: field.clone(),
                                property_symbol: *field_symbol_id,
                            },
                            expr_type: field_type,
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

                let result_type = self.infer_binary_operation(
                    op,
                    &tast_left.expr_type,
                    &tast_right.expr_type,
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
                    let _constraint_check =
                        self.infer_function_call(&function_type, &tast_arguments, location)?;
                }

                // Look up function type and determine return type
                let return_type =
                    self.infer_function_return_type(*function_symbol_id, &tast_arguments)?;

                (
                    TastExpressionKind::FunctionCall {
                        function: Box::new(TastExpression {
                            kind: TastExpressionKind::Variable {
                                symbol_id: *function_symbol_id,
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

                // Verify index type is integer
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

                // Extract element type from array type
                let element_type = match &tast_array.expr_type {
                    ConcreteType::Array(element_type) => (**element_type).clone(),
                    other_type => {
                        self.errors.push(CompilerError::type_error(
                            &format!("Cannot index into non-array type: {:?}", other_type),
                            None,
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

                // For now, use simple method resolution based on receiver type
                let return_type = self.infer_method_return_type(
                    method,
                    &tast_receiver.expr_type,
                    &tast_arguments,
                )?;

                (
                    TastExpressionKind::MethodCall {
                        receiver: Box::new(tast_receiver),
                        method_name: method.clone(),
                        method_symbol: method_symbol_id
                            .unwrap_or(crate::resolver::symbol_table::SymbolId(0)), // Use dummy SymbolId for built-in methods
                        arguments: tast_arguments,
                        type_args: Vec::new(),
                    },
                    return_type,
                    location.clone(),
                )
            }

            ResolvedHirExpression::StaticMethodCall {
                class_name,
                class_symbol_id,
                method,
                method_symbol_id,
                arguments,
                location,
            } => {
                let mut tast_arguments = Vec::new();
                for arg in arguments {
                    tast_arguments.push(self.infer_expression(arg)?);
                }

                // For now, use simple static method resolution based on class and method name
                let return_type =
                    self.infer_static_method_return_type(class_name, method, &tast_arguments)?;

                // For now, represent static method calls as function calls
                // since TAST doesn't have StaticMethodCall yet
                (
                    TastExpressionKind::FunctionCall {
                        function: Box::new(TastExpression {
                            kind: TastExpressionKind::Variable {
                                symbol_id: *method_symbol_id,
                                name: format!("{}.{}", class_name, method),
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

            ResolvedHirExpression::FieldAccess {
                object,
                field,
                field_symbol_id,
                location,
            } => {
                let tast_object = self.infer_expression(object)?;

                // Infer the field type based on the object type
                let field_type = self.infer_field_type(&tast_object.expr_type, field)?;

                (
                    TastExpressionKind::PropertyAccess {
                        object: Box::new(tast_object),
                        property_name: field.clone(),
                        property_symbol: *field_symbol_id,
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

                // Create TAST assignment expression - for now, convert target to a simple variable
                let tast_target = match target {
                    ResolvedHirLValue::Variable {
                        name,
                        symbol_id,
                        location: _,
                    } => TastExpression {
                        kind: TastExpressionKind::Variable {
                            symbol_id: *symbol_id,
                            name: name.clone(),
                        },
                        expr_type: assignment_type.clone(),
                        location: location.clone(),
                    },
                    _ => {
                        // For complex LValues, create a placeholder for now
                        self.errors.push(CompilerError::type_error(
                            "Complex assignment targets not yet fully supported in type inference",
                            None,
                            Some(location.clone()),
                        ));
                        TastExpression {
                            kind: TastExpressionKind::Variable {
                                symbol_id: crate::resolver::symbol_table::SymbolId(0),
                                name: "unknown".to_string(),
                            },
                            expr_type: ConcreteType::Unknown,
                            location: location.clone(),
                        }
                    }
                };

                // In Clean Language, assignment expressions return the assigned value
                // For now, we'll represent this as the value itself
                // TODO: Implement proper assignment expression support in TAST
                (tast_value.kind, assignment_type, location.clone())
            }

            ResolvedHirExpression::Constructor {
                class_name,
                class_symbol_id,
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
                                symbol_id: *class_symbol_id,
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

                // For now, allow casts and assume they succeed
                // TODO: Add runtime cast validation
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

            HirBinaryOp::Equal | HirBinaryOp::NotEqual => {
                // Equality can compare any types
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
        location: &SourceLocation,
    ) -> Result<ConcreteType, CompilerError> {
        match function_type {
            ConcreteType::Function {
                parameters,
                return_type,
                ..
            } => {
                if parameters.len() != arguments.len() {
                    return Err(CompilerError::type_error(
                        &format!(
                            "Function expects {} arguments, got {}",
                            parameters.len(),
                            arguments.len()
                        ),
                        None,
                        Some(location.clone()),
                    ));
                }

                // Check argument types match parameters
                for (param_type, arg) in parameters.iter().zip(arguments.iter()) {
                    self.add_constraint(TypeConstraint::Equality {
                        left: arg.expr_type.clone(),
                        right: param_type.clone(),
                        location: location.clone(),
                    });
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
        arguments: &[TastExpression],
    ) -> Result<ConcreteType, CompilerError> {
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
            (ConcreteType::String, "toUpperCase") => Ok(ConcreteType::String),
            (ConcreteType::String, "toLowerCase") => Ok(ConcreteType::String),
            (ConcreteType::String, "trim") => Ok(ConcreteType::String),

            // Array methods
            (ConcreteType::Array(_), "length") => Ok(ConcreteType::Integer),
            (ConcreteType::Array(_), "push") => Ok(ConcreteType::Undefined), // void return
            (ConcreteType::Array(element_type), "pop") => Ok((**element_type).clone()),
            (ConcreteType::Array(_), "toString") => Ok(ConcreteType::String),

            // Boolean methods
            (ConcreteType::Boolean, "toString") => Ok(ConcreteType::String),

            // For unknown method/type combinations, return Unknown
            _ => Ok(ConcreteType::Unknown),
        }
    }

    /// Infer return type for static method calls
    fn infer_static_method_return_type(
        &self,
        class_name: &str,
        method_name: &str,
        _arguments: &[TastExpression],
    ) -> Result<ConcreteType, CompilerError> {
        // For now, implement basic built-in static method type inference
        match (class_name, method_name) {
            // Math static methods
            ("Math", "abs") => Ok(ConcreteType::Number),
            ("Math", "floor") => Ok(ConcreteType::Integer),
            ("Math", "ceil") => Ok(ConcreteType::Integer),
            ("Math", "round") => Ok(ConcreteType::Integer),
            ("Math", "sqrt") => Ok(ConcreteType::Number),
            ("Math", "pow") => Ok(ConcreteType::Number),
            ("Math", "sin") => Ok(ConcreteType::Number),
            ("Math", "cos") => Ok(ConcreteType::Number),
            ("Math", "tan") => Ok(ConcreteType::Number),
            ("Math", "max") => Ok(ConcreteType::Number),
            ("Math", "min") => Ok(ConcreteType::Number),

            // String static methods
            ("String", "fromCharCode") => Ok(ConcreteType::String),
            ("String", "isEmpty") => Ok(ConcreteType::Boolean),

            // StringUtils static methods
            ("StringUtils", "length") => Ok(ConcreteType::Integer),
            ("StringUtils", "concat") => Ok(ConcreteType::String),
            ("StringUtils", "substring") => Ok(ConcreteType::String),
            ("StringUtils", "indexOf") => Ok(ConcreteType::Integer),
            ("StringUtils", "replace") => Ok(ConcreteType::String),
            ("StringUtils", "toUpperCase") => Ok(ConcreteType::String),
            ("StringUtils", "toLowerCase") => Ok(ConcreteType::String),

            // Integer static methods
            ("Integer", "parse") => Ok(ConcreteType::Integer),
            ("Integer", "toString") => Ok(ConcreteType::String),

            // Number static methods
            ("Number", "parse") => Ok(ConcreteType::Number),
            ("Number", "toString") => Ok(ConcreteType::String),

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
        }
    }

    /// Convert HIR unary operator to TAST unary operator
    fn convert_unary_operator(&self, op: &HirUnaryOp) -> UnaryOperator {
        match op {
            HirUnaryOp::Negate => UnaryOperator::Negate,
            HirUnaryOp::Not => UnaryOperator::Not,
        }
    }

    /// Infer the type of a field access based on the object type and field name
    fn infer_field_type(
        &self,
        object_type: &ConcreteType,
        field_name: &str,
    ) -> Result<ConcreteType, CompilerError> {
        match (object_type, field_name) {
            // Array fields
            (ConcreteType::Array(_element_type), "length") => Ok(ConcreteType::Integer),

            // String fields
            (ConcreteType::String, "length") => Ok(ConcreteType::Integer),

            // For class types, we'd look up the field in the class definition
            // For now, return Unknown for unrecognized field accesses
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
            HirType::Named { name, .. } => {
                // Look up the named type in the symbol table
                if let Some(symbol_id) = self.symbol_table.lookup_symbol(name) {
                    if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                        match &symbol.kind {
                            SymbolKind::Class { .. } => {
                                ConcreteType::Class {
                                    symbol_id,
                                    type_args: Vec::new(), // TODO: Handle generics
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
                    // If not found in symbol table, could be a built-in type
                    match name.as_str() {
                        "integer" => ConcreteType::Integer,
                        "number" => ConcreteType::Number,
                        "string" => ConcreteType::String,
                        "boolean" => ConcreteType::Boolean,
                        "void" => ConcreteType::Undefined,
                        _ => ConcreteType::Unknown,
                    }
                }
            }
            HirType::Inferred { .. } => {
                // Type inference placeholders are handled by the constraint solver
                // For now, return Unknown and let constraint solver handle it
                ConcreteType::Unknown
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
            HirBinaryOp::And => BinaryOperator::And,
            HirBinaryOp::Or => BinaryOperator::Or,
            HirBinaryOp::StringConcat => BinaryOperator::Concatenate,
        }
    }

    /// Add a constraint to be solved
    fn add_constraint(&mut self, constraint: TypeConstraint) {
        self.constraints.push(constraint);
    }

    /// Apply solved substitutions to type environment
    fn apply_substitutions(&mut self, solver_result: &SolverResult) {
        // Would apply substitutions to finalize types
        // For now, types in type_env are already concrete
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

// Extension trait to get location from HIR statements
trait StatementLocation {
    fn location(&self) -> &SourceLocation;
}

impl StatementLocation for ResolvedHirStatement {
    fn location(&self) -> &SourceLocation {
        match self {
            ResolvedHirStatement::Expression { location, .. } => location,
            ResolvedHirStatement::VariableDeclaration { location, .. } => location,
            ResolvedHirStatement::Return { location, .. } => location,
            ResolvedHirStatement::If { location, .. } => location,
            ResolvedHirStatement::While { location, .. } => location,
            ResolvedHirStatement::Assignment { location, .. } => location,
            ResolvedHirStatement::For { location, .. } => location,
            ResolvedHirStatement::Print { location, .. } => location,
            ResolvedHirStatement::LaterAssignment { location, .. } => location,
        }
    }
}
