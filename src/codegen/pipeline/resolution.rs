//! Type resolution and symbol management phase of the compilation pipeline
//! 
//! This phase takes the analysis results and performs type resolution, symbol
//! table construction, and semantic validation. It prepares a fully resolved
//! context for code generation.

use crate::ast::{Type, Expression, Statement, Function as AstFunction, Value};
use crate::error::CompilerError;
use super::{CompilationPhase, analysis::AnalysisResult, shared::{Symbol, FunctionSignature, ClassInfo}};
use std::collections::HashMap;

/// Context containing fully resolved symbols and types
#[derive(Debug)]
pub struct ResolutionContext {
    pub symbol_table: SymbolTable,
    pub type_registry: TypeRegistry,
    pub resolved_functions: Vec<ResolvedFunction>,
    pub resolved_classes: Vec<ResolvedClass>,
    pub builtin_functions: HashMap<String, BuiltinFunction>,
}

/// Hierarchical symbol table with scope management
#[derive(Debug)]
pub struct SymbolTable {
    scopes: Vec<HashMap<String, Symbol>>,
    current_scope_level: u32,
    class_context: Option<String>,
    function_context: Option<String>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()], // Global scope
            current_scope_level: 0,
            class_context: None,
            function_context: None,
        }
    }
    
    pub fn enter_scope(&mut self) {
        self.current_scope_level += 1;
        self.scopes.push(HashMap::new());
    }
    
    pub fn exit_scope(&mut self) {
        if self.current_scope_level > 0 {
            self.scopes.pop();
            self.current_scope_level -= 1;
        }
    }
    
    pub fn declare_symbol(&mut self, name: String, symbol: Symbol) -> Result<(), CompilerError> {
        if let Some(current_scope) = self.scopes.last_mut() {
            if current_scope.contains_key(&name) {
                return Err(CompilerError::codegen_error(&format!("Symbol '{}' already declared in current scope", name), None, None));
            }
            current_scope.insert(name, symbol);
            Ok(())
        } else {
            Err(CompilerError::codegen_error("No active scope for symbol declaration", None, None))
        }
    }
    
    pub fn lookup_symbol(&self, name: &str) -> Option<&Symbol> {
        // Search from innermost to outermost scope
        for scope in self.scopes.iter().rev() {
            if let Some(symbol) = scope.get(name) {
                return Some(symbol);
            }
        }
        None
    }
    
    pub fn set_class_context(&mut self, class_name: Option<String>) {
        self.class_context = class_name;
    }
    
    pub fn set_function_context(&mut self, function_name: Option<String>) {
        self.function_context = function_name;
    }
    
    pub fn get_class_context(&self) -> Option<&String> {
        self.class_context.as_ref()
    }
    
    pub fn get_function_context(&self) -> Option<&String> {
        self.function_context.as_ref()
    }
}

/// Registry for type information and validation
#[derive(Debug)]
pub struct TypeRegistry {
    primitive_types: HashMap<String, Type>,
    class_types: HashMap<String, ResolvedClass>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut primitive_types = HashMap::new();
        primitive_types.insert("integer".to_string(), Type::Integer);
        primitive_types.insert("number".to_string(), Type::Number);
        primitive_types.insert("string".to_string(), Type::String);
        primitive_types.insert("boolean".to_string(), Type::Boolean);
        
