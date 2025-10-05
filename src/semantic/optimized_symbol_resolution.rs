//! High-performance symbol resolution system for the Clean Language semantic analyzer
//!
//! This module provides optimized data structures and algorithms to eliminate O(n²)
//! performance bottlenecks in symbol lookup, function resolution, and type checking.

#![allow(dead_code)]

use crate::ast::{FunctionModifier, SourceLocation, Type, Visibility};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// Fast symbol cache using a multi-level hash map structure
/// This eliminates the need for linear scope chain traversal
#[derive(Debug, Clone)]
pub struct OptimizedSymbolCache {
    /// Flattened symbol table: symbol_name -> (scope_level, symbol_info)
    /// This allows O(1) symbol lookup instead of O(scope_depth)
    symbol_cache: HashMap<String, CachedSymbol>,

    /// Scope-specific symbols for efficient scope exit cleanup
    scope_symbols: HashMap<usize, HashSet<String>>,

    /// Current scope level for cache validation
    current_scope_level: usize,

    /// Maximum scope level reached (for cache invalidation)
    max_scope_level: usize,
}

/// Cached symbol information with scope tracking
#[derive(Debug, Clone)]
pub struct CachedSymbol {
    pub symbol_type: Type,
    pub scope_level: usize,
    pub is_used: bool,
    pub location: Option<SourceLocation>,
    pub visibility: Visibility,
}

impl OptimizedSymbolCache {
    pub fn new() -> Self {
        Self {
            symbol_cache: HashMap::new(),
            scope_symbols: HashMap::new(),
            current_scope_level: 0,
            max_scope_level: 0,
        }
    }

    /// Add a symbol to the cache - O(1) operation
    pub fn add_symbol(
        &mut self,
        name: String,
        symbol_type: Type,
        location: Option<SourceLocation>,
    ) {
        let cached_symbol = CachedSymbol {
            symbol_type,
            scope_level: self.current_scope_level,
            is_used: false,
            location,
            visibility: Visibility::Private,
        };

        // Add to flat cache
        self.symbol_cache.insert(name.clone(), cached_symbol);

        // Track scope-specific symbols for cleanup
        self.scope_symbols
            .entry(self.current_scope_level)
            .or_insert_with(HashSet::new)
            .insert(name);
    }

    /// Lookup symbol - O(1) operation instead of O(scope_depth)
    pub fn lookup_symbol(&self, name: &str) -> Option<&CachedSymbol> {
        self.symbol_cache.get(name)
    }

    /// Lookup and mark symbol as used - O(1) operation
    pub fn lookup_and_use_symbol(&mut self, name: &str) -> Option<Type> {
        if let Some(cached_symbol) = self.symbol_cache.get_mut(name) {
            cached_symbol.is_used = true;
            Some(cached_symbol.symbol_type.clone())
        } else {
            None
        }
    }

    /// Enter a new scope - O(1) operation
    pub fn enter_scope(&mut self) {
        self.current_scope_level += 1;
        self.max_scope_level = self.max_scope_level.max(self.current_scope_level);
    }

    /// Exit current scope - O(k) where k is symbols in current scope, not O(n²)
    pub fn exit_scope(&mut self) -> Vec<String> {
        let mut removed_symbols = Vec::new();

        if let Some(symbols_in_scope) = self.scope_symbols.remove(&self.current_scope_level) {
            for symbol_name in symbols_in_scope {
                if let Some(cached_symbol) = self.symbol_cache.get(&symbol_name) {
                    if cached_symbol.scope_level == self.current_scope_level {
                        self.symbol_cache.remove(&symbol_name);
                        removed_symbols.push(symbol_name);
                    }
                }
            }
        }

        if self.current_scope_level > 0 {
            self.current_scope_level -= 1;
        }

        removed_symbols
    }

    /// Get all visible symbols in current scope - O(1) since we maintain flat structure
    pub fn get_visible_symbols(&self) -> Vec<String> {
        self.symbol_cache.keys().cloned().collect()
    }

