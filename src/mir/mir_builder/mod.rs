//! MIR Builder - Lowering from TAST to MIR
//!
//! This module implements the transformation from TAST (Typed Abstract Syntax Tree)
//! to MIR (Medium-level Intermediate Representation). The builder converts high-level
//! typed constructs into optimization-friendly SSA form with explicit control flow.

#![allow(dead_code)]

use crate::ast::SourceLocation;
use crate::codegen::const_eval::{try_const_eval_tast, ConstValue};
use crate::error::CompilerError;
use crate::mir::mir_types::{
    AnyTypeTag, BasicBlockId, MirBasicBlock, MirBinaryOp, MirConstant, MirFunction,
    MirFunctionAttributes, MirGlobal, MirInstruction, MirLocal, MirOperand, MirOperation,
    MirParameter, MirProgram, MirTerminator, MirType, MirUnaryOp, ValueId,
};
use crate::resolver::SymbolId;
use crate::typechecker::tast::{
    BinaryOperator, ConcreteType, TastBlock, TastClass, TastExpression, TastExpressionKind,
    TastFunction, TastLiteral, TastProgram, TastStatement, UnaryOperator, Visibility,
};
use std::collections::{HashMap, HashSet};
use tracing::{debug, trace, warn};

mod expressions;
mod functions;
mod helpers;
mod statements;
mod types;

/// Result of MIR building process
#[derive(Debug)]
pub struct MirBuildResult {
    /// The generated MIR program
    pub program: MirProgram,

    /// Warnings generated during lowering
    pub warnings: Vec<CompilerError>,

    /// Statistics about the lowering process
    pub stats: MirBuildStats,
}

/// Statistics about the MIR building process
#[derive(Debug, Default)]
pub struct MirBuildStats {
    /// Number of functions lowered
    pub functions_lowered: usize,

    /// Number of basic blocks created
    pub basic_blocks_created: usize,

    /// Number of SSA values created
    pub ssa_values_created: usize,

    /// Number of phi nodes inserted
    pub phi_nodes_inserted: usize,

    /// Build time in microseconds
    pub build_time_us: u64,
}

/// MIR builder - converts TAST to MIR
#[derive(Debug)]
pub struct MirBuilder {
    /// Current function being built
    pub(super) current_function: Option<MirFunction>,

    /// Current basic block being built
    pub(super) current_block: Option<BasicBlockId>,

    /// SSA value counter
    pub(super) next_value_id: usize,

    /// Basic block counter
    pub(super) next_block_id: usize,

    /// Symbol ID to value ID mapping
    pub(super) symbol_values: HashMap<SymbolId, ValueId>,

    /// Variable name to value ID mapping for current scope
    pub(super) variable_values: HashMap<String, ValueId>,

    /// String literals pool
    pub(super) string_pool: Vec<String>,

    /// String literal to pool index mapping
    pub(super) string_indices: HashMap<String, usize>,

    /// Symbol table for looking up constructors, methods, etc.
    pub(super) symbol_table: std::sync::Arc<crate::resolver::GlobalSymbolTable>,

    /// Collected warnings
    pub(super) warnings: Vec<CompilerError>,

    /// Build statistics
    pub(super) stats: MirBuildStats,

    /// All classes in the program for inheritance lookups
    pub(super) all_classes: Vec<TastClass>,

    /// All functions in the program for default parameter lookups
    pub(super) all_functions: Vec<TastFunction>,

    /// State variables with their SymbolIds and types for global variable generation
    /// Maps variable name -> (SymbolId, MirType)
    pub(super) state_variables: HashMap<String, (SymbolId, MirType)>,

    /// State invariant rules to be checked at operation boundaries
    /// These are evaluated at the end of start:, frame:, and handler functions
    pub(super) state_rules: Vec<TastExpression>,

    /// State guards - per-variable validation conditions
    /// Maps state variable name → (guard condition expression, value_symbol_id, error_message)
    pub(super) state_guards: HashMap<String, (TastExpression, SymbolId, String)>,

