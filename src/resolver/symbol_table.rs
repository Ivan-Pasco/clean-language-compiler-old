//! Symbol Table Implementation for Name Resolution
//!
//! This module provides symbol table functionality for tracking and resolving
//! symbols (variables, functions, classes, methods, fields) across different scopes.

use crate::ast::SourceLocation;
use crate::hir::*;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// Unique identifier for symbols.
///
/// `PartialOrd`/`Ord` are required so BTreeMap<SymbolId, _> works. MirProgram
/// uses BTreeMap keyed by SymbolId for functions / globals / symbol_name_map
/// so codegen sees these in a deterministic sorted order, independent of
/// per-process HashMap hasher seeds. Without that, the compiler emits
/// different WASM bytes on every recompile of the same source — the
/// root cause of CODEGEN-STRING-ARG-ALIAS-JSONGET's flaky
/// tasks_list_page memory-access-out-of-bounds symptom (some recompiles
/// land memory layouts that happen to trap, others don't).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolId(pub usize);

// ============================================================================
// Reserved SymbolId ranges for MIR-synthesized symbols
// ============================================================================
//
// These SymbolIds are allocated by the MIR builder (NOT by the resolver) for
// stdlib operations, watch handlers, computed getters, and class serializers
// that need a stable SymbolId in the call graph.
//
// The resolver allocates SymbolIds sequentially starting at 0. To guarantee
// that resolver-assigned IDs NEVER collide with MIR-synthesized IDs, each
// reserved range uses a base far above any plausible resolver allocation.
// A previous version used SymbolId(1000..1011), which collided once a program
// loaded enough plugin bridge functions to push the resolver counter past 1000;
// the resolver-assigned ID overwrote `symbol_name_map[1011]` and class
// serializers ended up calling whatever user function happened to land on
// SymbolId(1011) instead of `__json_quote_string` (fingerprints CODEGEN_F64
// bf9fc9cf8a8e and CG-LOCAL29 follow-ups).

/// Base for MIR-synthesized stdlib builtins.
/// Reserves SYNTHETIC_BUILTIN_BASE..SYNTHETIC_BUILTIN_BASE+1024.
pub const SYNTHETIC_BUILTIN_BASE: usize = 0x4000_0000;

pub const SYM_BUILTIN_STRING_CONCAT: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE);
pub const SYM_BUILTIN_MATH_POW_F64: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 1);
/// `integer ^ integer`: exact integer power (`math.pow.i32`).
pub const SYM_BUILTIN_MATH_POW_I32: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 2);
pub const SYM_BUILTIN_LIST_ALLOCATE: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 3);
pub const SYM_BUILTIN_LIST_PUSH: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 4);
pub const SYM_BUILTIN_LIST_PUSH_F64: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 5);
pub const SYM_BUILTIN_LIST_SIZE: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 6);
pub const SYM_BUILTIN_LIST_ADD: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 7);
pub const SYM_BUILTIN_LIST_ADD_F64: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 8);
// SYNTHETIC_BUILTIN_BASE + 9..=11 were the string helpers of the removed
// in-WASM class JSON serializer; class JSON now goes through the bridges.
/// String concat that allocates the result through `__transient_alloc`
/// instead of `__malloc`. Used by the MIR builder when a `+`/`StringConcat`
/// result will be consumed inside an accumulator-rewritten loop body's
/// transient scope (established by `__transient_scope_enter` /
/// `__transient_scope_exit`). Codegen maps this symbol to
/// `__string_concat_transient` (see `codegen_registration.rs`).
///
/// Callers MUST guarantee the result does not escape the surrounding
/// transient scope. Lexically: only emitted when the destination is a
/// body-local symbol whose lifetime ends inside the loop body. If the
/// result escapes the scope (e.g. is stored in an outer-scope variable),
/// it will dangle on the next `__transient_scope_exit`.
pub const SYM_BUILTIN_STRING_CONCAT_TRANSIENT: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 12);
/// `list.add` for `list<integer:64>` (8-byte i64 elements).
pub const SYM_BUILTIN_LIST_ADD_I64: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 13);
/// `list.add` for `list<string>` (a `.unique` list compares strings by bytes).
pub const SYM_BUILTIN_LIST_ADD_STRING: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 14);
/// `integer:64 ^ integer`: exact integer power on i64 (`math.pow.i64`).
pub const SYM_BUILTIN_MATH_POW_I64: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 15);
/// `list.add` for a list of optional primitives (a `.unique` list compares
/// the values, not the boxes).
pub const SYM_BUILTIN_LIST_ADD_BOXED: SymbolId = SymbolId(SYNTHETIC_BUILTIN_BASE + 16);

/// Base for MIR-synthesized watch handler IDs (one per watch block target).
pub const SYM_WATCH_HANDLER_BASE: usize = 0x4001_0000;

/// Base for MIR-synthesized computed-property getter IDs.
pub const SYM_COMPUTED_GETTER_BASE: usize = 0x4002_0000;

/// Base for MIR-synthesized class JSON serializer IDs.
pub const SYM_CLASS_SERIALIZER_BASE: usize = 0x4003_0000;

/// Base for MIR-synthesized async task functions (one per `background` /
/// `later` call site, `mir_builder/async_tasks.rs`).
pub const SYM_ASYNC_TASK_BASE: usize = 0x4004_0000;

/// Unique identifier for scopes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub usize);

/// One method entry inside a capability contract, as stored on
/// `SymbolKind::Capability`. Slot index in the containing vec is the
/// stable dispatch slot used by codegen vtables (Stage 8).
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityMethodEntry {
    pub name: String,
    pub parameters: Vec<HirType>,
    pub return_type: HirType,
}

