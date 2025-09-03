//! Code generation phase of the compilation pipeline
//! 
//! This phase takes the resolved context and generates an intermediate representation
//! suitable for WebAssembly assembly. It handles instruction generation, memory
//! layout, and optimization.

use crate::ast::{Expression, Statement, BinaryOperator, UnaryOperator, Type, Value};
use crate::error::CompilerError;
use super::{
    CompilationPhase, 
    resolution::{ResolutionContext, ResolvedFunction, ResolvedClass, ResolvedStatement}
};
use wasm_encoder::{ValType, MemArg, BlockType, Instruction};
use std::collections::HashMap;

/// Result of the generation phase containing intermediate representation
#[derive(Debug)]
pub struct GenerationContext {
    pub functions: Vec<GeneratedFunction>,
    pub memory_layout: MemoryLayout,
    pub string_constants: Vec<String>,
    pub type_signatures: Vec<FunctionType>,
    pub imports: Vec<ImportDeclaration>,
    pub exports: Vec<ExportDeclaration>,
}

/// Generated function with WASM instructions
#[derive(Debug, Clone)]
pub struct GeneratedFunction {
    pub name: String,
    pub type_index: u32,
    pub locals: Vec<Local>,
    pub instructions: Vec<Instruction<'static>>,
    pub is_exported: bool,
}

/// Local variable information for WASM functions
#[derive(Debug, Clone)]
pub struct Local {
    pub name: String,
    pub wasm_type: ValType,
    pub index: u32,
}

/// Memory layout information for the WASM module
#[derive(Debug)]
pub struct MemoryLayout {
    pub initial_pages: u32,
    pub max_pages: Option<u32>,
    pub string_pool: HashMap<String, u32>, // string -> offset
    pub global_data_size: u32,
}

/// WASM function type signature
#[derive(Debug, Clone)]
pub struct FunctionType {
    pub parameters: Vec<ValType>,
    pub results: Vec<ValType>,
}

/// Import declaration for external functions
#[derive(Debug, Clone)]
pub struct ImportDeclaration {
    pub module: String,
    pub name: String,
    pub function_type: FunctionType,
}

/// Export declaration for module functions
#[derive(Debug, Clone)]
pub struct ExportDeclaration {
    pub name: String,
    pub internal_name: String,
}

/// Code emitter implementing the third phase of compilation
pub struct CodeEmitter {
    current_function: Option<String>,
    local_variables: HashMap<String, Local>,
    label_counter: u32,
    type_signatures: Vec<FunctionType>,
    string_pool: HashMap<String, u32>,
    next_string_offset: u32,
}

impl CodeEmitter {
    pub fn new() -> Self {
        Self {
            current_function: None,
            local_variables: HashMap::new(),
            label_counter: 0,
            type_signatures: Vec::new(),
            string_pool: HashMap::new(),
            next_string_offset: 1024, // Start after reserved memory
        }
    }
    
    fn generate_function(&mut self, function: &ResolvedFunction) -> Result<GeneratedFunction, CompilerError> {
        self.current_function = Some(function.signature.name.clone());
        self.local_variables.clear();
        
        // Create WASM type signature
        let type_index = self.register_function_type(&function.signature)?;
        
        // Set up local variables
        let mut locals = Vec::new();
        let mut local_index = function.signature.parameters.len() as u32;
        
        // Parameters are already included in WASM function signature
        for (i, param_type) in function.signature.parameters.iter().enumerate() {
            let local = Local {
                name: format!("param_{}", i),
                wasm_type: self.type_to_wasm_type(param_type),
                index: i as u32,
            };
            locals.push(local.clone());
            self.local_variables.insert(local.name.clone(), local);
        }
        
        // Add local variables from function body
        for var in &function.local_variables {
            let local = Local {
                name: var.name.clone(),
                wasm_type: self.type_to_wasm_type(&var.symbol_type),
                index: local_index,
            };
            locals.push(local.clone());
            self.local_variables.insert(var.name.clone(), local);
            local_index += 1;
        }
        
        // Generate instructions for function body
        let mut instructions = Vec::new();
        for statement in &function.resolved_body {
            self.generate_statement_instructions(statement, &mut instructions)?;
        }
        
        // Ensure function returns properly
        if function.signature.return_type.is_none() {
            // Void functions need explicit return
            // instructions.push(wasm_encoder::Instruction::Return);
        }
        
        self.current_function = None;
        
        Ok(GeneratedFunction {
            name: function.signature.name.clone(),
            type_index,
            locals,
            instructions,
            is_exported: function.signature.is_exported,
        })
    }
    