    /// Watch block handlers: state variable name → list of watch function SymbolIds.
    ///
    /// When a state variable is mutated, a call is emitted for every SymbolId registered
    /// under that variable's name.  Watch functions are given synthetic SymbolIds starting
    /// at 2000 to avoid clashing with user-defined symbols (1–999) and the MIR-internal
    /// synthetic builtins (1000–1011).
    pub(super) watch_handlers: HashMap<String, Vec<SymbolId>>,

    /// Counter for allocating synthetic SymbolIds for watch handler functions.
    pub(super) next_watch_symbol_id: usize,

    /// Computed property name → (SymbolId, return MirType) mapping.
    ///
    /// Populated during `build()` when computed declarations are lowered into
    /// `__computed_{name}` getter functions.  At variable-read sites, if the
    /// variable name is present in this map, a Call instruction to the getter
    /// function is emitted instead of a GlobalLoad.
    ///
    /// Synthetic SymbolIds for computed getters start at 3000 to avoid
    /// clashing with builtins (1000–1011) and watch handlers (2000+).
    pub(super) computed_properties: HashMap<String, (SymbolId, MirType)>,

    /// Counter for allocating synthetic SymbolIds for computed getter functions.
    pub(super) next_computed_symbol_id: usize,

    /// Class always: condition expressions to be injected before every return in the
    /// currently-being-built class method.  Set by `build_class` before calling
    /// `build_function_with_class_context` and cleared afterwards.
    pub(super) pending_class_invariants: Vec<TastExpression>,
}

/// Context for building a single function
#[derive(Debug)]
pub(super) struct FunctionBuildContext {
    /// Function being built
    pub(super) function: MirFunction,

    /// Stack of scopes for variable resolution
    pub(super) scope_stack: Vec<HashMap<String, ValueId>>,

    /// Pending phi nodes to resolve
    pub(super) pending_phis: Vec<PendingPhi>,

    /// Loop context stack for break/continue
    pub(super) loop_stack: Vec<LoopContext>,

    /// Class context for field access (if this is a class method)
    pub(super) class_context: Option<TastClass>,

    /// All classes in the program for inheritance lookups
    pub(super) all_classes: Vec<TastClass>,

    /// All functions in the program for default parameter lookups
    pub(super) all_functions: Vec<TastFunction>,

    /// Pending ensure postcondition expressions collected during statement lowering.
    /// These are evaluated just before every `return` in the function body:
    ///   1. The return value is stored in a `result` local.
    ///   2. Each ensure condition is evaluated (with `result` in scope).
    ///   3. If any condition is false the function traps.
    ///   4. The stored `result` value is returned.
    /// Only populated in debug / `--contracts` builds; empty in release builds.
    pub(super) ensure_conditions: Vec<TastExpression>,
}

/// Pending phi node that needs to be resolved
#[derive(Debug)]
pub(super) struct PendingPhi {
    /// The phi instruction value ID
    pub(super) value_id: ValueId,

    /// Variable name this phi represents
    pub(super) variable_name: String,

    /// Basic block where phi is located
    pub(super) block_id: BasicBlockId,

    /// Predecessor blocks and their values
    pub(super) incoming: Vec<(BasicBlockId, Option<ValueId>)>,
}

/// Loop context for break/continue statements
#[derive(Debug)]
pub(super) struct LoopContext {
    /// Basic block to jump to on continue
    pub(super) continue_block: BasicBlockId,

    /// Basic block to jump to on break
    pub(super) break_block: BasicBlockId,
}