/// Symbol kinds for type checking and usage validation
#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Variable {
        var_type: HirType,
        is_mutable: bool,
    },
    Parameter {
        param_type: HirType,
    },
    Function {
        parameters: Vec<HirType>,
        return_type: Option<HirType>,
    },
    Class {
        fields: Vec<SymbolId>,
        methods: Vec<SymbolId>,
        parent: Option<SymbolId>,
    },
    Method {
        class_id: SymbolId,
        parameters: Vec<HirType>,
        return_type: HirType,
    },
    Field {
        class_id: SymbolId,
        field_type: HirType,
    },
    Constructor {
        class_id: SymbolId,
        parameters: Vec<HirType>,
    },
    Namespace {
        functions: Vec<SymbolId>,
    },
    /// A capability declaration (`can Name:`) — a nominal contract of methods
    /// that classes may claim via `can Name` on their class header.
    /// See Clean Language Specification §Capabilities and semantic-rules.md SEM011–SEM013.
    Capability {
        /// Method contract entries in source order. Slot index in this vec is
        /// stable and used by codegen to build per-class vtables (Stage 8).
        methods: Vec<CapabilityMethodEntry>,
    },
    /// State variable - persistent global variable with optional guard
    StateVariable {
        var_type: HirType,
        scope: crate::hir::HirStateScope, // App or Screen scope
        has_guard: bool,                  // Whether the state has a guard clause
        /// true when declared inside `computed:` — read-only, assignment is STATE004 error.
        /// spec/semantic-rules.md STATE004: "Cannot assign to computed state variable"
        is_computed: bool,
        /// For Screen-scoped state, the name of the screen that owns this variable.
        /// Used to enforce SCOPE005: screen-local state cannot be accessed outside its screen.
        screen_name: Option<String>,
    },
}

/// `Symbol::owner_scope_name` of a private module-level function or state
/// variable: it is private to the file that declares it (SEM005).
pub const MODULE_OWNER: &str = "<module>";

/// Symbol information stored in the symbol table
#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub scope_id: ScopeId,
    pub location: SourceLocation,
    /// True when the symbol was declared outside a `public:` sub-section
    /// (declarations are private by default, semantic-rules.md §SEM005).
    ///
    /// The type checker reports SEM005 for a private class field or method
    /// used outside its class.
    pub is_private: bool,
    /// The name of the class (or [`MODULE_OWNER`]) that owns this private symbol.
    /// Used by the type checker to determine whether access is permitted.
    /// `None` for public symbols (the field is irrelevant in that case).
    pub owner_scope_name: Option<String>,
}

/// Scope information for nested scoping
#[derive(Debug, Clone)]
pub struct Scope {
    pub id: ScopeId,
    pub parent: Option<ScopeId>,
    pub symbols: HashMap<String, SymbolId>,
}

/// Global symbol table managing all symbols and scopes
#[derive(Debug)]
pub struct GlobalSymbolTable {
    symbols: HashMap<SymbolId, Symbol>,
    scopes: HashMap<ScopeId, Scope>,

    // Generators for unique IDs
    next_symbol_id: usize,
    next_scope_id: usize,

    // Current context
    current_scope: ScopeId,

    // Built-in symbols (populated during initialization)
    pub(crate) builtins: HashSet<SymbolId>,

    /// Accumulated SCOPE003 errors (scope depth exceeded). Drained by the resolver.
    pub(crate) scope_depth_errors: Mutex<Vec<String>>,

    /// Plugin functions (bridge and language names) with `ptr` parameters,
    /// and those parameters' positions. A `ptr` argument is passed as is —
    /// a string or an integer byte handle (type-system.md §9b) — never
    /// boxed, although the parameter type-checks as `any`.
    pub(crate) raw_ptr_params: HashMap<String, Vec<usize>>,
}

impl Clone for GlobalSymbolTable {
    fn clone(&self) -> Self {
        Self {
            symbols: self.symbols.clone(),
            scopes: self.scopes.clone(),
            next_symbol_id: self.next_symbol_id,
            next_scope_id: self.next_scope_id,
            current_scope: self.current_scope,
            builtins: self.builtins.clone(),
            scope_depth_errors: Mutex::new(self.scope_depth_errors.lock().unwrap().clone()),
            raw_ptr_params: self.raw_ptr_params.clone(),
        }
    }
}

impl GlobalSymbolTable {
    /// Whether argument `index` of the plugin function `name` is a `ptr`
    /// parameter (see `raw_ptr_params`).
    pub fn is_raw_ptr_param(&self, name: &str, index: usize) -> bool {
        self.raw_ptr_params
            .get(name)
            .is_some_and(|positions| positions.contains(&index))
    }

    /// Create a new global symbol table with built-in symbols
    pub fn new() -> Self {
        let mut table = Self {
            symbols: HashMap::new(),
            scopes: HashMap::new(),
            next_symbol_id: 0,
            next_scope_id: 0,
            current_scope: ScopeId(0),
            builtins: HashSet::new(),
            scope_depth_errors: Mutex::new(Vec::new()),
            raw_ptr_params: HashMap::new(),
        };

        // Create global scope
        table.create_scope(None);

        // Add built-in symbols
        table.add_builtins();

        table
    }

    /// Add built-in symbols to the global scope
    fn add_builtins(&mut self) {
        let global_scope = self.current_scope;

        // Built-in functions
        let builtin_functions = vec![
            ("print", vec![HirType::String], Some(HirType::Void)),
            ("println", vec![HirType::String], Some(HirType::Void)),
            ("printl", vec![HirType::String], Some(HirType::Void)),
            ("console_log", vec![HirType::String], Some(HirType::Void)),
            ("console_error", vec![HirType::String], Some(HirType::Void)),
            ("console_warn", vec![HirType::String], Some(HirType::Void)),
            ("abs", vec![HirType::Number], Some(HirType::Number)),
            (
                "max",
                vec![HirType::Number, HirType::Number],
                Some(HirType::Number),
            ),
            (
                "min",
                vec![HirType::Number, HirType::Number],
                Some(HirType::Number),
            ),
            ("sqrt", vec![HirType::Number], Some(HirType::Number)),
            (
                "pow",
                vec![HirType::Number, HirType::Number],
                Some(HirType::Number),
            ),
            ("toString", vec![HirType::Integer], Some(HirType::String)),
            ("toInteger", vec![HirType::String], Some(HirType::Integer)),
            // error(message) — raise a runtime error. Returns string so it can appear
            // in expression context (e.g. test assertions: expr = error("msg")).
            ("error", vec![HirType::String], Some(HirType::String)),
        ];

        for (name, params, return_type) in builtin_functions {
            let symbol_id = self.create_symbol(
                name.to_string(),
                SymbolKind::Function {
                    parameters: params,
                    return_type,
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                    byte_start: None,
                    byte_end: None,
                },
            );

            self.builtins.insert(symbol_id);
        }

        // Built-in static classes with methods
        self.add_builtin_static_classes(global_scope);

        // Built-in namespace functions (e.g., math_sin for math.sin())
        self.add_builtin_namespace_functions(global_scope);

        // Built-in namespace identifiers (so math.max() syntax works)
        self.add_builtin_namespace_identifiers(global_scope);

        // Built-in types are handled differently - they're part of HirType enum
        // Built-in methods are resolved dynamically based on receiver type
    }