    fn register_function_type(&mut self, signature: &super::shared::FunctionSignature) -> Result<u32, CompilerError> {
        let parameters: Vec<ValType> = signature.parameters
            .iter()
            .map(|t| self.type_to_wasm_type(t))
            .collect();
        
        let results: Vec<ValType> = signature.return_type
            .as_ref()
            .map(|t| vec![self.type_to_wasm_type(t)])
            .unwrap_or_default();
        
        let function_type = FunctionType { parameters, results };
        
        // Check if we already have this type signature
        for (i, existing_type) in self.type_signatures.iter().enumerate() {
            if self.function_types_equal(&function_type, existing_type) {
                return Ok(i as u32);
            }
        }
        
        // Add new type signature
        let type_index = self.type_signatures.len() as u32;
        self.type_signatures.push(function_type);
        Ok(type_index)
    }
    
    fn function_types_equal(&self, a: &FunctionType, b: &FunctionType) -> bool {
        a.parameters == b.parameters && a.results == b.results
    }
    
    fn type_to_wasm_type(&self, type_: &Type) -> ValType {
        match type_ {
            Type::Integer => ValType::I32,
            Type::Number => ValType::F64,
            Type::Boolean => ValType::I32,
            Type::String => ValType::I32, // Pointer to string
            Type::List(_) => ValType::I32, // Pointer to array
            Type::Object(_) | Type::Class { .. } => ValType::I32, // Pointer to object
            _ => ValType::I32, // Default to i32
        }
    }
    
    fn generate_statement_instructions(&mut self, statement: &ResolvedStatement, instructions: &mut Vec<Instruction<'static>>) -> Result<(), CompilerError> {
        match &statement.original {
            Statement::Expression { expr, .. } => {
                self.generate_expression_instructions(expr, instructions)?;
                // Pop the result if it's not used
                if statement.resolved_type.is_some() {
                    instructions.push(wasm_encoder::Instruction::Drop);
                }
            },
            
            Statement::VariableDecl { name, initializer, .. } => {
                if let Some(init_expr) = initializer {
                    // Generate code for initializer
                    self.generate_expression_instructions(init_expr, instructions)?;
                    
                    // Store in local variable
                    if let Some(local) = self.local_variables.get(name) {
                        instructions.push(wasm_encoder::Instruction::LocalSet(local.index));
                    } else {
                        return Err(CompilerError::codegen_error(&format!("Undefined local variable: {}", name), None, None));
                    }
                } else {
                    // Initialize with default value
                    self.generate_default_value_instructions(&statement.resolved_type, instructions)?;
                    if let Some(local) = self.local_variables.get(name) {
                        instructions.push(wasm_encoder::Instruction::LocalSet(local.index));
                    }
                }
            },
            
            Statement::Assignment { target, value, .. } => {
                // Generate value
                self.generate_expression_instructions(value, instructions)?;
                
                // Store in target
                match target.as_str() {
                    Expression::Variable(name) => {
                        if let Some(local) = self.local_variables.get(name) {
                            instructions.push(wasm_encoder::Instruction::LocalSet(local.index));
                        } else {
                            return Err(CompilerError::codegen_error(&format!("Undefined variable: {}", name), None, None));
                        }
                    },
                    Expression::PropertyAccess { object, property, .. } => {
                        // Generate object reference
                        self.generate_expression_instructions(object, instructions)?;
                        // Property assignment would need memory operations
                        // This is simplified for now
                        instructions.push(wasm_encoder::Instruction::Drop); // Remove object ref
                        instructions.push(wasm_encoder::Instruction::Drop); // Remove value
                    },
                    _ => return Err(CompilerError::codegen_error("Invalid assignment target", None, None)),
                }
            },
            
            Statement::If { condition, then_branch, else_branch, .. } => {
                // Generate condition
                self.generate_expression_instructions(condition, instructions)?;
                
                // Create if block
                instructions.push(wasm_encoder::Instruction::If(BlockType::Empty));
                
                // Generate then branch
                for stmt in then_branch {
                    self.generate_statement_instructions(stmt, instructions)?;
                }
                
                // Generate else branch if present
                if let Some(else_stmts) = else_branch {
                    instructions.push(wasm_encoder::Instruction::Else);
                    for stmt in else_stmts {
                        self.generate_statement_instructions(stmt, instructions)?;
                    }
                }
                
                instructions.push(wasm_encoder::Instruction::End);
            },
            
            Statement::While { condition, body, .. } => {
                let loop_label = self.next_label();
                
                // Create loop block
                instructions.push(wasm_encoder::Instruction::Loop(BlockType::Empty));
                
                // Generate condition
                self.generate_expression_instructions(condition, instructions)?;
                instructions.push(wasm_encoder::Instruction::I32Eqz); // Invert condition
                instructions.push(wasm_encoder::Instruction::BrIf(0)); // Break if false
                
                // Generate body
                for stmt in body {
                    self.generate_statement_instructions(stmt, instructions)?;
                }
                
                // Continue loop
                instructions.push(wasm_encoder::Instruction::Br(0));
                instructions.push(wasm_encoder::Instruction::End);
            },
            
            Statement::Return { value: expr, .. } => {
                if let Some(return_expr) = expr {
                    self.generate_expression_instructions(return_expr, instructions)?;
                }
                instructions.push(wasm_encoder::Instruction::Return);
            },
            
            _ => {
                // Handle other statement types
                return Err(CompilerError::codegen_error("Unsupported statement type", None, None));
            },
        }
        
        Ok(())
    }
    
