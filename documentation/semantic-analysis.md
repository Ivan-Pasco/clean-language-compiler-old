# Clean Language Semantic Analysis System

This document provides comprehensive documentation for Claude on how the Clean Language semantic analysis system works. This information will help Claude understand and work with the type checking, scope management, and semantic validation systems.

> 🔗 **Related Documentation**: [Parser Documentation](./parser.md) • [WebAssembly Generation](./webassembly.md) • [Standard Library](./standard-library.md) • [Development Guide](./development-guide.md)

## Overview

The Clean Language semantic analysis system performs comprehensive validation of parsed code, including type checking, scope management, symbol resolution, and semantic error detection. The system bridges the gap between parsing and code generation, ensuring type safety and semantic correctness.

## Architecture Components

### 1. Semantic Analyzer Core (`src/semantic/mod.rs`)

The `SemanticAnalyzer` struct is the central component that orchestrates all semantic analysis:

```rust
pub struct SemanticAnalyzer {
    // Symbol and type management
    symbol_table: HashMap<String, Type>,
    function_table: HashMap<String, Vec<FunctionOverload>>,
    class_table: HashMap<String, ClassInfo>,
    type_constraints: Vec<TypeConstraint>,
    
    // Scope management
    current_scope: Scope,
    scope_stack: Vec<Scope>,
    
    // Analysis state
    current_function: Option<String>,
    current_class: Option<String>,
    in_loop: bool,
    async_context: bool,
    
    // Error and warning collection
    errors: Vec<CompilerError>,
    warnings: Vec<CompilerWarning>,
    
    // Module system
    module_resolver: ModuleResolver,
    imported_symbols: HashMap<String, ImportInfo>,
}

pub struct FunctionOverload {
    pub parameter_types: Vec<Type>,
    pub return_type: Type,
    pub function_index: usize,
    pub location: SourceLocation,
}

pub struct ClassInfo {
    pub name: String,
    pub parent: Option<String>,
    pub fields: HashMap<String, (Type, SourceLocation)>,
    pub methods: HashMap<String, Vec<FunctionOverload>>,
    pub constructor: Option<Constructor>,
    pub type_parameters: Vec<String>,
    pub is_generic: bool,
}
```

**Key Responsibilities:**
- **Type inference and checking**: Validates all type relationships
- **Scope management**: Tracks variable and function visibility
- **Symbol resolution**: Resolves identifiers to their definitions
- **Function overload resolution**: Handles multiple function signatures
- **Class inheritance validation**: Ensures proper class hierarchies
- **Module dependency resolution**: Manages imports and exports
- **Error reporting**: Collects and formats semantic errors

### 2. Type System (`src/semantic/type_checker.rs`)

The type checker implements Clean Language's rich type system:

**Core Type Definitions:**
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    // Primitive types
    Boolean,
    Integer,
    Number,
    String,
    Void,
    
    // Sized numeric types
    IntegerSized { bits: u8, unsigned: bool },
    NumberSized { bits: u8 },
    
    // Composite types
    List(Box<Type>),
    Matrix(Box<Type>),
    Pairs(Box<Type>, Box<Type>),
    
    // Object-oriented types
    Object(String),
    Class { name: String, type_args: Vec<Type> },
    
    // Advanced types
    Function(Vec<Type>, Box<Type>),
    Future(Box<Type>),
    Generic(Box<Type>, Vec<Type>),
    TypeParameter(String),
    Any,
    
    // Error type for recovery
    Error,
}