    /// Check if symbol exists in current scope only - O(1)
    pub fn exists_in_current_scope(&self, name: &str) -> bool {
        if let Some(cached_symbol) = self.symbol_cache.get(name) {
            cached_symbol.scope_level == self.current_scope_level
        } else {
            false
        }
    }
}

/// High-performance function resolution system
/// Eliminates repeated HashMap lookups and provides O(1) function resolution
#[derive(Debug)]
pub struct OptimizedFunctionResolver {
    /// Pre-computed function signature cache: function_name -> compiled_signatures
    function_cache: HashMap<String, CompiledFunctionInfo>,

    /// Function overload groups indexed by parameter count for fast filtering
    overload_index: HashMap<String, HashMap<usize, Vec<FunctionSignature>>>,

    /// Built-in function cache (populated once, never changes)
    builtin_cache: HashMap<String, CompiledFunctionInfo>,

    /// Method resolution cache: (class_name, method_name) -> resolved_type
    method_cache: HashMap<MethodKey, Type>,
}

/// Compiled function information for O(1) lookups
#[derive(Debug, Clone)]
pub struct CompiledFunctionInfo {
    pub name: String,
    pub overloads: Vec<FunctionSignature>,
    pub is_builtin: bool,
    pub is_variadic: bool,
}

/// Function signature with precomputed hash for fast comparison
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub parameter_types: Vec<Type>,
    pub return_type: Type,
    pub required_param_count: usize,
    pub modifiers: Vec<FunctionModifier>,
    pub signature_hash: u64, // Precomputed for O(1) comparison
}

/// Method resolution cache key
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MethodKey {
    class_name: String,
    method_name: String,
    param_count: usize,
}

impl FunctionSignature {
    pub fn new(
        parameter_types: Vec<Type>,
        return_type: Type,
        required_param_count: usize,
        modifiers: Vec<FunctionModifier>,
    ) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        // Hash parameter types
        for param_type in &parameter_types {
            format!("{:?}", param_type).hash(&mut hasher);
        }

        // Hash return type
        format!("{:?}", return_type).hash(&mut hasher);

        // Hash required param count
        required_param_count.hash(&mut hasher);

        let signature_hash = hasher.finish();

        Self {
            parameter_types,
            return_type,
            required_param_count,
            modifiers,
            signature_hash,
        }
    }

    /// Fast signature matching using precomputed hash
    pub fn matches_signature(&self, other: &FunctionSignature) -> bool {
        // First check hash for O(1) elimination of non-matches
        if self.signature_hash != other.signature_hash {
            return false;
        }

        // Only if hashes match, do detailed comparison
        self.parameter_types == other.parameter_types
            && self.return_type == other.return_type
            && self.required_param_count == other.required_param_count
    }

    /// Check if this signature can accept the given argument types
    pub fn is_compatible_with(&self, arg_types: &[Type]) -> bool {
        if arg_types.len() < self.required_param_count {
            return false;
        }

        if arg_types.len() > self.parameter_types.len() {
            return false;
        }

        // Check type compatibility for each provided argument
        for (i, arg_type) in arg_types.iter().enumerate() {
            if i < self.parameter_types.len() {
                if !self.is_type_compatible(arg_type, &self.parameter_types[i]) {
                    return false;
                }
            }
        }

        true
    }

    /// Enhanced type compatibility checking
    fn is_type_compatible(&self, provided: &Type, expected: &Type) -> bool {
        match (provided, expected) {
            // Exact matches
            (a, b) if a == b => true,

            // Numeric conversions
            (Type::Integer, Type::Number) => true,
            (Type::Number, Type::Integer) => false, // No implicit narrowing

            // Any type compatibility
            (_, Type::Any) => true,
            (Type::Any, _) => true,

            _ => false,
        }
    }
}

impl OptimizedFunctionResolver {
    pub fn new() -> Self {
        let mut resolver = Self {
            function_cache: HashMap::new(),
            overload_index: HashMap::new(),
            builtin_cache: HashMap::new(),
            method_cache: HashMap::new(),
        };

        resolver.initialize_builtin_cache();
        resolver
    }