        Self {
            primitive_types,
            class_types: HashMap::new(),
        }
    }
    
    pub fn register_class(&mut self, class: ResolvedClass) {
        self.class_types.insert(class.info.name.clone(), class);
    }
    
    pub fn lookup_type(&self, type_name: &str) -> Option<&Type> {
        self.primitive_types.get(type_name)
    }
    
    pub fn lookup_class(&self, class_name: &str) -> Option<&ResolvedClass> {
        self.class_types.get(class_name)
    }
    
    pub fn is_compatible(&self, from: &Type, to: &Type) -> bool {
        match (from, to) {
            // Exact matches
            (Type::Integer, Type::Integer) |
            (Type::Number, Type::Number) |
            (Type::String, Type::String) |
            (Type::Boolean, Type::Boolean) => true,
            
            // Numeric promotions
            (Type::Integer, Type::Number) => true,
            
            // Array compatibility
            (Type::List(from_elem), Type::List(to_elem)) => {
                self.is_compatible(from_elem, to_elem)
            },
            
            // Class compatibility (including inheritance)
            (Type::Class { name: from_class, .. }, Type::Class { name: to_class, .. }) => {
                from_class == to_class || self.is_subclass(from_class, to_class)
            },
            
            // Object type compatibility
            (Type::Class { .. }, Type::Object(_)) | (Type::Object(_), Type::Class { .. }) => true,
            
            _ => false,
        }
    }
    
    fn is_subclass(&self, child: &str, parent: &str) -> bool {
        if let Some(child_class) = self.lookup_class(child) {
            if let Some(ref parent_name) = child_class.info.parent {
                parent_name == parent || self.is_subclass(parent_name, parent)
            } else {
                false
            }
        } else {
            false
        }
    }
}

/// Fully resolved function with type information
#[derive(Debug, Clone)]
pub struct ResolvedFunction {
    pub signature: FunctionSignature,
    pub local_variables: Vec<Symbol>,
    pub resolved_body: Vec<ResolvedStatement>,
    pub wasm_type_index: Option<u32>,
}

/// Fully resolved class with inheritance information
#[derive(Debug, Clone)]
pub struct ResolvedClass {
    pub info: ClassInfo,
    pub resolved_fields: Vec<Symbol>,
    pub resolved_methods: Vec<ResolvedFunction>,
    pub inheritance_chain: Vec<String>,
    pub field_offsets: HashMap<String, u32>,
}

/// Statement with resolved types and symbols
#[derive(Debug, Clone)]
pub struct ResolvedStatement {
    pub original: Statement,
    pub resolved_type: Option<Type>,
}

/// Expression with resolved types
#[derive(Debug, Clone)]
pub struct ResolvedExpression {
    pub original: Expression,
    pub resolved_type: Type,
}

/// Built-in function definition
#[derive(Debug, Clone)]
pub struct BuiltinFunction {
    pub name: String,
    pub parameters: Vec<Type>,
    pub return_type: Option<Type>,
    pub module: String,
    pub is_variadic: bool,
}

/// Type resolver implementing the second phase of compilation
pub struct TypeResolver {
    symbol_table: SymbolTable,
    type_registry: TypeRegistry,
    builtin_functions: HashMap<String, BuiltinFunction>,
}

impl TypeResolver {
    pub fn new() -> Self {
        Self {
            symbol_table: SymbolTable::new(),
            type_registry: TypeRegistry::new(),
            builtin_functions: HashMap::new(),
        }
    }
    
    fn initialize_builtin_functions() -> HashMap<String, BuiltinFunction> {
        let mut builtins = HashMap::new();
        
        // I/O functions
        builtins.insert("print".to_string(), BuiltinFunction {
            name: "print".to_string(),
            parameters: vec![Type::String],
            return_type: None,
            module: "stdlib".to_string(),
            is_variadic: false,
        });
        
        builtins.insert("println".to_string(), BuiltinFunction {
            name: "println".to_string(),
            parameters: vec![Type::String],
            return_type: None,
            module: "stdlib".to_string(),
            is_variadic: false,
        });
        
        // String functions
        builtins.insert("toString".to_string(), BuiltinFunction {
            name: "toString".to_string(),
            parameters: vec![], // Will be resolved based on context
            return_type: Some(Type::String),
            module: "stdlib".to_string(),
            is_variadic: false,
        });
        
        // Math functions
        builtins.insert("sqrt".to_string(), BuiltinFunction {
            name: "sqrt".to_string(),
            parameters: vec![Type::Number],
            return_type: Some(Type::Number),
            module: "math".to_string(),
            is_variadic: false,
        });
        
        builtins.insert("pow".to_string(), BuiltinFunction {
            name: "pow".to_string(),
            parameters: vec![Type::Number, Type::Number],
            return_type: Some(Type::Number),
            module: "math".to_string(),
            is_variadic: false,
        });
        
        // Array functions
        builtins.insert("length".to_string(), BuiltinFunction {
            name: "length".to_string(),
            parameters: vec![], // Will be resolved based on array type
            return_type: Some(Type::Integer),
            module: "array".to_string(),
            is_variadic: false,
        });
        
        builtins
    }
    