impl Type {
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Integer | Type::Number | 
                Type::IntegerSized { .. } | Type::NumberSized { .. })
    }
    
    pub fn is_compatible_with(&self, other: &Type) -> bool {
        match (self, other) {
            // Any is compatible with everything
            (Type::Any, _) | (_, Type::Any) => true,
            
            // Exact matches
            (a, b) if a == b => true,
            
            // Numeric promotions
            (Type::Integer, Type::Number) => true,
            (Type::IntegerSized { .. }, Type::Number) => true,
            
            // Generic type compatibility
            (Type::Generic(base1, args1), Type::Generic(base2, args2)) => {
                base1 == base2 && args1.len() == args2.len() &&
                args1.iter().zip(args2.iter()).all(|(a, b)| a.is_compatible_with(b))
            }
            
            // Class inheritance
            (Type::Class { name: child, .. }, Type::Class { name: parent, .. }) => {
                self.is_subclass_of(child, parent)
            }
            
            _ => false,
        }
    }
    
    pub fn get_common_type(&self, other: &Type) -> Option<Type> {
        match (self, other) {
            (Type::Integer, Type::Number) | (Type::Number, Type::Integer) => Some(Type::Number),
            (Type::IntegerSized { .. }, Type::Number) => Some(Type::Number),
            (Type::Any, t) | (t, Type::Any) => Some(t.clone()),
            (a, b) if a == b => Some(a.clone()),
            _ => None,
        }
    }
}
```

**Type Checking Functions:**
```rust
impl SemanticAnalyzer {
    pub fn check_expression(&mut self, expr: &Expression) -> Result<Type, CompilerError> {
        match expr {
            Expression::Literal(literal) => self.check_literal(literal),
            Expression::Variable(name) => self.lookup_variable_type(name),
            Expression::Binary { left, operator, right, .. } => {
                self.check_binary_operation(left, operator, right)
            }
            Expression::Call { name, arguments, .. } => {
                self.check_function_call(name, arguments)
            }
            Expression::MethodCall { object, method, arguments, .. } => {
                self.check_method_call(object, method, arguments)
            }
            Expression::PropertyAccess { object, property, .. } => {
                self.check_property_access(object, property)
            }
            Expression::IndexAccess { object, index, .. } => {
                self.check_index_access(object, index)
            }
            Expression::StringInterpolation { parts, .. } => {
                self.check_string_interpolation(parts)
            }
            Expression::Conditional { condition, then_expr, else_expr, .. } => {
                self.check_conditional_expression(condition, then_expr, else_expr)
            }
            Expression::OnError { expression, error_handler, .. } => {
                self.check_error_handling(expression, error_handler)
            }
            Expression::Base { arguments, .. } => {
                self.check_base_call(arguments)
            }
        }
    }
    
    fn check_binary_operation(&mut self, left: &Expression, op: &BinaryOperator, right: &Expression) -> Result<Type, CompilerError> {
        let left_type = self.check_expression(left)?;
        let right_type = self.check_expression(right)?;
        
        match op {
            BinaryOperator::Add | BinaryOperator::Subtract | 
            BinaryOperator::Multiply | BinaryOperator::Divide | 
            BinaryOperator::Modulo => {
                self.check_arithmetic_operation(&left_type, &right_type, op)
            }
            BinaryOperator::Power => {
                self.check_power_operation(&left_type, &right_type)
            }
            BinaryOperator::Equal | BinaryOperator::NotEqual => {
                // Any two types can be compared for equality
                Ok(Type::Boolean)
            }
            BinaryOperator::Less | BinaryOperator::Greater | 
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual => {
                self.check_comparison_operation(&left_type, &right_type)
            }
            BinaryOperator::And | BinaryOperator::Or => {
                self.check_logical_operation(&left_type, &right_type)
            }
            BinaryOperator::Is | BinaryOperator::Not => {
                self.check_identity_operation(&left_type, &right_type)
            }
        }
    }
}
```

### 3. Scope Management (`src/semantic/scope.rs`)

The scope system manages variable and function visibility:

```rust
#[derive(Debug, Clone)]
pub struct Scope {
    variables: HashMap<String, VariableInfo>,
    functions: HashSet<String>,
    parent: Option<Box<Scope>>,
    scope_type: ScopeType,
    scope_id: usize,
}