impl MirBuilder {
    /// Create a new MIR builder
    pub fn new(symbol_table: std::sync::Arc<crate::resolver::GlobalSymbolTable>) -> Self {
        // Scan symbol table for state variables and store their SymbolIds for global variable generation
        let mut state_variables = HashMap::new();

        let all_symbols: Vec<_> = symbol_table.accessible_symbols();
        for symbol_id in all_symbols {
            if let Some(symbol) = symbol_table.get_symbol(symbol_id) {
                if let crate::resolver::SymbolKind::StateVariable { var_type, .. } = &symbol.kind {
                    let mir_type = match var_type {
                        crate::hir::HirType::Integer => MirType::I32,
                        crate::hir::HirType::Number => MirType::F64,
                        crate::hir::HirType::String => MirType::I32, // String pointer
                        crate::hir::HirType::Boolean => MirType::I32,
                        _ => MirType::I32, // Default to i32 for other types
                    };
                    state_variables.insert(symbol.name.clone(), (symbol_id, mir_type.clone()));
                    tracing::trace!(
                        "Registered state variable '{}' with SymbolId {:?}, type {:?}",
                        symbol.name,
                        symbol_id,
                        mir_type
                    );
                }
            }
        }

        Self {
            current_function: None,
            current_block: None,
            next_value_id: 0,
            next_block_id: 0,
            symbol_values: HashMap::new(),
            variable_values: HashMap::new(),
            string_pool: Vec::new(),
            string_indices: HashMap::new(),
            symbol_table,
            warnings: Vec::new(),
            stats: MirBuildStats::default(),
            all_classes: Vec::new(),
            all_functions: Vec::new(),
            state_variables,
            state_rules: Vec::new(),
            state_guards: HashMap::new(),
            watch_handlers: HashMap::new(),
            next_watch_symbol_id: 2000,
            computed_properties: HashMap::new(),
            next_computed_symbol_id: 3000,
            pending_class_invariants: Vec::new(),
        }
    }