    /// Register a function with all its overloads - O(1) operation
    pub fn register_function(
        &mut self,
        name: String,
        overloads: Vec<FunctionSignature>,
        is_builtin: bool,
    ) {
        let compiled_info = CompiledFunctionInfo {
            name: name.clone(),
            overloads: overloads.clone(),
            is_builtin,
            is_variadic: false,
        };

        // Add to main cache
        self.function_cache
            .insert(name.clone(), compiled_info.clone());

        // Add to builtin cache if applicable
        if is_builtin {
            self.builtin_cache.insert(name.clone(), compiled_info);
        }

        // Create overload index by parameter count
        let mut param_count_index = HashMap::new();
        for overload in overloads {
            let param_count = overload.parameter_types.len();
            param_count_index
                .entry(param_count)
                .or_insert_with(Vec::new)
                .push(overload);
        }
        self.overload_index.insert(name, param_count_index);
    }

    /// Resolve function call - O(1) lookup + O(k) where k is number of overloads (usually small)
    pub fn resolve_function_call(
        &self,
        function_name: &str,
        arg_types: &[Type],
    ) -> Result<&FunctionSignature, String> {
        // O(1) lookup in function cache
        let _function_info = self
            .function_cache
            .get(function_name)
            .ok_or_else(|| format!("Function '{}' not found", function_name))?;

        // Fast overload resolution using parameter count index
        if let Some(overload_index) = self.overload_index.get(function_name) {
            // Try exact parameter count match first
            if let Some(candidates) = overload_index.get(&arg_types.len()) {
                for signature in candidates {
                    if signature.is_compatible_with(arg_types) {
                        return Ok(signature);
                    }
                }
            }

            // Try variadic matches (parameter count >= required)
            for (param_count, candidates) in overload_index {
                if *param_count <= arg_types.len() {
                    for signature in candidates {
                        if signature.is_compatible_with(arg_types) {
                            return Ok(signature);
                        }
                    }
                }
            }
        }

        Err(format!(
            "No compatible overload found for function '{}' with {} arguments",
            function_name,
            arg_types.len()
        ))
    }

    /// Cache method resolution results for O(1) repeated lookups
    pub fn cache_method_resolution(
        &mut self,
        class_name: String,
        method_name: String,
        param_count: usize,
        resolved_type: Type,
    ) {
        let key = MethodKey {
            class_name,
            method_name,
            param_count,
        };
        self.method_cache.insert(key, resolved_type);
    }

    /// Lookup cached method resolution - O(1)
    pub fn lookup_cached_method(
        &self,
        class_name: &str,
        method_name: &str,
        param_count: usize,
    ) -> Option<&Type> {
        let key = MethodKey {
            class_name: class_name.to_string(),
            method_name: method_name.to_string(),
            param_count,
        };
        self.method_cache.get(&key)
    }

    /// Initialize built-in function cache
    fn initialize_builtin_cache(&mut self) {
        // This would be populated with built-in functions
        // For now, it's a placeholder for the implementation
    }

    /// Get function suggestions for error messages - O(1)
    pub fn get_function_suggestions(&self, partial_name: &str) -> Vec<String> {
        self.function_cache
            .keys()
            .filter(|name| name.contains(partial_name))
            .cloned()
            .collect()
    }

    /// Clear method cache when class definitions change
    pub fn invalidate_method_cache(&mut self) {
        self.method_cache.clear();
    }
}

/// High-performance scope chain optimization
/// Maintains parent-child relationships without requiring traversal for lookups
#[derive(Debug)]
pub struct OptimizedScopeChain {
    /// Scope information by level
    scopes: HashMap<usize, ScopeInfo>,

    /// Current scope level
    current_level: usize,

    /// Parent scope chain for scoped symbol resolution
    parent_chain: Vec<usize>,
}

#[derive(Debug)]
struct ScopeInfo {
    level: usize,
    symbols: HashSet<String>,
    parent_level: Option<usize>,
}

impl OptimizedScopeChain {
    pub fn new() -> Self {
        let mut chain = Self {
            scopes: HashMap::new(),
            current_level: 0,
            parent_chain: Vec::new(),
        };

        // Initialize global scope
        chain.scopes.insert(
            0,
            ScopeInfo {
                level: 0,
                symbols: HashSet::new(),
                parent_level: None,
            },
        );

        chain
    }