#[derive(Debug, Clone)]
pub struct VariableInfo {
    pub var_type: Type,
    pub location: SourceLocation,
    pub is_mutable: bool,
    pub is_used: bool,
    pub scope_level: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScopeType {
    Global,
    Function,
    Class,
    Block,
    Loop,
    Conditional,
}

impl Scope {
    pub fn new(scope_type: ScopeType, scope_id: usize) -> Self {
        Self {
            variables: HashMap::new(),
            functions: HashSet::new(),
            parent: None,
            scope_type,
            scope_id,
        }
    }
    
    pub fn enter_scope(&mut self, scope_type: ScopeType, scope_id: usize) {
        let new_scope = Scope::new(scope_type, scope_id);
        let old_scope = std::mem::replace(self, new_scope);
        self.parent = Some(Box::new(old_scope));
    }
    
    pub fn exit_scope(&mut self) -> Result<(), SemanticError> {
        if let Some(parent) = self.parent.take() {
            // Check for unused variables before exiting
            self.check_unused_variables();
            *self = *parent;
            Ok(())
        } else {
            Err(SemanticError::CannotExitGlobalScope)
        }
    }
    
    pub fn lookup_variable(&self, name: &str) -> Option<&VariableInfo> {
        if let Some(var_info) = self.variables.get(name) {
            Some(var_info)
        } else if let Some(parent) = &self.parent {
            parent.lookup_variable(name)
        } else {
            None
        }
    }
    
    pub fn define_variable(&mut self, name: String, var_type: Type, location: SourceLocation) -> Result<(), SemanticError> {
        if self.variables.contains_key(&name) {
            Err(SemanticError::VariableAlreadyDefined { 
                name: name.clone(), 
                location 
            })
        } else {
            self.variables.insert(name, VariableInfo {
                var_type,
                location,
                is_mutable: true,
                is_used: false,
                scope_level: self.scope_id,
            });
            Ok(())
        }
    }
    
    pub fn mark_variable_used(&mut self, name: &str) {
        if let Some(var_info) = self.variables.get_mut(name) {
            var_info.is_used = true;
        } else if let Some(parent) = &mut self.parent {
            parent.mark_variable_used(name);
        }
    }
    
    fn check_unused_variables(&self) {
        for (name, var_info) in &self.variables {
            if !var_info.is_used && !name.starts_with('_') {
                // Generate warning for unused variable
                self.warnings.push(CompilerWarning::UnusedVariable {
                    name: name.clone(),
                    location: var_info.location.clone(),
                });
            }
        }
    }
}
```

### 4. Function Resolution System

Handles function overloading and method resolution:

```rust
impl SemanticAnalyzer {
    pub fn resolve_function_call(&mut self, name: &str, arg_types: &[Type], location: &SourceLocation) -> Result<FunctionOverload, CompilerError> {
        // First, look for exact match
        if let Some(overloads) = self.function_table.get(name) {
            // Try exact type match first
            for overload in overloads {
                if self.types_match_exactly(&overload.parameter_types, arg_types) {
                    return Ok(overload.clone());
                }
            }
            
            // Try compatible type match
            for overload in overloads {
                if self.types_are_compatible(&overload.parameter_types, arg_types) {
                    return Ok(overload.clone());
                }
            }
            
            // No matching overload found
            Err(CompilerError::NoMatchingOverload {
                function_name: name.to_string(),
                provided_types: arg_types.to_vec(),
                available_overloads: overloads.iter()
                    .map(|o| o.parameter_types.clone())
                    .collect(),
                location: location.clone(),
            })
        } else {
            // Check for built-in functions
            self.resolve_builtin_function(name, arg_types, location)
        }
    }
    