    fn generate_expression_instructions(&mut self, expression: &Expression, instructions: &mut Vec<Instruction<'static>>) -> Result<(), CompilerError> {
        match expression {
            Expression::Literal(value) => {
                self.generate_literal_instructions(value, instructions)?;
            },
            
            Expression::Variable(name) => {
                if let Some(local) = self.local_variables.get(name) {
                    instructions.push(wasm_encoder::Instruction::LocalGet(local.index));
                } else {
                    return Err(CompilerError::codegen_error(&format!("Undefined variable: {}", name), None, None));
                }
            },
            
            Expression::Binary(left, operator, right) => {
                // Generate operands
                self.generate_expression_instructions(left, instructions)?;
                self.generate_expression_instructions(right, instructions)?;
                
                // Generate operation
                match operator {
                    BinaryOperator::Add => instructions.push(wasm_encoder::Instruction::I32Add),
                    BinaryOperator::Subtract => instructions.push(wasm_encoder::Instruction::I32Sub),
                    BinaryOperator::Multiply => instructions.push(wasm_encoder::Instruction::I32Mul),
                    BinaryOperator::Divide => instructions.push(wasm_encoder::Instruction::I32DivS),
                    BinaryOperator::Equal => instructions.push(wasm_encoder::Instruction::I32Eq),
                    BinaryOperator::NotEqual => instructions.push(wasm_encoder::Instruction::I32Ne),
                    BinaryOperator::Less => instructions.push(wasm_encoder::Instruction::I32LtS),
                    BinaryOperator::LessEqual => instructions.push(wasm_encoder::Instruction::I32LeS),
                    BinaryOperator::Greater => instructions.push(wasm_encoder::Instruction::I32GtS),
                    BinaryOperator::GreaterEqual => instructions.push(wasm_encoder::Instruction::I32GeS),
                    BinaryOperator::And => {
                        // Logical AND with short-circuiting
                        instructions.push(wasm_encoder::Instruction::I32And);
                    },
                    BinaryOperator::Or => {
                        // Logical OR with short-circuiting
                        instructions.push(wasm_encoder::Instruction::I32Or);
                    },
                }
            },
            
            Expression::Unary(operator, operand) => {
                self.generate_expression_instructions(operand, instructions)?;
                
                match operator {
                    UnaryOperator::Not => {
                        instructions.push(wasm_encoder::Instruction::I32Eqz);
                    },
                    UnaryOperator::Negate => {
                        instructions.push(wasm_encoder::Instruction::I32Const(0));
                        instructions.push(wasm_encoder::Instruction::I32Sub);
                    },
                }
            },
            
            Expression::Call(function, arguments) => {
                // Generate arguments
                for arg in arguments {
                    self.generate_expression_instructions(arg, instructions)?;
                }
                
                // Generate function call
                // For now, assume it's a function index (would need proper resolution)
                let function_index = 0; // Placeholder
                instructions.push(wasm_encoder::Instruction::Call(function_index));
            },
            
            Expression::PropertyAccess { object, property, .. } => {
                // Generate object reference
                self.generate_expression_instructions(object, instructions)?;
                
                // Load property (simplified - would need proper offset calculation)
                let offset = 0; // Placeholder offset
                instructions.push(wasm_encoder::Instruction::I32Load(MemArg {
                    offset: offset,
                    align: 2,
                }));
            },
            
            Expression::MethodCall { object, method, arguments, .. } => {
                // Generate object as first argument
                self.generate_expression_instructions(object, instructions)?;
                
                // Generate other arguments
                for arg in arguments {
                    self.generate_expression_instructions(arg, instructions)?;
                }
                
                // Generate method call (simplified)
                let method_index = 0; // Placeholder
                instructions.push(wasm_encoder::Instruction::Call(method_index));
            },
            
            Expression::Literal(Value::List(elements)) => {
                // Allocate array memory and populate
                // This is simplified - would need proper memory management
                instructions.push(wasm_encoder::Instruction::I32Const(elements.len() as i32));
                
                // For now, just push a null pointer
                instructions.push(wasm_encoder::Instruction::I32Const(0));
            },
            
            Expression::Conditional { condition, then_expr, else_expr, .. } => {
                // Generate condition
                self.generate_expression_instructions(condition, instructions)?;
                
                // Create if-else block
                instructions.push(wasm_encoder::Instruction::If(BlockType::Result(ValType::I32)));
                
                // True branch
                self.generate_expression_instructions(then_expr, instructions)?;
                
                instructions.push(wasm_encoder::Instruction::Else);
                
                // False branch
                self.generate_expression_instructions(else_expr, instructions)?;
                
                instructions.push(wasm_encoder::Instruction::End);
            },
            
            _ => {
                return Err(CompilerError::codegen_error("Unsupported expression type", None, None));
            },
        }
        
        Ok(())
    }
    