    fn resolve_classes(&mut self, classes: &[ClassInfo]) -> Result<Vec<ResolvedClass>, CompilerError> {
        let mut resolved_classes = Vec::new();
        
        // First pass: Register all classes
        for class in classes {
            self.type_registry.register_class(ResolvedClass {
                info: class.clone(),
                resolved_fields: vec![],
                resolved_methods: vec![],
                inheritance_chain: vec![],
                field_offsets: HashMap::new(),
            });
        }
        
        // Second pass: Resolve inheritance and fields
        for class in classes {
            let inheritance_chain = self.build_inheritance_chain(&class.name)?;
            let resolved_fields = self.resolve_class_fields(class, &inheritance_chain)?;
            let field_offsets = self.calculate_field_offsets(&resolved_fields);
            
            let resolved_class = ResolvedClass {
                info: class.clone(),
                resolved_fields,
                resolved_methods: vec![], // Will be resolved later
                inheritance_chain,
                field_offsets,
            };
            
            resolved_classes.push(resolved_class);
        }
        
        Ok(resolved_classes)
    }
    
    fn build_inheritance_chain(&self, class_name: &str) -> Result<Vec<String>, CompilerError> {
        let mut chain = vec![class_name.to_string()];
        let mut current = class_name;
        let mut visited = std::collections::HashSet::new();
        
        while let Some(class) = self.type_registry.lookup_class(current) {
            if let Some(ref parent) = class.info.parent {
                if visited.contains(parent) {
                    return Err(CompilerError::codegen_error(&format!(
                        "Circular inheritance detected in class '{}'", current
                    ), None, None));
                }
                visited.insert(current.to_string());
                chain.push(parent.clone());
                current = parent;
            } else {
                break;
            }
        }
        
        chain.reverse(); // Base class first
        Ok(chain)
    }
    
    fn resolve_class_fields(&self, class: &ClassInfo, inheritance_chain: &[String]) -> Result<Vec<Symbol>, CompilerError> {
        let mut fields = Vec::new();
        
        // Add fields from inheritance chain (base first)
        for class_name in inheritance_chain {
            if let Some(resolved_class) = self.type_registry.lookup_class(class_name) {
                fields.extend(resolved_class.info.fields.clone());
            }
        }
        
        Ok(fields)
    }
    
    fn calculate_field_offsets(&self, fields: &[Symbol]) -> HashMap<String, u32> {
        let mut offsets = HashMap::new();
        let mut current_offset = 0u32;
        
        for field in fields {
            offsets.insert(field.name.clone(), current_offset);
            current_offset += self.type_size(&field.symbol_type);
        }
        
        offsets
    }
    
    fn type_size(&self, type_: &Type) -> u32 {
        match type_ {
            Type::Integer => 4,
            Type::Number => 8,
            Type::Boolean => 1,
            Type::String => 4, // Pointer to string data
            Type::List(_) => 4, // Pointer to array data
            Type::Object(_) | Type::Class { .. } => 4, // Pointer to object data
            _ => 4, // Default size
        }
    }
    