    /// Add built-in static classes (Integer only - Math, StringUtils, String removed)
    fn add_builtin_static_classes(&mut self, global_scope: ScopeId) {
        // Integer class with static methods
        let integer_class_id = self.create_symbol(
            "Integer".to_string(),
            SymbolKind::Class {
                fields: Vec::new(),
                methods: Vec::new(),
                parent: None,
            },
            global_scope,
            SourceLocation {
                file: "<builtin>".to_string(),
                line: 0,
                column: 0,
                byte_start: None,
                byte_end: None,
            },
        );

        let integer_methods = vec![("parse", vec![HirType::String], HirType::Integer)];

        let mut integer_method_ids = Vec::new();
        for (method_name, params, return_type) in integer_methods {
            let method_id = self.create_symbol(
                method_name.to_string(),
                SymbolKind::Method {
                    class_id: integer_class_id,
                    parameters: params,
                    return_type,
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                    byte_start: None,
                    byte_end: None,
                },
            );
            integer_method_ids.push(method_id);
            self.builtins.insert(method_id);
        }

        // Update Integer class with methods
        if let Some(integer_symbol) = self.symbols.get_mut(&integer_class_id) {
            if let SymbolKind::Class { methods, .. } = &mut integer_symbol.kind {
                *methods = integer_method_ids;
            }
        }
        self.builtins.insert(integer_class_id);
    }

    /// Generate a new unique symbol ID
    fn next_symbol_id(&mut self) -> SymbolId {
        let id = SymbolId(self.next_symbol_id);
        self.next_symbol_id += 1;
        id
    }

    /// Generate a new unique scope ID
    fn next_scope_id(&mut self) -> ScopeId {
        let id = ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        id
    }

    /// Create a new scope
    pub fn create_scope(&mut self, parent: Option<ScopeId>) -> ScopeId {
        let scope_id = self.next_scope_id();

        // Prevent circular reference: scope should never be its own parent
        let parent_scope = match parent {
            Some(p) => {
                // Ensure parent is not the same as the new scope (should be impossible but safety check)
                if p != scope_id {
                    Some(p)
                } else {
                    None
                }
            }
            None => {
                // Use current scope as parent unless it would create a circular reference
                if self.current_scope != scope_id {
                    Some(self.current_scope)
                } else {
                    None
                }
            }
        };

        let scope = Scope {
            id: scope_id,
            parent: parent_scope,
            symbols: HashMap::new(),
        };

        self.scopes.insert(scope_id, scope);
        scope_id
    }

    /// Enter a scope (set as current)
    pub fn enter_scope(&mut self, scope_id: ScopeId) {
        self.current_scope = scope_id;
    }

    /// Exit current scope (return to parent)
    pub fn exit_scope(&mut self) {
        if let Some(scope) = self.scopes.get(&self.current_scope) {
            if let Some(parent_id) = scope.parent {
                self.current_scope = parent_id;
            }
        }
    }

    /// Create a new symbol and add it to the current scope
    pub fn create_symbol(
        &mut self,
        name: String,
        kind: SymbolKind,
        scope_id: ScopeId,
        location: SourceLocation,
    ) -> SymbolId {
        let symbol_id = self.next_symbol_id();
        let symbol = Symbol {
            id: symbol_id,
            name: name.clone(),
            kind,
            scope_id,
            location,
            is_private: false,
            owner_scope_name: None,
        };

        self.symbols.insert(symbol_id, symbol);

        // Add to scope
        if let Some(scope) = self.scopes.get_mut(&scope_id) {
            scope.symbols.insert(name, symbol_id);
        }

        symbol_id
    }

    /// Look up a symbol by name only in the *current* scope (no parent traversal).
    /// Used to detect SCOPE002 (RedeclarationInScope).
    pub fn lookup_symbol_in_current_scope(&self, name: &str) -> Option<SymbolId> {
        self.scopes
            .get(&self.current_scope)
            .and_then(|scope| scope.symbols.get(name).copied())
    }

    /// Look up a symbol by name in the current scope and parent scopes
    pub fn lookup_symbol(&self, name: &str) -> Option<SymbolId> {
        self.lookup_symbol_in_scope(name, self.current_scope)
    }

    /// Drain and return any SCOPE003 errors accumulated during symbol lookup.
    /// Call this after each resolution pass to surface depth-exceeded errors.
    pub fn take_scope_depth_errors(&self) -> Vec<String> {
        self.scope_depth_errors.lock().unwrap().drain(..).collect()
    }

    /// Look up a symbol by name starting from a specific scope
    pub fn lookup_symbol_in_scope(&self, name: &str, scope_id: ScopeId) -> Option<SymbolId> {
        self.lookup_symbol_in_scope_with_depth(name, scope_id, 0)
    }

    fn lookup_symbol_in_scope_with_depth(
        &self,
        name: &str,
        scope_id: ScopeId,
        depth: usize,
    ) -> Option<SymbolId> {
        const MAX_SCOPE_DEPTH: usize = 50; // Prevent infinite recursion in scope chains

        if depth > MAX_SCOPE_DEPTH {
            let msg = format!(
                "SCOPE003: Maximum scope nesting depth ({}) exceeded while looking up symbol '{}'",
                MAX_SCOPE_DEPTH, name
            );
            tracing::warn!("{}", msg);
            self.scope_depth_errors.lock().unwrap().push(msg);
            return None;
        }

        if let Some(scope) = self.scopes.get(&scope_id) {
            // Check current scope
            if let Some(&symbol_id) = scope.symbols.get(name) {
                return Some(symbol_id);
            }

            // Check parent scope
            if let Some(parent_id) = scope.parent {
                return self.lookup_symbol_in_scope_with_depth(name, parent_id, depth + 1);
            }
        }
        None
    }

    /// Look up a symbol in a specific class scope (for method/field access)
    pub fn lookup_class_member(&self, class_id: SymbolId, member_name: &str) -> Option<SymbolId> {
        if let Some(symbol) = self.symbols.get(&class_id) {
            if let SymbolKind::Class {
                fields, methods, ..
            } = &symbol.kind
            {
                // Check fields
                for &field_id in fields {
                    if let Some(field_symbol) = self.symbols.get(&field_id) {
                        if field_symbol.name == member_name {
                            return Some(field_id);
                        }
                    }
                }

                // Check methods
                for &method_id in methods {
                    if let Some(method_symbol) = self.symbols.get(&method_id) {
                        if method_symbol.name == member_name {
                            return Some(method_id);
                        }
                    }
                }

                // Check parent class if exists
                if let SymbolKind::Class {
                    parent: Some(parent_id),
                    ..
                } = &symbol.kind
                {
                    return self.lookup_class_member(*parent_id, member_name);
                }
            }
        }
        None
    }