    /// Build MIR from TAST program
    pub fn build(&mut self, tast: TastProgram) -> Result<MirBuildResult, Vec<CompilerError>> {
        let start_time = std::time::Instant::now();

        // Store all classes for inheritance lookups
        self.all_classes = tast.classes.clone();

        // Store all functions for default parameter lookups
        self.all_functions = tast.functions.clone();

        // Store state rules and guards for runtime enforcement
        if let Some(ref state_block) = tast.state {
            self.state_rules = state_block.rules.clone();
            if !self.state_rules.is_empty() {
                debug!(
                    rule_count = self.state_rules.len(),
                    "State rules registered for operation boundary checking"
                );
            }
            // Extract guards from state declarations
            for decl in &state_block.declarations {
                if let Some(ref guard) = decl.guard {
                    self.state_guards.insert(
                        decl.name.clone(),
                        (
                            guard.condition.clone(),
                            guard.value_symbol_id,
                            guard.error_message.clone(),
                        ),
                    );
                    debug!(
                        var_name = %decl.name,
                        error_msg = %guard.error_message,
                        "State guard registered for variable"
                    );
                }
            }
        }

        // Pre-register watch handler SymbolIds BEFORE building any functions.
        //
        // This two-phase approach is required: function bodies (start:, frame:, etc.) are built
        // first and may contain state mutations.  For the GlobalStore injection to emit calls to
        // watch handlers, `self.watch_handlers` must already be populated at that point.
        //
        // Phase 1 (here): allocate a SymbolId for every (target, watch_block) pair and record
        //   the mapping so the injection code in `build_statement` can look up the handler IDs.
        // Phase 2 (after class lowering): actually build the handler function bodies and insert
        //   them into the MIR program.
        //
        // We store (target_name, fn_name, symbol_id, watch_block_index) for phase 2.
        let mut watch_block_registrations: Vec<(String, String, SymbolId, usize)> = Vec::new();
        for (block_idx, watch_block) in tast.watch_blocks.iter().enumerate() {
            for target_name in &watch_block.targets {
                let symbol_id = SymbolId(self.next_watch_symbol_id);
                self.next_watch_symbol_id += 1;
                let fn_name = format!("__watch_{}", target_name);
                self.watch_handlers
                    .entry(target_name.clone())
                    .or_default()
                    .push(symbol_id);
                watch_block_registrations.push((
                    target_name.clone(),
                    fn_name,
                    symbol_id,
                    block_idx,
                ));
                debug!(
                    target = %target_name,
                    symbol_id = symbol_id.0,
                    "Pre-registered watch handler SymbolId (phase 1)"
                );
            }
        }

        // Pre-register computed property getter SymbolIds BEFORE building any functions.
        //
        // This is the same two-phase approach used for watch handlers.  Function bodies
        // (start:, frame:, etc.) are built first.  If they read a computed property, the
        // Variable expression builder must already know to emit a Call rather than a
        // GlobalLoad.  We achieve this by populating `self.computed_properties` here, so
        // variable reads during function body building resolve correctly.
        //
        // Phase 1 (here): allocate SymbolIds, populate `self.computed_properties`.
        // Phase 2 (after watch-block lowering): build the actual getter function bodies.
        //
        // We store (computed_name, fn_name, symbol_id) for phase 2.
        let mut computed_registrations: Vec<(String, String, SymbolId)> = Vec::new();
        if let Some(ref state_block) = tast.state {
            for comp_decl in &state_block.computed {
                let symbol_id = SymbolId(self.next_computed_symbol_id);
                self.next_computed_symbol_id += 1;
                let fn_name = format!("__computed_{}", comp_decl.name);
                let return_mir_type = MirType::from_concrete_type(&comp_decl.computed_type);
                // Populate the map so Variable expression builds will see it.
                self.computed_properties
                    .insert(comp_decl.name.clone(), (symbol_id, return_mir_type));
                computed_registrations.push((comp_decl.name.clone(), fn_name, symbol_id));
                debug!(
                    computed_name = %comp_decl.name,
                    symbol_id = symbol_id.0,
                    "Pre-registered computed getter SymbolId (phase 1)"
                );
            }
        }

        // Initialize program structure
        let mut mir_program = MirProgram {
            functions: HashMap::new(),
            globals: HashMap::new(),
            string_pool: Vec::new(),
            entry_point: None,
            debug_info: None,
            symbol_name_map: HashMap::new(),
            used_plugins: Vec::new(),
            state_rules: Vec::new(),
            state_guards: Vec::new(),
            externals: Vec::new(),
        };

        // NOTE: Populate symbol_name_map from SymbolTable for dynamic resolution
        // This captures ALL symbols: builtins (print, math.*, string.*, etc.) AND user-defined
        debug!(
            symbol_count = tast.symbol_table.all_symbols().len(),
            "Populating symbol_name_map from SymbolTable"
        );
        for (symbol_id, symbol) in tast.symbol_table.all_symbols() {
            // NOTE: For Method symbols, construct fully qualified name (e.g., "math.min")
            let full_name =
                if let crate::resolver::SymbolKind::Method { class_id, .. } = &symbol.kind {
                    // Get the class name for this method
                    if let Some(class_symbol) = tast.symbol_table.all_symbols().get(class_id) {
                        format!("{}.{}", class_symbol.name.to_lowercase(), symbol.name)
                    } else {
                        symbol.name.clone()
                    }
                } else {
                    symbol.name.clone()
                };

            mir_program
                .symbol_name_map
                .insert(*symbol_id, full_name.clone());
            trace!(symbol_id = symbol_id.0, name = %full_name, "Registered symbol");
        }
        debug!(
            entry_count = mir_program.symbol_name_map.len(),
            "symbol_name_map populated"
        );

        // NOTE: Add synthetic SymbolIds for MIR-generated built-in functions
        // These are created during MIR building for string concatenation and power operations
        // Map them to existing WASM builtin functions
        mir_program
            .symbol_name_map
            .insert(SymbolId(1000), "string.concat".to_string());
        // pow_f64 and pow_i32 should map to the existing math.pow function
        mir_program
            .symbol_name_map
            .insert(SymbolId(1001), "math.pow".to_string());
        mir_program
            .symbol_name_map
            .insert(SymbolId(1002), "math.pow".to_string());
        // list.allocate and list.push for array literal creation
        mir_program
            .symbol_name_map
            .insert(SymbolId(1003), "list.allocate".to_string());
        // NOTE: Use names matching stdlib function registration
        // list_push uses underscore (from list_ops.rs line 112)
        // list.size uses dot (from list_behavior.rs line 96)
        mir_program
            .symbol_name_map
            .insert(SymbolId(1004), "list_push".to_string());
        mir_program
            .symbol_name_map
            .insert(SymbolId(1005), "list_push_f64".to_string());
        mir_program
            .symbol_name_map
            .insert(SymbolId(1006), "list.size".to_string());
        // CRITICAL: list.add (in-place modification) vs list_push (creates new list)
        mir_program
            .symbol_name_map
            .insert(SymbolId(1007), "list.add".to_string());
        mir_program
            .symbol_name_map
            .insert(SymbolId(1008), "list.add_f64".to_string());

        // NOTE: Add common name variations for builtin functions
        // Check symbol_name_map for variations and add correct WASM names
        let mut fixes_needed = Vec::new();
        for (symbol_id, name) in mir_program.symbol_name_map.iter() {
            match name.as_str() {
                "println" => fixes_needed.push((*symbol_id, "printl".to_string())),
                // Map underscore naming to dot naming for list operations
                "list_push_f64" => fixes_needed.push((*symbol_id, "list.push_f64".to_string())),
                "list_push" => fixes_needed.push((*symbol_id, "list.push".to_string())),
                _ => {}
            }
        }
        for (symbol_id, corrected_name) in fixes_needed {
            trace!(
                symbol_id = symbol_id.0,
                from = %mir_program.symbol_name_map.get(&symbol_id).unwrap(),
                to = %corrected_name,
                "Correcting symbol name"
            );
            mir_program
                .symbol_name_map
                .insert(symbol_id, corrected_name);
        }

        debug!(
            final_count = mir_program.symbol_name_map.len(),
            "MIR symbol_name_map ready (includes 5 synthetic SymbolIds)"
        );

        // Lower all functions
        for function in tast.functions {
            let symbol_id = function.symbol_id;
            let name = function.name.clone();

            match self.build_function(function) {
                Ok(mir_function) => {
                    if mir_function.attributes.entry_point {
                        mir_program.entry_point = Some(mir_function.symbol_id);
                    }
                    // NOTE: Add function name to symbol map for dynamic resolution
                    mir_program.symbol_name_map.insert(symbol_id, name);
                    mir_program
                        .functions
                        .insert(mir_function.symbol_id, mir_function);
                    self.stats.functions_lowered += 1;
                }
                Err(errors) => {
                    self.warnings.extend(errors);
                }
            }
        }

        // NOTE: Lower the start function if it exists
        if let Some(start_function) = tast.start_function {
            let symbol_id = start_function.symbol_id;
            let name = start_function.name.clone();

            match self.build_function(start_function) {
                Ok(mir_function) => {
                    // Mark the start function as the entry point
                    mir_program.entry_point = Some(mir_function.symbol_id);
                    // NOTE: Add start function name to symbol map
                    mir_program.symbol_name_map.insert(symbol_id, name);
                    mir_program
                        .functions
                        .insert(mir_function.symbol_id, mir_function);
                    self.stats.functions_lowered += 1;
                }
                Err(errors) => {
                    self.warnings.extend(errors);
                }
            }
        }

        // Lower all classes (as constructor functions and method functions)
        for class in tast.classes {
            match self.build_class(class) {
                Ok(class_functions) => {
                    for function in class_functions {
                        // NOTE: Add class method/constructor names to symbol map
                        mir_program
                            .symbol_name_map
                            .insert(function.symbol_id, function.name.clone());
                        mir_program.functions.insert(function.symbol_id, function);
                        self.stats.functions_lowered += 1;
                    }
                }
                Err(errors) => {
                    self.warnings.extend(errors);
                }
            }
        }

        // Phase 2: build the watch handler function bodies using the registrations
        // created in phase 1.  Because `self.watch_handlers` is already populated,
        // any recursive state mutation inside a watch body would also trigger the
        // appropriate handlers (depth-first, in declaration order).
        for (target_name, fn_name, symbol_id, block_idx) in &watch_block_registrations {
            let watch_block = &tast.watch_blocks[*block_idx];

            debug!(
                target = %target_name,
                symbol_id = symbol_id.0,
                fn_name = %fn_name,
                "Lowering watch block body to MIR function (phase 2)"
            );

            // Construct a synthetic TastFunction wrapping the watch body.
            let synthetic_function = TastFunction {
                symbol_id: *symbol_id,
                name: fn_name.clone(),
                parameters: Vec::new(),
                return_type: ConcreteType::Undefined,
                body: watch_block.body.clone(),
                generic_params: Vec::new(),
                constraints: Vec::new(),
                is_background: false,
                is_static: false,
                visibility: crate::typechecker::tast::Visibility::Private,
                location: watch_block.location.clone(),
            };

            match self.build_function(synthetic_function) {
                Ok(mir_function) => {
                    mir_program
                        .symbol_name_map
                        .insert(*symbol_id, fn_name.clone());
                    mir_program.functions.insert(*symbol_id, mir_function);
                    self.stats.functions_lowered += 1;
                    debug!(
                        target = %target_name,
                        symbol_id = symbol_id.0,
                        "Watch handler function built and registered"
                    );
                }
                Err(errors) => {
                    warn!(
                        target = %target_name,
                        error_count = errors.len(),
                        "Failed to lower watch block body"
                    );
                    self.warnings.extend(errors);
                }
            }
        }

        // Lower computed state property getter functions (Phase 2).
        //
        // SymbolIds and the `self.computed_properties` map were pre-registered in Phase 1
        // above (before function building).  Here we build the actual getter function bodies
        // using the pre-allocated SymbolIds so the symbol_name_map and function table stay
        // consistent.
        if let Some(ref state_block) = tast.state {
            // Clone the computed declarations to release the borrow on `tast.state`
            // before calling `self.build_function()`.
            let computed_decls = state_block.computed.clone();
            for ((comp_name, fn_name, symbol_id), comp_decl) in
                computed_registrations.iter().zip(computed_decls.iter())
            {
                debug!(
                    computed_name = %comp_name,
                    fn_name = %fn_name,
                    symbol_id = symbol_id.0,
                    return_type = ?comp_decl.computed_type,
                    "Lowering computed property to MIR getter function (phase 2)"
                );

                // Build a synthetic TastFunction wrapping the computed body.
                let synthetic_function = TastFunction {
                    symbol_id: *symbol_id,
                    name: fn_name.clone(),
                    parameters: Vec::new(),
                    return_type: comp_decl.computed_type.clone(),
                    body: comp_decl.body.clone(),
                    generic_params: Vec::new(),
                    constraints: Vec::new(),
                    is_background: false,
                    is_static: false,
                    visibility: Visibility::Private,
                    location: comp_decl.location.clone(),
                };

                match self.build_function(synthetic_function) {
                    Ok(mir_function) => {
                        mir_program
                            .symbol_name_map
                            .insert(*symbol_id, fn_name.clone());
                        mir_program.functions.insert(*symbol_id, mir_function);
                        self.stats.functions_lowered += 1;

                        debug!(
                            computed_name = %comp_name,
                            symbol_id = symbol_id.0,
                            "Computed getter function built and registered"
                        );
                    }
                    Err(errors) => {
                        warn!(
                            computed_name = %comp_name,
                            error_count = errors.len(),
                            "Failed to lower computed property body"
                        );
                        self.warnings.extend(errors);
                    }
                }
            }
        }

        // Transfer string pool
        mir_program.string_pool = self.string_pool.clone();

        // Add state variables to MirProgram.globals
        // If TAST has state declarations, use them with const evaluation
        // Otherwise fall back to pre-registered state variables
        if let Some(ref state_block) = tast.state {
            for decl in &state_block.declarations {
                let mir_type = MirType::from_concrete_type(&decl.state_type);

                // Try to evaluate the initializer at compile time
                let const_val = try_const_eval_tast(&decl.initializer);

                // Convert ConstValue to MirConstant, or use default if not const-evaluable
                // Note: MirConstant::Integer takes i64, codegen handles i32 truncation
                let initializer = match (&const_val, &mir_type) {
                    (ConstValue::Integer(n), MirType::I32) => Some(MirConstant::Integer(*n)),
                    (ConstValue::Integer(n), MirType::F64) => Some(MirConstant::Float(*n as f64)),
                    (ConstValue::Float(f), MirType::F64) => Some(MirConstant::Float(*f)),
                    (ConstValue::Float(f), MirType::I32) => Some(MirConstant::Integer(*f as i64)),
                    (ConstValue::Boolean(b), MirType::I32 | MirType::Bool) => {
                        Some(MirConstant::Integer(if *b { 1 } else { 0 }))
                    }
                    // Strings require runtime initialization - use 0 (null ptr) as placeholder
                    (ConstValue::String(_), _) => Some(MirConstant::Integer(0)),
                    // Non-const expressions need runtime init - use default zero
                    (ConstValue::NotConst, MirType::I32 | MirType::Bool) => {
                        Some(MirConstant::Integer(0))
                    }
                    (ConstValue::NotConst, MirType::F64) => Some(MirConstant::Float(0.0)),
                    // Default case (strings as Ptr, etc)
                    _ => Some(MirConstant::Integer(0)),
                };

                let is_const = const_val.is_const()
                    && !matches!(const_val, ConstValue::String(_))
                    && !matches!(const_val, ConstValue::NotConst);

                let global = MirGlobal {
                    symbol_id: decl.symbol_id,
                    name: decl.name.clone(),
                    global_type: mir_type.clone(),
                    initializer,
                    is_mutable: true, // State variables are mutable
                    location: decl.location.clone(),
                };
                mir_program.globals.insert(decl.symbol_id, global);

                trace!(
                    name = %decl.name,
                    symbol_id = ?decl.symbol_id,
                    mir_type = ?mir_type,
                    const_evaluated = is_const,
                    "Added state variable to MirProgram.globals with const evaluation"
                );
            }
        } else {
            // Fallback: use pre-registered state variables (legacy path)
            for (name, (symbol_id, mir_type)) in &self.state_variables {
                let global = MirGlobal {
                    symbol_id: *symbol_id,
                    name: name.clone(),
                    global_type: mir_type.clone(),
                    initializer: match mir_type {
                        MirType::I32 => Some(MirConstant::Integer(0)),
                        MirType::F64 => Some(MirConstant::Float(0.0)),
                        _ => Some(MirConstant::Integer(0)),
                    },
                    is_mutable: true,
                    location: SourceLocation::new(0, 0, ""),
                };
                mir_program.globals.insert(*symbol_id, global);
                trace!(
                    name = %name,
                    symbol_id = ?symbol_id,
                    mir_type = ?mir_type,
                    "Added state variable to MirProgram.globals (legacy path)"
                );
            }
        }

        // Record build time
        self.stats.build_time_us = start_time.elapsed().as_micros() as u64;

        // Return early if there are critical errors
        let critical_errors: Vec<_> = self
            .warnings
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    CompilerError::Type { .. } | CompilerError::Validation { .. }
                )
            })
            .cloned()
            .collect();

        if !critical_errors.is_empty() {
            return Err(critical_errors);
        }

        // Convert external functions (WASM imports)
        mir_program.externals = tast
            .externals
            .iter()
            .enumerate()
            .map(
                |(ext_idx, ext)| crate::mir::mir_types::MirExternalFunction {
                    name: ext.name.clone(),
                    parameters: ext
                        .parameters
                        .iter()
                        .enumerate()
                        .map(|(param_idx, p)| MirParameter {
                            value_id: ValueId(10000 + ext_idx * 100 + param_idx),
                            name: p.name.clone(),
                            param_type: MirType::from_concrete_type(&p.param_type),
                            location: p.location.clone(),
                        })
                        .collect(),
                    return_type: MirType::from_concrete_type(&ext.return_type),
                    module: ext.module.clone(),
                    location: ext.location.clone(),
                },
            )
            .collect();

        Ok(MirBuildResult {
            program: mir_program,
            warnings: std::mem::take(&mut self.warnings),
            stats: std::mem::take(&mut self.stats),
        })
    }
}

impl Default for MirBuilder {
    fn default() -> Self {
        // Create empty symbol table for default initialization
        let empty_symbol_table = std::sync::Arc::new(crate::resolver::GlobalSymbolTable::new());
        Self::new(empty_symbol_table)
    }
}