    fn resolve_function_body(&mut self, function: &AstFunction) -> Result<Vec<ResolvedStatement>, CompilerError> {
        let mut resolved_statements = Vec::new();
        
        // Set function context
        self.symbol_table.set_function_context(Some(function.name.clone()));
        self.symbol_table.enter_scope();
        
        // Declare parameters in scope
        for param in &function.parameters {
            let symbol = Symbol {
                name: param.name.clone(),
                symbol_type: param.type_.clone(),
                scope_level: self.symbol_table.current_scope_level,
                is_mutable: false, // Parameters are immutable by default
            };
            self.symbol_table.declare_symbol(param.name.clone(), symbol)?;
        }
        
        // Resolve function body
        for statement in &function.body {
            let resolved_stmt = self.resolve_statement(statement)?;
            resolved_statements.push(resolved_stmt);
        }
        
        self.symbol_table.exit_scope();
        self.symbol_table.set_function_context(None);
        
        Ok(resolved_statements)
    }
    
    fn resolve_statement(&mut self, statement: &Statement) -> Result<ResolvedStatement, CompilerError> {
        let resolved_type = match statement {
            Statement::VariableDecl { name, type_, initializer, .. } => {
                let declared_type = type_.clone();
                
                let symbol = Symbol {
                    name: name.clone(),
                    symbol_type: declared_type.clone(),
                    scope_level: self.symbol_table.current_scope_level,
                    is_mutable: true,
                };
                
                self.symbol_table.declare_symbol(name.clone(), symbol)?;
                Some(declared_type)
            },
            
            Statement::Expression { expr, .. } => {
                Some(self.infer_expression_type(expr)?)
            },
            
            Statement::Return { value: Some(expr), .. } => {
                Some(self.infer_expression_type(expr)?)
            },
            
            _ => None,
        };
        
        Ok(ResolvedStatement {
            original: statement.clone(),
            resolved_type,
        })
    }
    
    fn infer_expression_type(&self, expression: &Expression) -> Result<Type, CompilerError> {
        match expression {
            Expression::Literal(value) => {
                Ok(match value {
                    crate::ast::Value::Integer(_) => Type::Integer,
                    crate::ast::Value::Number(_) => Type::Number,
                    crate::ast::Value::String(_) => Type::String,
                    crate::ast::Value::Boolean(_) => Type::Boolean,
                    crate::ast::Value::List(_) => Type::List(Box::new(Type::Object("Unknown".to_string()))),
                    crate::ast::Value::Matrix(_) => Type::List(Box::new(Type::List(Box::new(Type::Number)))),
                })
            },
            
            Expression::Variable(name) => {
                if let Some(symbol) = self.symbol_table.lookup_symbol(name) {
                    Ok(symbol.symbol_type.clone())
                } else {
                    Err(CompilerError::codegen_error(&format!("Undefined variable: {}", name), None, None))
                }
            },
            
            Expression::Call(function, arguments) => {
                // Check if it's a builtin function
                if let Some(builtin) = self.builtin_functions.get(function) {
                    Ok(builtin.return_type.clone().unwrap_or(Type::Object("Unknown".to_string())))
                } else {
                    // Look up user-defined function
                    // For now, return Object type
                    Ok(Type::Object("Unknown".to_string()))
                }
            },
            
            Expression::Binary(left, operator, right) => {
                use crate::ast::BinaryOperator;
                match operator {
                    BinaryOperator::Add | BinaryOperator::Subtract | 
                    BinaryOperator::Multiply | BinaryOperator::Divide => {
                        let left_type = self.infer_expression_type(left)?;
                        let right_type = self.infer_expression_type(right)?;
                        
                        match (&left_type, &right_type) {
                            (Type::Integer, Type::Integer) => Ok(Type::Integer),
                            (Type::Number, _) | (_, Type::Number) => Ok(Type::Number),
                            _ => Ok(Type::Number), // Default to number for arithmetic
                        }
                    },
                    
                    BinaryOperator::Equal | BinaryOperator::NotEqual |
                    BinaryOperator::Less | BinaryOperator::LessEqual |
                    BinaryOperator::Greater | BinaryOperator::GreaterEqual => {
                        Ok(Type::Boolean)
                    },
                    
                    BinaryOperator::And | BinaryOperator::Or => Ok(Type::Boolean),
                    
                    // Handle remaining binary operators
                    _ => Ok(Type::Object("Unknown".to_string())),
                }
            },
            
            Expression::PropertyAccess { object, property, .. } => {
                let object_type = self.infer_expression_type(object)?;
                match object_type {
                    Type::Class { name: class_name, .. } => {
                        if let Some(class) = self.type_registry.lookup_class(&class_name) {
                            for field in &class.resolved_fields {
                                if field.name == *property {
                                    return Ok(field.symbol_type.clone());
                                }
                            }
                        }
                        Err(CompilerError::codegen_error(&format!(
                            "Property '{}' not found in class '{}'", property, class_name
                        ), None, None))
                    },
                    _ => Ok(Type::Object("Unknown".to_string())), // Generic property access
                }
            },
            
            Expression::Literal(Value::List(_)) => {
                // For list literals, we need to infer the element type
                // For now, assume generic object type
                Ok(Type::List(Box::new(Type::Object("Unknown".to_string()))))
            },
            
            _ => Ok(Type::Object("Unknown".to_string())), // Default fallback
        }
    }
}

