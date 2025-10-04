//! Program analysis phase of the compilation pipeline
//! 
//! This phase extracts structural information from the AST without performing
//! type resolution or code generation. It builds symbol tables, analyzes
//! dependencies, and prepares data structures for the resolution phase.

use crate::ast::{Program, Function as AstFunction, Expression, Statement, Class, Visibility, Value};
use crate::error::CompilerError;
use super::{CompilationPhase, shared::{Symbol, FunctionSignature, ClassInfo}};
use std::collections::{HashMap, HashSet, VecDeque};

/// Result of the analysis phase containing all extracted program structure
#[derive(Debug)]
pub struct AnalysisResult {
    pub functions: Vec<FunctionSignature>,
    pub classes: Vec<ClassInfo>, 
    pub global_symbols: HashMap<String, Symbol>,
    pub dependency_graph: DependencyGraph,
    pub imports: Vec<ImportInfo>,
    pub exports: Vec<String>,
}

/// Import information extracted from the program
#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub module_name: String,
    pub function_name: String,
    pub signature: FunctionSignature,
}

/// Dependency analysis for proper compilation ordering
#[derive(Debug)]
pub struct DependencyGraph {
    dependencies: HashMap<String, HashSet<String>>,
    resolved_order: Vec<String>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            dependencies: HashMap::new(),
            resolved_order: Vec::new(),
        }
    }
    
    pub fn add_dependency(&mut self, dependent: String, dependency: String) {
        self.dependencies
            .entry(dependent)
            .or_insert_with(HashSet::new)
            .insert(dependency);
    }
    
    pub fn resolve_order(&mut self) -> Result<&[String], CompilerError> {
        if !self.resolved_order.is_empty() {
            return Ok(&self.resolved_order);
        }
        
        // Topological sort using Kahn's algorithm
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut queue = VecDeque::new();
        
        // Initialize in-degrees
        for (node, deps) in &self.dependencies {
            in_degree.entry(node.clone()).or_insert(0);
            for dep in deps {
                *in_degree.entry(dep.clone()).or_insert(0) += 1;
            }
        }
        
        // Find nodes with no incoming edges
        for (node, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(node.clone());
            }
        }
        
        while let Some(node) = queue.pop_front() {
            self.resolved_order.push(node.clone());
            
            if let Some(dependencies) = self.dependencies.get(&node) {
                for dep in dependencies {
                    if let Some(degree) = in_degree.get_mut(dep) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(dep.clone());
                        }
                    }
                }
            }
        }
        
        // Check for circular dependencies
        if self.resolved_order.len() != in_degree.len() {
            return Err(CompilerError::codegen_error("Circular dependency detected in program", None, None));
        }
        
        Ok(&self.resolved_order)
    }
    
    pub fn get_dependencies(&self, item: &str) -> Option<&HashSet<String>> {
        self.dependencies.get(item)
    }
}

/// Program analyzer implementing the first phase of compilation
pub struct ProgramAnalyzer {
    current_class: Option<String>,
    current_function: Option<String>,
}

impl ProgramAnalyzer {
    pub fn new() -> Self {
        Self {
            current_class: None,
            current_function: None,
        }
    }
    
    fn analyze_function(&self, function: &AstFunction) -> FunctionSignature {
        FunctionSignature {
            name: function.name.clone(),
            parameters: function.parameters.iter().map(|p| p.type_.clone()).collect(),
            return_type: Some(function.return_type.clone()),
            is_exported: function.name == "start" || matches!(function.visibility, Visibility::Public),
        }
    }
    
    fn analyze_class(&self, class: &Class) -> ClassInfo {
        let fields = class.fields.iter().map(|field| Symbol {
            name: field.name.clone(),
            symbol_type: field.type_.clone(),
            scope_level: 0, // Class level scope
            is_mutable: true, // Class fields are mutable by default
        }).collect();
        
        let methods = class.methods.iter().map(|method| self.analyze_function(method)).collect();
        
        ClassInfo {
            name: class.name.clone(),
            fields,
            methods,
            parent: class.base_class.clone(),
        }
    }
    