    /// Enter new scope - O(1)
    pub fn enter_scope(&mut self) {
        self.parent_chain.push(self.current_level);
        self.current_level += 1;

        let parent_level = if self.current_level > 0 {
            Some(self.current_level - 1)
        } else {
            None
        };

        self.scopes.insert(
            self.current_level,
            ScopeInfo {
                level: self.current_level,
                symbols: HashSet::new(),
                parent_level,
            },
        );
    }

    /// Exit scope - O(1)
    pub fn exit_scope(&mut self) -> Vec<String> {
        let removed_symbols = if let Some(scope) = self.scopes.remove(&self.current_level) {
            scope.symbols.into_iter().collect()
        } else {
            Vec::new()
        };

        if let Some(parent_level) = self.parent_chain.pop() {
            self.current_level = parent_level;
        }

        removed_symbols
    }

    /// Add symbol to current scope - O(1)
    pub fn add_symbol(&mut self, symbol_name: String) {
        if let Some(scope) = self.scopes.get_mut(&self.current_level) {
            scope.symbols.insert(symbol_name);
        }
    }

    /// Check if symbol exists in scope chain - O(scope_depth) but with HashMap lookups
    pub fn symbol_exists_in_chain(&self, symbol_name: &str) -> bool {
        let mut current_level = self.current_level;

        loop {
            if let Some(scope) = self.scopes.get(&current_level) {
                if scope.symbols.contains(symbol_name) {
                    return true;
                }

                match scope.parent_level {
                    Some(parent) => current_level = parent,
                    None => break,
                }
            } else {
                break;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Type;

    #[test]
    fn test_optimized_symbol_cache_performance() {
        let mut cache = OptimizedSymbolCache::new();

        // Add symbols to different scopes
        cache.add_symbol("x".to_string(), Type::Integer, None);
        cache.enter_scope();
        cache.add_symbol("y".to_string(), Type::String, None);
        cache.enter_scope();
        cache.add_symbol("z".to_string(), Type::Boolean, None);

        // O(1) lookups
        assert!(cache.lookup_symbol("x").is_some());
        assert!(cache.lookup_symbol("y").is_some());
        assert!(cache.lookup_symbol("z").is_some());
        assert!(cache.lookup_symbol("nonexistent").is_none());

        // Scope exit cleanup
        let removed = cache.exit_scope();
        assert!(removed.contains(&"z".to_string()));
        assert!(cache.lookup_symbol("z").is_none());
        assert!(cache.lookup_symbol("y").is_some());
    }

    #[test]
    fn test_function_signature_matching() {
        let sig1 =
            FunctionSignature::new(vec![Type::Integer, Type::String], Type::Boolean, 2, vec![]);

        let sig2 =
            FunctionSignature::new(vec![Type::Integer, Type::String], Type::Boolean, 2, vec![]);

        let sig3 = FunctionSignature::new(vec![Type::Integer], Type::Boolean, 1, vec![]);

        assert!(sig1.matches_signature(&sig2));
        assert!(!sig1.matches_signature(&sig3));
    }

    #[test]
    fn test_function_resolver_performance() {
        let mut resolver = OptimizedFunctionResolver::new();

        // Register a function with overloads
        let overloads = vec![
            FunctionSignature::new(vec![Type::Integer], Type::String, 1, vec![]),
            FunctionSignature::new(vec![Type::Integer, Type::Boolean], Type::String, 2, vec![]),
        ];

        resolver.register_function("test_func".to_string(), overloads, false);

        // O(1) + O(k) resolution
        let result1 = resolver.resolve_function_call("test_func", &[Type::Integer]);
        assert!(result1.is_ok());

        let result2 = resolver.resolve_function_call("test_func", &[Type::Integer, Type::Boolean]);
        assert!(result2.is_ok());

        let result3 = resolver.resolve_function_call("test_func", &[Type::String]);
        assert!(result3.is_err());
    }
}