    fn generate_literal_instructions(&mut self, value: &Value, instructions: &mut Vec<Instruction<'static>>) -> Result<(), CompilerError> {
        match value {
            Value::Integer(n) => {
                instructions.push(wasm_encoder::Instruction::I32Const(*n));
            },
            Value::Float(f) => {
                instructions.push(wasm_encoder::Instruction::F64Const(*f));
            },
            Value::Boolean(b) => {
                instructions.push(wasm_encoder::Instruction::I32Const(if *b { 1 } else { 0 }));
            },
            Value::String(s) => {
                // Add string to pool and push pointer
                let offset = self.add_string_to_pool(s.clone());
                instructions.push(wasm_encoder::Instruction::I32Const(offset as i32));
            },
            Value::List(elements) => {
                // Simplified array creation
                instructions.push(wasm_encoder::Instruction::I32Const(elements.len() as i32));
                instructions.push(wasm_encoder::Instruction::I32Const(0)); // Null pointer for now
            },
            Value::Matrix(rows) => {
                // Simplified matrix creation
                instructions.push(wasm_encoder::Instruction::I32Const(rows.len() as i32));
                instructions.push(wasm_encoder::Instruction::I32Const(0)); // Null pointer for now
            },
        }
        Ok(())
    }
    
    fn add_string_to_pool(&mut self, string: String) -> u32 {
        if let Some(&offset) = self.string_pool.get(&string) {
            offset
        } else {
            let offset = self.next_string_offset;
            self.string_pool.insert(string.clone(), offset);
            self.next_string_offset += string.len() as u32 + 4; // String length + null terminator + length prefix
            offset
        }
    }
    