    fn extract_dependencies(&self, expression: &Expression, dependencies: &mut HashSet<String>) {
        match expression {
            Expression::Variable(name) => {
                dependencies.insert(name.clone());
            },
            Expression::Call(function, arguments) => {
                dependencies.insert(function.clone());
                for arg in arguments {
                    self.extract_dependencies(arg, dependencies);
                }
            },
            Expression::PropertyAccess { object, .. } => {
                self.extract_dependencies(object, dependencies);
            },
            Expression::MethodCall { object, arguments, .. } => {
                self.extract_dependencies(object, dependencies);
                for arg in arguments {
                    self.extract_dependencies(arg, dependencies);
                }
            },
            Expression::Binary(left, _, right) => {
                self.extract_dependencies(left, dependencies);
                self.extract_dependencies(right, dependencies);
            },
            Expression::Unary(_, operand) => {
                self.extract_dependencies(operand, dependencies);
            },
            Expression::Conditional { condition, then_expr, else_expr, .. } => {
                self.extract_dependencies(condition, dependencies);
                self.extract_dependencies(then_expr, dependencies);
                self.extract_dependencies(else_expr, dependencies);
            },
            Expression::Literal(Value::List(elements)) => {
                // For list literals, we need to process the values
                // but they're already resolved so no dependencies to extract
            },
            Expression::Literal(Value::Matrix(_)) => {
                // For matrix literals, values are already resolved
            },
            // Base cases - no dependencies
            Expression::Literal(_) | Expression::StringInterpolation(_) => {},
        }
    }
    
    fn analyze_dependencies(&self, functions: &[AstFunction]) -> DependencyGraph {
        let mut graph = DependencyGraph::new();
        
        for function in functions {
            let mut dependencies = HashSet::new();
            
            // Extract dependencies from function body
            for statement in &function.body {
                self.extract_dependencies_from_statement(statement, &mut dependencies);
            }
            
            // Add to dependency graph
            for dep in dependencies {
                if dep != function.name {
                    graph.add_dependency(function.name.clone(), dep);
                }
            }
        }
        
        graph
    }
    
    fn extract_dependencies_from_statement(&self, statement: &Statement, dependencies: &mut HashSet<String>) {
        match statement {
            Statement::Expression { expr, .. } => {
                self.extract_dependencies(expr, dependencies);
            },
            Statement::VariableDecl { initializer: Some(expr), .. } => {
                self.extract_dependencies(expr, dependencies);
            },
            Statement::Assignment { value, .. } => {
                self.extract_dependencies(value, dependencies);
            },
            Statement::If { condition, then_branch, else_branch, .. } => {
                self.extract_dependencies(condition, dependencies);
                for stmt in then_branch {
                    self.extract_dependencies_from_statement(stmt, dependencies);
                }
                if let Some(else_stmts) = else_branch {
                    for stmt in else_stmts {
                        self.extract_dependencies_from_statement(stmt, dependencies);
                    }
                }
            },
            Statement::While { condition, body, .. } => {
                self.extract_dependencies(condition, dependencies);
                for stmt in body {
                    self.extract_dependencies_from_statement(stmt, dependencies);
                }
            },
            Statement::Return { value: Some(expr), .. } => {
                self.extract_dependencies(expr, dependencies);
            },
            // Other statement types
            _ => {},
        }
    }
}

impl CompilationPhase<&Program, AnalysisResult> for ProgramAnalyzer {
    type Error = CompilerError;
    
    fn execute(&mut self, program: &Program) -> Result<AnalysisResult, Self::Error> {
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut global_symbols = HashMap::new();
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        
        // Analyze functions
        for function in &program.functions {
            let sig = self.analyze_function(function);
            if sig.is_exported {
                exports.push(sig.name.clone());
            }
            functions.push(sig);
        }
        
        // Analyze classes
        for class in &program.classes {
            let class_info = self.analyze_class(class);
            
            // Add class methods to exports if they're public
            for method in &class_info.methods {
                if method.is_exported {
                    exports.push(format!("{}_{}", class_info.name, method.name));
                }
            }
            
            classes.push(class_info);
        }
        
        // Extract standard library and host function imports
        self.extract_imports(&program.functions, &mut imports);
        
        // Build dependency graph
        let dependency_graph = self.analyze_dependencies(&program.functions);
        
        Ok(AnalysisResult {
            functions,
            classes,
            global_symbols,
            dependency_graph,
            imports,
            exports,
        })
    }
}