    /// Find a field declared by `class_id` or any of its ancestors, nearest
    /// class first. Returns the field's symbol and declared type.
    pub fn lookup_class_field(
        &self,
        class_id: SymbolId,
        field_name: &str,
    ) -> Option<(SymbolId, &HirType)> {
        let mut visited = std::collections::HashSet::new();
        let mut current = Some(class_id);
        while let Some(id) = current {
            if !visited.insert(id) {
                return None;
            }
            let SymbolKind::Class { fields, parent, .. } = &self.symbols.get(&id)?.kind else {
                return None;
            };
            for &field_id in fields {
                if let Some(Symbol {
                    name,
                    kind: SymbolKind::Field { field_type, .. },
                    ..
                }) = self.symbols.get(&field_id)
                {
                    if name == field_name {
                        return Some((field_id, field_type));
                    }
                }
            }
            current = *parent;
        }
        None
    }

    /// Get symbol by ID
    pub fn get_symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.get(&id)
    }

    /// Get mutable symbol by ID
    pub fn get_symbol_mut(&mut self, id: SymbolId) -> Option<&mut Symbol> {
        self.symbols.get_mut(&id)
    }

    /// Check if a symbol is a built-in
    pub fn is_builtin(&self, symbol_id: SymbolId) -> bool {
        self.builtins.contains(&symbol_id)
    }

    /// Mark a symbol as builtin
    pub fn mark_as_builtin(&mut self, symbol_id: SymbolId) {
        self.builtins.insert(symbol_id);
    }

    /// Mark a symbol as private and record the name of its owning scope
    /// (the class name for class members, or [`MODULE_OWNER`] for module-level private items).
    ///
    /// Called by the resolver after creating a symbol declared outside a `public:` sub-section.
    pub fn mark_as_private(&mut self, symbol_id: SymbolId, owner_scope_name: String) {
        if let Some(symbol) = self.symbols.get_mut(&symbol_id) {
            symbol.is_private = true;
            symbol.owner_scope_name = Some(owner_scope_name);
        }
    }

    /// Get current scope ID
    pub fn current_scope_id(&self) -> ScopeId {
        self.current_scope
    }

    /// Check for symbol conflicts in current scope
    pub fn has_symbol_in_current_scope(&self, name: &str) -> bool {
        if let Some(scope) = self.scopes.get(&self.current_scope) {
            scope.symbols.contains_key(name)
        } else {
            false
        }
    }

    /// Find all accessible symbols (for completion/suggestion purposes)
    pub fn accessible_symbols(&self) -> Vec<SymbolId> {
        let mut symbols = Vec::new();
        let mut current_scope_id = Some(self.current_scope);

        while let Some(scope_id) = current_scope_id {
            if let Some(scope) = self.scopes.get(&scope_id) {
                symbols.extend(scope.symbols.values());
                current_scope_id = scope.parent;
            } else {
                break;
            }
        }

        symbols
    }

    /// Get all symbols in the symbol table (for MIR building)
    /// Returns a reference to the internal symbols HashMap
    pub fn all_symbols(&self) -> &HashMap<SymbolId, Symbol> {
        &self.symbols
    }

    /// Add built-in namespace functions (e.g., math.sin for math.sin calls)
    /// NOTE: Use dot notation to match WASM function names
    fn add_builtin_namespace_functions(&mut self, global_scope: ScopeId) {
        let namespace_functions = vec![
            // Math namespace functions - using dot notation to match WASM
            ("math.sin", vec![HirType::Number], HirType::Number),
            ("math.cos", vec![HirType::Number], HirType::Number),
            ("math.tan", vec![HirType::Number], HirType::Number),
            ("math.asin", vec![HirType::Number], HirType::Number),
            ("math.acos", vec![HirType::Number], HirType::Number),
            ("math.atan", vec![HirType::Number], HirType::Number),
            (
                "math.atan2",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            ("math.sinh", vec![HirType::Number], HirType::Number),
            ("math.cosh", vec![HirType::Number], HirType::Number),
            ("math.tanh", vec![HirType::Number], HirType::Number),
            ("math.log", vec![HirType::Number], HirType::Number),
            ("math.ln", vec![HirType::Number], HirType::Number),
            ("math.log10", vec![HirType::Number], HirType::Number),
            ("math.log2", vec![HirType::Number], HirType::Number),
            ("math.exp", vec![HirType::Number], HirType::Number),
            ("math.exp2", vec![HirType::Number], HirType::Number),
            ("math.abs", vec![HirType::Number], HirType::Number),
            ("math.floor", vec![HirType::Number], HirType::Number),
            ("math.ceil", vec![HirType::Number], HirType::Number),
            ("math.round", vec![HirType::Number], HirType::Number),
            ("math.sqrt", vec![HirType::Number], HirType::Number),
            ("math.trunc", vec![HirType::Number], HirType::Number),
            ("math.pi", vec![], HirType::Number),
            ("math.e", vec![], HirType::Number),
            ("math.tau", vec![], HirType::Number),
            (
                "math.pow",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math.max",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            (
                "math.min",
                vec![HirType::Number, HirType::Number],
                HirType::Number,
            ),
            // math.random / math.sign — Layer 2 bridges; registered in
            // foundation/platform-architecture/function-registry.toml as
            // `math_random` / `math_sign` with `aliases = ["math.random"]` /
            // `aliases = ["math.sign"]`. spec/stdlib-reference.md §2.
            ("math.random", vec![], HirType::Number),
            ("math.sign", vec![HirType::Number], HirType::Number),
            // String namespace functions
            // Use string.length to match semantic analyzer and codegen
            ("string.length", vec![HirType::String], HirType::Integer),
            (
                "string.concat",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            ("string.isEmpty", vec![HirType::String], HirType::Boolean),
            ("string.isBlank", vec![HirType::String], HirType::Boolean),
            (
                "string.indexOf",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            (
                "string.lastIndexOf",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            (
                "string.substring",
                vec![HirType::String, HirType::Integer, HirType::Integer],
                HirType::String,
            ),
            ("string.toUpperCase", vec![HirType::String], HirType::String),
            ("string.toLowerCase", vec![HirType::String], HirType::String),
            ("string.trim", vec![HirType::String], HirType::String),
            ("string.trimStart", vec![HirType::String], HirType::String),
            ("string.trimEnd", vec![HirType::String], HirType::String),
            (
                "string.split",
                vec![HirType::String, HirType::String],
                HirType::List(Box::new(HirType::String)),
            ),
            (
                "string.join",
                vec![HirType::List(Box::new(HirType::String)), HirType::String],
                HirType::String,
            ),
            (
                "string.padStart",
                vec![HirType::String, HirType::Integer, HirType::String],
                HirType::String,
            ),
            (
                "string.padEnd",
                vec![HirType::String, HirType::Integer, HirType::String],
                HirType::String,
            ),
            (
                "string.charAt",
                vec![HirType::String, HirType::Integer],
                HirType::String,
            ),
            (
                "string.charCodeAt",
                vec![HirType::String, HirType::Integer],
                HirType::Integer,
            ),
            (
                "string.fromCharCode",
                vec![HirType::Integer],
                HirType::String,
            ),
            (
                "string.contains",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string.startsWith",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string.endsWith",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "string.replace",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "string.replaceAll",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            // List namespace functions
            // Both list.size and list.length are supported; length is canonical per spec
            (
                "list.size",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Integer,
            ),
            (
                "list.length",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Integer,
            ),
            (
                "list.isEmpty",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Boolean,
            ),
            (
                "list.isNotEmpty",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Boolean,
            ),
            (
                "list.push",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Void,
            ),
            (
                "list.pop",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.removeLast",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.get",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Integer],
                HirType::Number,
            ),
            (
                "list.remove",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Integer],
                HirType::Number,
            ),
            (
                "list.peek",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.setFlags",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Integer],
                HirType::Void,
            ),
            (
                "list.add",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::List(Box::new(HirType::Number)), // Returns modified list per spec
            ),
            (
                "list.contains",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Boolean,
            ),
            (
                "list.insert",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::Integer,
                    HirType::Number,
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.indexOf",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Integer,
            ),
            (
                "list.lastIndexOf",
                vec![HirType::List(Box::new(HirType::Number)), HirType::Number],
                HirType::Integer,
            ),
            // Byte-comparing variants for `list<string>` — dispatched from
            // MIR when the receiver's element type is String. See prompt
            // baf3ccc5 for why `list.contains` on strings can't share the
            // pointer-equality body used for numeric lists.
            (
                "list.contains.string",
                vec![HirType::List(Box::new(HirType::String)), HirType::String],
                HirType::Boolean,
            ),
            (
                "list.indexOf.string",
                vec![HirType::List(Box::new(HirType::String)), HirType::String],
                HirType::Integer,
            ),
            // Value-comparing variants for a list of optional primitives
            // (`list<integer?>`): the elements are boxes.
            (
                "list.contains.boxed",
                vec![HirType::List(Box::new(HirType::Any)), HirType::Any],
                HirType::Boolean,
            ),
            (
                "list.indexOf.boxed",
                vec![HirType::List(Box::new(HirType::Any)), HirType::Any],
                HirType::Integer,
            ),
            (
                "list.first",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.last",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::Number,
            ),
            (
                "list.reverse",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.sort",
                vec![HirType::List(Box::new(HirType::Number))],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.slice",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::Integer,
                    HirType::Integer,
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.concat",
                vec![
                    HirType::List(Box::new(HirType::Number)),
                    HirType::List(Box::new(HirType::Number)),
                ],
                HirType::List(Box::new(HirType::Number)),
            ),
            (
                "list.fill",
                vec![
                    HirType::Integer, // size parameter
                    HirType::Number,  // value parameter (generic placeholder)
                ],
                HirType::List(Box::new(HirType::Number)), // returns new list
            ),
            (
                "list.range",
                vec![HirType::Integer, HirType::Integer],
                HirType::List(Box::new(HirType::Integer)),
            ),
            (
                "list.join",
                vec![HirType::List(Box::new(HirType::Number)), HirType::String],
                HirType::String,
            ),
            // HTTP namespace functions
            ("http.get", vec![HirType::String], HirType::String),
            (
                "http.post",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.put",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.patch",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            ("http.delete", vec![HirType::String], HirType::String),
            // Extended HTTP client functions
            ("http.head", vec![HirType::String], HirType::String),
            ("http.options", vec![HirType::String], HirType::String),
            (
                "http.postJson",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.putJson",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.postForm",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.getWithHeaders",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.postWithHeaders",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            ("http.encodeUrl", vec![HirType::String], HirType::String),
            ("http.decodeUrl", vec![HirType::String], HirType::String),
            ("http.buildQuery", vec![HirType::String], HirType::String),
            // Layer 3: HTTP response functions (server-only)
            (
                "http.respond",
                vec![HirType::Integer, HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "http.redirect",
                vec![HirType::Integer, HirType::String],
                HirType::String,
            ),
            (
                "http.setHeader",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            // http.setCache / http.noCache removed in 0.30.301 — now provided by
            // frame.server plugin.toml `[[functions]]` with `returns = "boolean"`.
            // Language type changes from Integer → Boolean (matches the actual
            // bridge ABI). Per BUILTIN-NAMESPACE-OVERREACH Sub-A finalize.
            // File namespace functions
            ("file.read", vec![HirType::String], HirType::String),
            (
                "file.write",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "file.append",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            ("file.exists", vec![HirType::String], HirType::Boolean),
            ("file.delete", vec![HirType::String], HirType::Boolean),
            // fs.write_bytes(path, handle) — opaque-handle bytes writer.
            // Handle is an integer per spec/type-system.md §9b (pointer to
            // a [4-byte LE length][bytes] buffer). Returns integer error
            // code (0 = success; see spec/plugins/frame-server.ebnf).
            (
                "fs.write_bytes",
                vec![HirType::String, HirType::Integer],
                HirType::Integer,
            ),
            // req.body_bytes() — opaque byte handle for the raw request body.
            // Additive to req.body (UTF-8 string). Handle typed as integer
            // until a bytes primitive lands.
            ("req.body_bytes", vec![], HirType::Integer),
            // crypto.sha256_bytes(handle) — SHA-256 of the length-prefixed byte
            // buffer at the opaque handle. Handle is an integer per
            // spec/type-system.md §9b (pointer to a [4-byte LE length][bytes]
            // buffer, same layout as req.body_bytes output). Returns a
            // lowercase hex string (64 chars). Third bridge in the
            // opaque-handle triad (req.body_bytes → crypto.sha256_bytes →
            // fs.write_bytes).
            (
                "crypto.sha256_bytes",
                vec![HirType::Integer],
                HirType::String,
            ),
            // JSON namespace functions  // BOOK: json-module
            // JSON parse functions return Any type (dynamic data structures)
            ("json.textToData", vec![HirType::String], HirType::Any),
            ("json.tryTextToData", vec![HirType::String], HirType::Any),
            // JSON stringify functions take Any type and return string
            ("json.dataToText", vec![HirType::Any], HirType::String),
            ("json.prettyDataToText", vec![HirType::Any], HirType::String),
            // Spec-canonical aliases (stdlib-reference.md §json)
            ("json.encode", vec![HirType::Any], HirType::String),
            ("json.decode", vec![HirType::String], HirType::Any),
            (
                "json.get",
                vec![HirType::Any, HirType::String],
                HirType::Any,
            ),
            // Database namespace (stdlib-reference §9, Layer 2 host bridges).
            // Typed here so the calls keep their types without the frame.data
            // plugin; a plugin declaration with a specific type replaces them.
            (
                "db.query",
                vec![HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "db.execute",
                vec![HirType::String, HirType::String],
                HirType::Integer,
            ),
            ("db.begin", vec![], HirType::Boolean),
            ("db.commit", vec![], HirType::Boolean),
            ("db.rollback", vec![], HirType::Boolean),
            // Input namespace functions (input(), input.integer(), input.yesNo(), etc.)
            ("input", vec![HirType::String], HirType::String),
            ("input_string", vec![HirType::String], HirType::String),
            ("input_integer", vec![HirType::String], HirType::Integer),
            ("input_number", vec![HirType::String], HirType::Number),
            ("input_yesNo", vec![HirType::String], HirType::Boolean),
            (
                "input_integerWithDefault",
                vec![HirType::String, HirType::Integer],
                HirType::Integer,
            ),
            // Validator namespace functions (stdlib-reference.md §15). Rules
            // and result handles are integers.
            ("validator.create", vec![], HirType::Integer),
            (
                "validator.createWithName",
                vec![HirType::String],
                HirType::Integer,
            ),
            (
                "validator.field",
                vec![HirType::Integer, HirType::String],
                HirType::Integer,
            ),
            (
                "validator.merge",
                vec![HirType::Integer, HirType::Integer],
                HirType::Integer,
            ),
            (
                "validator.run",
                vec![HirType::Integer, HirType::Any],
                HirType::Integer,
            ),
            (
                "validator.runField",
                vec![HirType::Integer, HirType::String, HirType::Any],
                HirType::Integer,
            ),
            (
                "validator.validate",
                vec![HirType::Integer, HirType::Any],
                HirType::Integer,
            ),
            ("validator.ok", vec![HirType::Integer], HirType::Integer),
            ("validator.error", vec![HirType::Integer], HirType::Integer),
            ("validator.isOk", vec![HirType::Integer], HirType::Boolean),
            (
                "validator.isError",
                vec![HirType::Integer],
                HirType::Boolean,
            ),
            ("validator.getValue", vec![HirType::Integer], HirType::Any),
            (
                "validator.getErrors",
                vec![HirType::Integer],
                HirType::List(Box::new(HirType::String)),
            ),
            (
                "validator.getFirstError",
                vec![HirType::Integer],
                HirType::String,
            ),
            (
                "validator.required",
                vec![HirType::Integer],
                HirType::Integer,
            ),
            (
                "validator.optional",
                vec![HirType::Integer],
                HirType::Integer,
            ),
            (
                "validator.match",
                vec![HirType::Integer, HirType::String],
                HirType::Integer,
            ),
            (
                "validator.range",
                vec![HirType::Integer, HirType::Number, HirType::Number],
                HirType::Integer,
            ),
            (
                "validator.minLength",
                vec![HirType::Integer, HirType::Integer],
                HirType::Integer,
            ),
            (
                "validator.maxLength",
                vec![HirType::Integer, HirType::Integer],
                HirType::Integer,
            ),
            // The check function `boolean fn(string value)` (grammar.ebnf
            // §8.10), passed as a function reference.
            (
                "validator.custom",
                vec![HirType::Integer, HirType::Handler],
                HirType::Integer,
            ),
            (
                "validator.message",
                vec![HirType::Integer, HirType::String],
                HirType::Integer,
            ),
            ("validator.trim", vec![HirType::Integer], HirType::Integer),
            (
                "validator.type",
                vec![HirType::Integer, HirType::String],
                HirType::Integer,
            ),
            (
                "validator.shape",
                vec![HirType::Integer, HirType::Integer],
                HirType::Integer,
            ),
            (
                "validator.each",
                vec![HirType::Integer, HirType::Integer],
                HirType::Integer,
            ),
            // A missing field takes `value` before its other rules run.
            (
                "validator.default",
                vec![HirType::Integer, HirType::Any],
                HirType::Integer,
            ),
            // Required when the field `otherField` is present and not empty.
            (
                "validator.requiredIf",
                vec![HirType::Integer, HirType::String],
                HirType::Integer,
            ),
            // The errors of a result per field label.
            (
                "validator.fieldErrors",
                vec![HirType::Integer],
                HirType::Pairs(
                    Box::new(HirType::String),
                    Box::new(HirType::List(Box::new(HirType::String))),
                ),
            ),
            // db.* / crypto.{hashPassword,verifyPassword,randomBytes,randomHex,hmac}
            // / env.get / time.now / `now` removed in 0.30.301 — all provided by
            // frame.data, frame.auth, and frame.server plugin.toml `[[functions]]`
            // with `maps_to` + `params` + `returns`. Framework v2.12.29 supplies
            // the language-level declarations (db.query/db.execute/db.queryAs/db.begin/
            // db.commit/db.rollback/db.paginate/db.cursorPage from frame.data;
            // crypto.* from frame.auth; env.get/time.now/now from frame.server).
            // Per BUILTIN-NAMESPACE-OVERREACH Sub-A finalize.
            //
            // crypto.sha256 / crypto.sha512 kept hardcoded — they reference bridge
            // names (`_crypto_sha256` / `_crypto_sha512`) that do not exist in
            // function-registry.toml (which declares them as `_crypto_hash_sha256` /
            // `_crypto_hash_sha512` with `aliases = ["crypto.hash_sha256",
            // "crypto.hash_sha512"]`). Resolving that mismatch is a separate task —
            // either rename the registry aliases or change the resolver entries.
            ("crypto.sha256", vec![HirType::String], HirType::String),
            ("crypto.sha512", vec![HirType::String], HirType::String),
            // Layer 2 crypto / jwt functions (stdlib-reference.md §10, §11) are
            // available to every program: codegen supplies built-in wrappers over
            // the host bridges when no plugin declares them. Their signatures are
            // declared here so a call's result has its real type (string or
            // boolean) instead of an untyped value; a plugin declaring the same
            // names registers the same signatures.
            (
                "crypto.hashPassword",
                vec![HirType::String],
                HirType::String,
            ),
            (
                "crypto.verifyPassword",
                vec![HirType::String, HirType::String],
                HirType::Boolean,
            ),
            (
                "crypto.randomBytes",
                vec![HirType::Integer],
                HirType::String,
            ),
            ("crypto.randomHex", vec![HirType::Integer], HirType::String),
            (
                "crypto.hmac",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "jwt.sign",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            (
                "jwt.verify",
                vec![HirType::String, HirType::String, HirType::String],
                HirType::String,
            ),
            ("jwt.decode", vec![HirType::String], HirType::String),
            // jwt.sign / jwt.verify / jwt.decode removed in 0.30.296 — provided by
            // frame.auth plugin.toml. Per BUILTIN-NAMESPACE-OVERREACH Sub-A resolution;
            // framework v2.12.26+ declares these with maps_to and matching types.
            // req.* removed in 0.30.296 — provided by frame.server plugin.toml `[[functions]]`
            // entries (lines 228-239) with maps_to and matching String types.
            // Per BUILTIN-NAMESPACE-OVERREACH Sub-A resolution; framework v2.12.26+.
            // Bridge-level request context functions (underscore prefix) — Layer 3 server-only.
            // These are the raw WASM import names used by frame.server.  They are registered
            // as builtins so the resolver accepts calls to them even when the plugin declaration
            // is implicit (e.g. files in app/server/ directories auto-load frame.server).
            ("_req_param", vec![HirType::String], HirType::String),
            ("_req_query", vec![HirType::String], HirType::String),
            ("_req_body", vec![], HirType::String),
            // _req_body_bytes returns an opaque byte handle typed as Integer
            // (pointer to a length-prefixed byte buffer). See
            // foundation/spec/type-system.md §9b "Opaque Handle Convention".
            ("_req_body_bytes", vec![], HirType::Integer),
            // _crypto_sha256_bytes(handle) -> hex string. Bridge-level entry
            // for the language-surface crypto.sha256_bytes above. Handle is an
            // Integer per §9b. Registered so files that call the raw bridge
            // name (e.g. generated framework code) resolve without needing an
            // explicit plugin declaration.
            (
                "_crypto_sha256_bytes",
                vec![HirType::Integer],
                HirType::String,
            ),
            ("_req_header", vec![HirType::String], HirType::String),
            ("_req_method", vec![], HirType::String),
            ("_req_path", vec![], HirType::String),
            ("_req_cookie", vec![HirType::String], HirType::String),
            // _fs_write_bytes(path, handle) -> integer error code.
            // path is String; handle is an opaque Integer per §9b.
            (
                "_fs_write_bytes",
                vec![HirType::String, HirType::Integer],
                HirType::Integer,
            ),
            // auth.* / session.* / server.sleep removed in 0.30.296 — provided by:
            //   - frame.server plugin.toml: auth.getSession, auth.requireAuth, auth.requireRole,
            //     auth.can, auth.hasAnyRole, server.sleep
            //   - frame.auth plugin.toml: auth.setSession, auth.clearSession, auth.userId,
            //     auth.userRole, session.get, session.delete, session.exists
            // Per BUILTIN-NAMESPACE-OVERREACH Sub-A resolution; framework v2.12.26+.
            //
            // _server_sleep raw bridge name retained — still called directly from some
            // framework-generated code (parallel to _req_* below).
            ("_server_sleep", vec![HirType::Integer], HirType::Void),
        ];

        for (name, params, return_type) in namespace_functions {
            let symbol_id = self.create_symbol(
                name.to_string(),
                SymbolKind::Function {
                    parameters: params,
                    return_type: Some(return_type),
                },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                    byte_start: None,
                    byte_end: None,
                },
            );
            self.builtins.insert(symbol_id);
        }
    }

    /// Add the built-in namespace identifiers (`math`, `string`, `http`, ...)
    /// so `math.sin(x)` resolves `math` as a namespace. Each namespace lists
    /// the built-in functions registered under it; a name that is not
    /// registered is left out.
    fn add_builtin_namespace_identifiers(&mut self, global_scope: ScopeId) {
        const NAMESPACES: &[(&str, &[&str])] = &[
            (
                "math",
                &[
                    "math.sin",
                    "math.cos",
                    "math.tan",
                    "math.asin",
                    "math.acos",
                    "math.atan",
                    "math.atan2",
                    "math.sinh",
                    "math.cosh",
                    "math.tanh",
                    "math.log",
                    "math.ln",
                    "math.log10",
                    "math.log2",
                    "math.exp",
                    "math.exp2",
                    "math.abs",
                    "math.floor",
                    "math.ceil",
                    "math.round",
                    "math.sqrt",
                    "math.trunc",
                    "math.pi",
                    "math.e",
                    "math.tau",
                    "math.pow",
                    "math.max",
                    "math.min",
                    "math.random",
                    "math.sign",
                ],
            ),
            (
                "string",
                &[
                    "string.length",
                    "string.concat",
                    "string.isEmpty",
                    "string.isBlank",
                    "string.indexOf",
                    "string.lastIndexOf",
                    "string.substring",
                    "string.toUpperCase",
                    "string.toLowerCase",
                    "string.trim",
                    "string.trimStart",
                    "string.trimEnd",
                    "string.contains",
                    "string.startsWith",
                    "string.endsWith",
                    "string.replace",
                    "string.replaceAll",
                    "string.split",
                    "string.join",
                    "string.padStart",
                    "string.padEnd",
                    "string.charAt",
                    "string.charCodeAt",
                ],
            ),
            (
                "list",
                &["list.concat", "list.fill", "list.range", "list.join"],
            ),
            (
                "http",
                &[
                    "http.get",
                    "http.post",
                    "http.put",
                    "http.patch",
                    "http.delete",
                    "http.head",
                    "http.options",
                    "http.postJson",
                    "http.putJson",
                    "http.postForm",
                    "http.getWithHeaders",
                    "http.postWithHeaders",
                    "http.encodeUrl",
                    "http.decodeUrl",
                    "http.buildQuery",
                    "http.respond",
                    "http.redirect",
                    "http.setHeader",
                    "http.setCache",
                    "http.noCache",
                ],
            ),
            (
                "file",
                &[
                    "file.read",
                    "file.write",
                    "file.append",
                    "file.exists",
                    "file.delete",
                ],
            ),
            ("fs", &["fs.write_bytes"]),
            (
                "json",
                &[
                    "json.textToData",
                    "json.tryTextToData",
                    "json.dataToText",
                    "json.prettyDataToText",
                    "json.encode",
                    "json.decode",
                    "json.get",
                ],
            ),
            (
                "input",
                &[
                    "input",
                    "input_string",
                    "input_integer",
                    "input_number",
                    "input_yesNo",
                    "input_integerWithDefault",
                ],
            ),
            (
                "validator",
                &[
                    "validator.create",
                    "validator.createWithName",
                    "validator.run",
                    "validator.runField",
                    "validator.validate",
                    "validator.ok",
                    "validator.error",
                    "validator.isOk",
                    "validator.isError",
                    "validator.getValue",
                    "validator.getErrors",
                    "validator.getFirstError",
                    "validator.field",
                    "validator.required",
                    "validator.optional",
                    "validator.match",
                    "validator.range",
                    "validator.minLength",
                    "validator.maxLength",
                    "validator.custom",
                    "validator.message",
                    "validator.trim",
                    "validator.type",
                    "validator.shape",
                    "validator.each",
                    "validator.merge",
                    "validator.default",
                    "validator.requiredIf",
                    "validator.fieldErrors",
                ],
            ),
            (
                "db",
                &[
                    "db.query",
                    "db.execute",
                    "db.begin",
                    "db.commit",
                    "db.rollback",
                ],
            ),
            (
                "crypto",
                &[
                    "crypto.hashPassword",
                    "crypto.verifyPassword",
                    "crypto.randomBytes",
                    "crypto.randomHex",
                    "crypto.sha256",
                    "crypto.sha512",
                    "crypto.hmac",
                ],
            ),
            ("jwt", &["jwt.sign", "jwt.verify", "jwt.decode"]),
            ("env", &["env.get"]),
            ("time", &["time.now"]),
            (
                "req",
                &[
                    "req.param",
                    "req.query",
                    "req.body",
                    "req.header",
                    "req.headers",
                    "req.method",
                    "req.path",
                    "req.ip",
                    "req.form",
                    "req.cookie",
                ],
            ),
            (
                "auth",
                &[
                    "auth.getSession",
                    "auth.requireAuth",
                    "auth.requireRole",
                    "auth.can",
                    "auth.hasAnyRole",
                    "auth.setSession",
                    "auth.clearSession",
                    "auth.userId",
                    "auth.userRole",
                ],
            ),
            (
                "session",
                &["session.get", "session.delete", "session.exists"],
            ),
            ("server", &["server.sleep"]),
        ];
        for (namespace, function_names) in NAMESPACES {
            let functions = function_names
                .iter()
                .filter_map(|name| self.lookup_symbol(name))
                .collect();
            let namespace_id = self.create_symbol(
                namespace.to_string(),
                SymbolKind::Namespace { functions },
                global_scope,
                SourceLocation {
                    file: "<builtin>".to_string(),
                    line: 0,
                    column: 0,
                    byte_start: None,
                    byte_end: None,
                },
            );
            self.builtins.insert(namespace_id);
        }
    }
}

impl Default for GlobalSymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_table_creation() {
        let table = GlobalSymbolTable::new();

        // Should have global scope
        assert!(table.scopes.contains_key(&ScopeId(0)));

        // Should have built-in functions
        assert!(!table.builtins.is_empty());

        // Built-ins should be in global scope
        let print_symbol = table.lookup_symbol("print");
        assert!(print_symbol.is_some());
        assert!(table.is_builtin(print_symbol.expect("test: print symbol must exist in builtins")));
    }

    #[test]
    fn test_scope_management() {
        let mut table = GlobalSymbolTable::new();

        // Create function scope
        let func_scope = table.create_scope(None);
        table.enter_scope(func_scope);

        // Add parameter
        let param_id = table.create_symbol(
            "x".to_string(),
            SymbolKind::Parameter {
                param_type: HirType::Integer,
            },
            func_scope,
            SourceLocation {
                file: "test".to_string(),
                line: 1,
                column: 1,
                byte_start: None,
                byte_end: None,
            },
        );

        // Should find parameter in function scope
        assert_eq!(table.lookup_symbol("x"), Some(param_id));

        // Exit scope
        table.exit_scope();

        // Should not find parameter in global scope
        assert_eq!(table.lookup_symbol("x"), None);
    }

    #[test]
    fn test_class_member_lookup() {
        let mut table = GlobalSymbolTable::new();

        // Create class
        let class_id = table.create_symbol(
            "TestClass".to_string(),
            SymbolKind::Class {
                fields: vec![],
                methods: vec![],
                parent: None,
            },
            table.current_scope_id(),
            SourceLocation {
                file: "test".to_string(),
                line: 1,
                column: 1,
                byte_start: None,
                byte_end: None,
            },
        );

        // Create field
        let field_id = table.create_symbol(
            "field".to_string(),
            SymbolKind::Field {
                class_id,
                field_type: HirType::Integer,
            },
            table.current_scope_id(),
            SourceLocation {
                file: "test".to_string(),
                line: 2,
                column: 1,
                byte_start: None,
                byte_end: None,
            },
        );

        // Update class to include field
        if let Some(symbol) = table.get_symbol_mut(class_id) {
            if let SymbolKind::Class { fields, .. } = &mut symbol.kind {
                fields.push(field_id);
            }
        }

        // Should find field in class
        assert_eq!(table.lookup_class_member(class_id, "field"), Some(field_id));
        assert_eq!(table.lookup_class_member(class_id, "nonexistent"), None);
    }
}