    pub fn resolve_method_call(&mut self, object_type: &Type, method_name: &str, arg_types: &[Type], location: &SourceLocation) -> Result<Type, CompilerError> {
        match object_type {
            Type::Class { name, .. } => {
                if let Some(class_info) = self.class_table.get(name).cloned() {
                    if let Some(method_overloads) = class_info.methods.get(method_name) {
                        // Find matching method overload
                        for overload in method_overloads {
                            if self.types_are_compatible(&overload.parameter_types, arg_types) {
                                return Ok(overload.return_type.clone());
                            }
                        }
                        
                        Err(CompilerError::NoMatchingMethodOverload {
                            class_name: name.clone(),
                            method_name: method_name.to_string(),
                            provided_types: arg_types.to_vec(),
                            location: location.clone(),
                        })
                    } else {
                        // Check parent class methods
                        if let Some(parent_name) = &class_info.parent {
                            let parent_type = Type::Class { 
                                name: parent_name.clone(), 
                                type_args: vec![] 
                            };
                            self.resolve_method_call(&parent_type, method_name, arg_types, location)
                        } else {
                            Err(CompilerError::UndefinedMethod {
                                class_name: name.clone(),
                                method_name: method_name.to_string(),
                                location: location.clone(),
                            })
                        }
                    }
                } else {
                    Err(CompilerError::UndefinedClass {
                        name: name.clone(),
                        location: location.clone(),
                    })
                }
            }
            
            // Built-in type methods
            Type::String => self.resolve_string_method(method_name, arg_types, location),
            Type::List(element_type) => self.resolve_list_method(element_type, method_name, arg_types, location),
            Type::Matrix(element_type) => self.resolve_matrix_method(element_type, method_name, arg_types, location),
            Type::Integer | Type::Number => self.resolve_numeric_method(object_type, method_name, arg_types, location),
            
            _ => Err(CompilerError::InvalidMethodCall {
                object_type: object_type.clone(),
                method_name: method_name.to_string(),
                location: location.clone(),
            })
        }
    }
}
```

### 5. Class Inheritance Validation

Ensures proper class hierarchy and inheritance rules:

```rust
impl SemanticAnalyzer {
    pub fn validate_class_hierarchy(&mut self) -> Result<(), Vec<CompilerError>> {
        let mut errors = Vec::new();
        
        // Check for circular inheritance
        for (class_name, class_info) in &self.class_table {
            if let Err(error) = self.check_circular_inheritance(class_name, &mut HashSet::new()) {
                errors.push(error);
            }
        }
        
        // Validate method overrides
        for (class_name, class_info) in &self.class_table {
            if let Some(parent_name) = &class_info.parent {
                if let Err(override_errors) = self.validate_method_overrides(class_name, parent_name) {
                    errors.extend(override_errors);
                }
            }
        }
        
        // Validate constructor chains
        for (class_name, class_info) in &self.class_table {
            if let Some(constructor) = &class_info.constructor {
                if let Err(error) = self.validate_constructor_chain(class_name, constructor) {
                    errors.push(error);
                }
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
    
    fn check_circular_inheritance(&self, class_name: &str, visited: &mut HashSet<String>) -> Result<(), CompilerError> {
        if visited.contains(class_name) {
            return Err(CompilerError::CircularInheritance {
                class_name: class_name.to_string(),
                inheritance_chain: visited.iter().cloned().collect(),
            });
        }
        
        visited.insert(class_name.to_string());
        
        if let Some(class_info) = self.class_table.get(class_name) {
            if let Some(parent_name) = &class_info.parent {
                self.check_circular_inheritance(parent_name, visited)?;
            }
        }
        
        visited.remove(class_name);
        Ok(())
    }
    
    fn validate_method_overrides(&self, child_class: &str, parent_class: &str) -> Result<(), Vec<CompilerError>> {
        let mut errors = Vec::new();
        
        if let (Some(child_info), Some(parent_info)) = 
            (self.class_table.get(child_class), self.class_table.get(parent_class)) {
            
            for (method_name, child_overloads) in &child_info.methods {
                if let Some(parent_overloads) = parent_info.methods.get(method_name) {
                    // Method is being overridden, validate signatures
                    for child_overload in child_overloads {
                        let mut found_compatible_parent = false;
                        
                        for parent_overload in parent_overloads {
                            if child_overload.parameter_types == parent_overload.parameter_types {
                                // Check return type compatibility
                                if !child_overload.return_type.is_compatible_with(&parent_overload.return_type) {
                                    errors.push(CompilerError::IncompatibleMethodOverride {
                                        child_class: child_class.to_string(),
                                        parent_class: parent_class.to_string(),
                                        method_name: method_name.clone(),
                                        child_return_type: child_overload.return_type.clone(),
                                        parent_return_type: parent_overload.return_type.clone(),
                                        location: child_overload.location.clone(),
                                    });
                                }
                                found_compatible_parent = true;
                                break;
                            }
                        }
                        
                        if !found_compatible_parent {
                            errors.push(CompilerError::MethodOverrideSignatureMismatch {
                                child_class: child_class.to_string(),
                                parent_class: parent_class.to_string(),
                                method_name: method_name.clone(),
                                child_signature: child_overload.parameter_types.clone(),
                                location: child_overload.location.clone(),
                            });
                        }
                    }
                }
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
```

### 6. Built-in Function Registration

Registers standard library functions for semantic analysis:

```rust
impl SemanticAnalyzer {
    pub fn register_builtin_functions(&mut self) {
        // Console functions
        self.register_builtin("print", vec![Type::String], Type::Void);
        self.register_builtin("println", vec![Type::String], Type::Void);
        self.register_builtin("printl", vec![Type::String], Type::Void);
        
        // Math module functions
        self.register_math_functions();
        
        // String module functions
        self.register_string_functions();
        
        // List module functions
        self.register_list_functions();
        
        // File I/O functions
        self.register_file_functions();
        
        // HTTP functions
        self.register_http_functions();
        
        // Type conversion functions
        self.register_conversion_functions();
        
        // Assertion functions
        self.register_assertion_functions();
        
        // Utility functions
        self.register_utility_functions();
    }
    
    fn register_math_functions(&mut self) {
        let math_functions = [
            ("abs", vec![Type::Integer], Type::Integer),
            ("abs", vec![Type::Number], Type::Number),
            ("sqrt", vec![Type::Number], Type::Number),
            ("max", vec![Type::Number, Type::Number], Type::Number),
            ("min", vec![Type::Number, Type::Number], Type::Number),
            ("floor", vec![Type::Number], Type::Number),
            ("ceil", vec![Type::Number], Type::Number),
            ("round", vec![Type::Number], Type::Number),
            ("sin", vec![Type::Number], Type::Number),
            ("cos", vec![Type::Number], Type::Number),
            ("tan", vec![Type::Number], Type::Number),
            ("ln", vec![Type::Number], Type::Number),
            ("log10", vec![Type::Number], Type::Number),
            ("exp", vec![Type::Number], Type::Number),
            ("pi", vec![], Type::Number),
            ("e", vec![], Type::Number),
        ];
        
        for (name, params, return_type) in math_functions {
            self.register_builtin(&format!("math.{}", name), params, return_type);
        }
    }
    
    fn register_string_functions(&mut self) {
        let string_functions = [
            ("length", vec![Type::String], Type::Integer),
            ("concat", vec![Type::String, Type::String], Type::String),
            ("substring", vec![Type::String, Type::Integer, Type::Integer], Type::String),
            ("toUpperCase", vec![Type::String], Type::String),
            ("toLowerCase", vec![Type::String], Type::String),
            ("contains", vec![Type::String, Type::String], Type::Boolean),
            ("indexOf", vec![Type::String, Type::String], Type::Integer),
            ("replace", vec![Type::String, Type::String, Type::String], Type::String),
            ("trim", vec![Type::String], Type::String),
            ("isEmpty", vec![Type::String], Type::Boolean),
        ];
        
        for (name, params, return_type) in string_functions {
            self.register_builtin(&format!("string.{}", name), params, return_type);
        }
    }
    
    fn register_assertion_functions(&mut self) {
        self.register_builtin("mustBeTrue", vec![Type::Boolean], Type::Void);
        self.register_builtin("mustBeFalse", vec![Type::Boolean], Type::Void);
        self.register_builtin("mustBeEqual", vec![Type::Any, Type::Any], Type::Void);
        self.register_builtin("mustNotBeEqual", vec![Type::Any, Type::Any], Type::Void);
    }
}
```

## Error Handling and Diagnostics

### 1. Semantic Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum SemanticError {
    #[error("Undefined variable '{name}' at {location}")]
    UndefinedVariable { 
        name: String, 
        location: SourceLocation,
        suggestions: Vec<String>,
    },
    
    #[error("Type mismatch: expected {expected}, found {found} at {location}")]
    TypeMismatch { 
        expected: Type, 
        found: Type, 
        location: SourceLocation,
        context: String,
    },
    
    #[error("Undefined function '{name}' with signature ({signature}) at {location}")]
    UndefinedFunction { 
        name: String,
        signature: Vec<Type>,
        location: SourceLocation,
        available_overloads: Vec<Vec<Type>>,
    },
    
    #[error("Circular inheritance detected: {class_name} inherits from itself through {chain:?}")]
    CircularInheritance { 
        class_name: String,
        inheritance_chain: Vec<String>,
    },
    
    #[error("Cannot access private member '{member}' of class '{class}' at {location}")]
    PrivateMemberAccess { 
        class: String,
        member: String,
        location: SourceLocation,
    },
    
    #[error("Invalid base() call: not in constructor context at {location}")]
    InvalidBaseCall { location: SourceLocation },
    
    #[error("Async operation '{operation}' not allowed in sync context at {location}")]
    AsyncInSyncContext { 
        operation: String,
        location: SourceLocation,
    },
}
```

### 2. Warning System

```rust
#[derive(Debug)]
pub enum SemanticWarning {
    UnusedVariable { 
        name: String, 
        location: SourceLocation 
    },
    UnusedFunction { 
        name: String, 
        location: SourceLocation 
    },
    DeadCode { 
        location: SourceLocation,
        reason: String,
    },
    DeprecatedFeature { 
        feature: String,
        alternative: String,
        location: SourceLocation,
    },
    PotentialNullPointer { 
        expression: String,
        location: SourceLocation,
    },
    TypeInferenceAmbiguity { 
        variable: String,
        possible_types: Vec<Type>,
        location: SourceLocation,
    },
}
```

### 3. Error Recovery

The semantic analyzer includes recovery mechanisms:

```rust
impl SemanticAnalyzer {
    fn handle_semantic_error(&mut self, error: SemanticError) -> Type {
        self.errors.push(CompilerError::Semantic(error));
        
        // Return error type to allow continued analysis
        Type::Error
    }
    
    fn suggest_similar_names(&self, name: &str, available_names: &[String]) -> Vec<String> {
        available_names
            .iter()
            .filter(|available| self.levenshtein_distance(name, available) <= 2)
            .cloned()
            .collect()
    }
    
    fn create_placeholder_type(&mut self, context: &str) -> Type {
        self.warnings.push(CompilerWarning::TypeInferenceFailure {
            context: context.to_string(),
            fallback_type: Type::Any,
        });
        
        Type::Any
    }
}
```

## Integration with Code Generation

### 1. Type Information Preservation

The semantic analyzer preserves type information for code generation:

```rust
pub struct TypedASTNode {
    pub node: ASTNode,
    pub node_type: Type,
    pub location: SourceLocation,
    pub semantic_info: SemanticInfo,
}

pub struct SemanticInfo {
    pub resolved_symbols: HashMap<String, SymbolInfo>,
    pub type_constraints: Vec<TypeConstraint>,
    pub usage_info: UsageInfo,
}

pub struct SymbolInfo {
    pub symbol_type: Type,
    pub definition_location: SourceLocation,
    pub is_mutable: bool,
    pub scope_level: usize,
}
```

### 2. Function Signature Resolution

Provides resolved function signatures for code generation:

```rust
pub struct ResolvedFunctionCall {
    pub function_name: String,
    pub resolved_overload: FunctionOverload,
    pub argument_types: Vec<Type>,
    pub return_type: Type,
    pub requires_type_conversion: Vec<(usize, Type, Type)>, // arg_index, from_type, to_type
}
```

## Testing and Validation

### 1. Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_type_compatibility() {
        assert!(Type::Integer.is_compatible_with(&Type::Number));
        assert!(Type::Any.is_compatible_with(&Type::String));
        assert!(!Type::String.is_compatible_with(&Type::Integer));
    }
    
    #[test]
    fn test_function_resolution() {
        let mut analyzer = SemanticAnalyzer::new();
        
        // Register function overloads
        analyzer.register_function("test", vec![Type::Integer], Type::String);
        analyzer.register_function("test", vec![Type::Number], Type::String);
        
        // Test resolution
        let result = analyzer.resolve_function_call(
            "test", 
            &[Type::Integer], 
            &SourceLocation::default()
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap().return_type, Type::String);
    }
    
    #[test]
    fn test_scope_management() {
        let mut scope = Scope::new(ScopeType::Global, 0);
        
        // Define variable in global scope
        scope.define_variable("x".to_string(), Type::Integer, SourceLocation::default()).unwrap();
        
        // Enter function scope
        scope.enter_scope(ScopeType::Function, 1);
        
        // Variable should be accessible
        assert!(scope.lookup_variable("x").is_some());
        
        // Define local variable
        scope.define_variable("y".to_string(), Type::String, SourceLocation::default()).unwrap();
        
        // Exit function scope
        scope.exit_scope().unwrap();
        
        // Local variable should not be accessible
        assert!(scope.lookup_variable("y").is_none());
        assert!(scope.lookup_variable("x").is_some());
    }
}
```

### 2. Integration Tests

```bash
# Test semantic analysis pipeline
cargo test --test semantic_analysis_tests

# Test error reporting
cargo test --test semantic_error_tests

# Test performance with large codebases
cargo test --test semantic_performance --release
```

## Best Practices for Claude

When working with the semantic analysis system:

1. **Comprehensive Type Checking**: Always validate type compatibility before operations
2. **Proper Error Recovery**: Use error types to continue analysis after errors
3. **Scope Management**: Maintain proper scope boundaries and variable visibility
4. **Performance Considerations**: Be mindful of analysis complexity for large codebases
5. **Error Message Quality**: Provide helpful error messages with suggestions
6. **Testing**: Add comprehensive tests for any new semantic features
7. **Documentation**: Keep semantic rules well-documented for maintainability

## Future Enhancements

### 1. Advanced Type Inference

- **Hindley-Milner Type Inference**: More sophisticated type inference algorithm
- **Generic Type Inference**: Better support for generic type parameters
- **Flow-Sensitive Typing**: Type refinement based on control flow
- **Union Types**: Support for union and intersection types

### 2. Enhanced Analysis

- **Data Flow Analysis**: Track data flow for optimization and error detection
- **Escape Analysis**: Determine object lifetime for memory optimization
- **Purity Analysis**: Identify pure functions for optimization
- **Effect Tracking**: Track side effects for better async analysis

### 3. IDE Integration

- **Language Server Protocol**: Real-time semantic analysis for IDEs
- **Quick Fixes**: Automatic code fixes for common semantic errors
- **Refactoring Support**: Semantic-aware code refactoring
- **Code Navigation**: Go-to-definition and find-references functionality

This semantic analysis system provides a robust foundation for ensuring Clean Language's type safety and semantic correctness while maintaining excellent error reporting and recovery capabilities.