impl ProgramAnalyzer {
    fn extract_imports(&self, functions: &[AstFunction], imports: &mut Vec<ImportInfo>) {
        // Standard library functions that need to be imported
        let stdlib_functions = [
            "print", "println", "input", "toString", "parseInt", "parseFloat",
            "length", "push", "pop", "slice", "indexOf", "contains",
            "sqrt", "pow", "abs", "min", "max", "floor", "ceil", "round"
        ];
        
        for func_name in &stdlib_functions {
            // Check if any function uses this stdlib function
            let is_used = functions.iter().any(|f| self.function_uses_symbol(f, func_name));
            
            if is_used {
                imports.push(ImportInfo {
                    module_name: "stdlib".to_string(),
                    function_name: func_name.to_string(),
                    signature: FunctionSignature {
                        name: func_name.to_string(),
                        parameters: vec![], // Will be resolved in resolution phase
                        return_type: None,  // Will be resolved in resolution phase
                        is_exported: false,
                    },
                });
            }
        }
    }
    
    fn function_uses_symbol(&self, function: &AstFunction, symbol: &str) -> bool {
        // Simplified check - would need more sophisticated analysis
        function.body.iter().any(|stmt| self.statement_uses_symbol(stmt, symbol))
    }
    
    fn statement_uses_symbol(&self, statement: &Statement, symbol: &str) -> bool {
        match statement {
            Statement::Expression { expr, .. } => self.expression_uses_symbol(expr, symbol),
            Statement::VariableDecl { initializer: Some(expr), .. } => self.expression_uses_symbol(expr, symbol),
            Statement::Assignment { value, .. } => self.expression_uses_symbol(value, symbol),
            Statement::If { condition, then_branch, else_branch, .. } => {
                self.expression_uses_symbol(condition, symbol) ||
                then_branch.iter().any(|stmt| self.statement_uses_symbol(stmt, symbol)) ||
                else_branch.as_ref().map_or(false, |stmts| stmts.iter().any(|stmt| self.statement_uses_symbol(stmt, symbol)))
            },
            Statement::While { condition, body, .. } => {
                self.expression_uses_symbol(condition, symbol) ||
                body.iter().any(|stmt| self.statement_uses_symbol(stmt, symbol))
            },
            Statement::Return { value: Some(expr), .. } => self.expression_uses_symbol(expr, symbol),
            _ => false,
        }
    }
    
    fn expression_uses_symbol(&self, expression: &Expression, symbol: &str) -> bool {
        match expression {
            Expression::Variable(name) => name == symbol,
            Expression::Call(function, arguments) => {
                function == symbol || arguments.iter().any(|arg| self.expression_uses_symbol(arg, symbol))
            },
            Expression::PropertyAccess { object, .. } => self.expression_uses_symbol(object, symbol),
            Expression::MethodCall { object, method, arguments, .. } => {
                method == symbol ||
                self.expression_uses_symbol(object, symbol) ||
                arguments.iter().any(|arg| self.expression_uses_symbol(arg, symbol))
            },
            Expression::Binary(left, _, right) => {
                self.expression_uses_symbol(left, symbol) || self.expression_uses_symbol(right, symbol)
            },
            Expression::Unary(_, operand) => self.expression_uses_symbol(operand, symbol),
            Expression::Conditional { condition, then_expr, else_expr, .. } => {
                self.expression_uses_symbol(condition, symbol) ||
                self.expression_uses_symbol(then_expr, symbol) ||
                self.expression_uses_symbol(else_expr, symbol)
            },
            Expression::Literal(Value::List(_)) => false,
            Expression::Literal(Value::Matrix(_)) => false,
            Expression::Literal(_) | Expression::StringInterpolation(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Program, Parameter};

    #[test]
    fn test_dependency_graph() {
        let mut graph = DependencyGraph::new();
        graph.add_dependency("main".to_string(), "helper".to_string());
        graph.add_dependency("helper".to_string(), "util".to_string());
        
        let order = graph.resolve_order().unwrap();
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn test_program_analyzer() {
        let program = Program {
            functions: vec![AstFunction {
                name: "main".to_string(),
                parameters: vec![],
                return_type: None,
                body: vec![],
                is_public: Some(true),
                location: None,
            }],
            classes: vec![],
            start_function: Some("main".to_string()),
        };
        
        let mut analyzer = ProgramAnalyzer::new();
        let result = analyzer.execute(&program).unwrap();
        
        assert_eq!(result.functions.len(), 1);
        assert_eq!(result.functions[0].name, "main");
        assert!(result.functions[0].is_exported);
    }
}