impl CompilationPhase<AnalysisResult, ResolutionContext> for TypeResolver {
    type Error = CompilerError;
    
    fn execute(&mut self, analysis: AnalysisResult) -> Result<ResolutionContext, Self::Error> {
        // Initialize builtin functions
        let builtin_functions = Self::initialize_builtin_functions();
        
        // Resolve classes first
        let resolved_classes = self.resolve_classes(&analysis.classes)?;
        
        // Register resolved classes in type registry
        for class in &resolved_classes {
            self.type_registry.register_class(class.clone());
        }
        
        // Resolve functions
        let mut resolved_functions = Vec::new();
        for function_sig in &analysis.functions {
            // Find the corresponding AST function (this would need to be passed from analysis)
            // For now, create a placeholder
            let resolved_function = ResolvedFunction {
                signature: function_sig.clone(),
                local_variables: vec![],
                resolved_body: vec![],
                wasm_type_index: None,
            };
            resolved_functions.push(resolved_function);
        }
        
        Ok(ResolutionContext {
            symbol_table: self.symbol_table.clone(),
            type_registry: self.type_registry.clone(),
            resolved_functions,
            resolved_classes,
            builtin_functions,
        })
    }
}

// Clone implementations for the SymbolTable and TypeRegistry
impl Clone for SymbolTable {
    fn clone(&self) -> Self {
        Self {
            scopes: self.scopes.clone(),
            current_scope_level: self.current_scope_level,
            class_context: self.class_context.clone(),
            function_context: self.function_context.clone(),
        }
    }
}

impl Clone for TypeRegistry {
    fn clone(&self) -> Self {
        Self {
            primitive_types: self.primitive_types.clone(),
            class_types: self.class_types.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Parameter, Function as AstFunction, Program};

    #[test]
    fn test_symbol_table() {
        let mut table = SymbolTable::new();
        
        let symbol = Symbol {
            name: "x".to_string(),
            symbol_type: Type::Integer,
            scope_level: 0,
            is_mutable: true,
        };
        
        table.declare_symbol("x".to_string(), symbol).unwrap();
        assert!(table.lookup_symbol("x").is_some());
        
        table.enter_scope();
        assert!(table.lookup_symbol("x").is_some()); // Should find in parent scope
    }

    #[test]
    fn test_type_compatibility() {
        let registry = TypeRegistry::new();
        
        assert!(registry.is_compatible(&Type::Integer, &Type::Number));
        assert!(!registry.is_compatible(&Type::String, &Type::Integer));
        assert!(registry.is_compatible(&Type::Integer, &Type::Integer));
    }
}