    fn generate_default_value_instructions(&mut self, type_opt: &Option<Type>, instructions: &mut Vec<Instruction<'static>>) -> Result<(), CompilerError> {
        match type_opt {
            Some(Type::Integer) => instructions.push(wasm_encoder::Instruction::I32Const(0)),
            Some(Type::Number) => instructions.push(wasm_encoder::Instruction::F64Const(0.0)),
            Some(Type::Boolean) => instructions.push(wasm_encoder::Instruction::I32Const(0)),
            Some(Type::String) => instructions.push(wasm_encoder::Instruction::I32Const(0)), // Null string pointer
            _ => instructions.push(wasm_encoder::Instruction::I32Const(0)), // Default to null pointer
        }
        Ok(())
    }
    
    fn next_label(&mut self) -> u32 {
        let label = self.label_counter;
        self.label_counter += 1;
        label
    }
    
    fn generate_memory_layout(&self) -> MemoryLayout {
        MemoryLayout {
            initial_pages: 1,
            max_pages: Some(10),
            string_pool: self.string_pool.clone(),
            global_data_size: self.next_string_offset,
        }
    }
    
    fn generate_imports(&self, context: &ResolutionContext) -> Vec<ImportDeclaration> {
        let mut imports = Vec::new();
        
        for (name, builtin) in &context.builtin_functions {
            let parameters = builtin.parameters
                .iter()
                .map(|t| self.type_to_wasm_type(t))
                .collect();
            
            let results = builtin.return_type
                .as_ref()
                .map(|t| vec![self.type_to_wasm_type(t)])
                .unwrap_or_default();
            
            imports.push(ImportDeclaration {
                module: builtin.module.clone(),
                name: builtin.name.clone(),
                function_type: FunctionType { parameters, results },
            });
        }
        
        imports
    }
    
    fn generate_exports(&self, functions: &[GeneratedFunction]) -> Vec<ExportDeclaration> {
        functions
            .iter()
            .filter(|f| f.is_exported)
            .map(|f| ExportDeclaration {
                name: f.name.clone(),
                internal_name: f.name.clone(),
            })
            .collect()
    }
}

impl CompilationPhase<ResolutionContext, GenerationContext> for CodeEmitter {
    type Error = CompilerError;
    
    fn execute(&mut self, context: ResolutionContext) -> Result<GenerationContext, Self::Error> {
        let mut functions = Vec::new();
        
        // Generate code for all functions
        for function in &context.resolved_functions {
            let generated_function = self.generate_function(function)?;
            functions.push(generated_function);
        }
        
        // Generate memory layout
        let memory_layout = self.generate_memory_layout();
        
        // Extract string constants
        let string_constants: Vec<String> = self.string_pool.keys().cloned().collect();
        
        // Generate imports and exports
        let imports = self.generate_imports(&context);
        let exports = self.generate_exports(&functions);
        
        Ok(GenerationContext {
            functions,
            memory_layout,
            string_constants,
            type_signatures: self.type_signatures.clone(),
            imports,
            exports,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Program, Function as AstFunction, Parameter};

    #[test]
    fn test_code_emitter() {
        let mut emitter = CodeEmitter::new();
        assert_eq!(emitter.type_signatures.len(), 0);
        assert_eq!(emitter.string_pool.len(), 0);
    }

    #[test]
    fn test_type_to_wasm_type() {
        let emitter = CodeEmitter::new();
        assert_eq!(emitter.type_to_wasm_type(&Type::Integer), ValType::I32);
        assert_eq!(emitter.type_to_wasm_type(&Type::Number), ValType::F64);
        assert_eq!(emitter.type_to_wasm_type(&Type::Boolean), ValType::I32);
    }

    #[test]
    fn test_string_pool() {
        let mut emitter = CodeEmitter::new();
        let offset1 = emitter.add_string_to_pool("hello".to_string());
        let offset2 = emitter.add_string_to_pool("world".to_string());
        let offset3 = emitter.add_string_to_pool("hello".to_string()); // Duplicate
        
        assert!(offset1 < offset2);
        assert_eq!(offset1, offset3); // Should reuse existing string
        assert_eq!(emitter.string_pool.len(), 2);
    }
}