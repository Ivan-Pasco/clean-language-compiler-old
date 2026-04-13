//! Function generation for Clean Language WebAssembly compiler.

use wasm_encoder::{Function, Instruction, ValType};
use crate::ast::{Function as AstFunction, Statement, Type};
use crate::error::CompilerError;
use crate::types::WasmType;
use super::{CodeGenerator, LocalVarInfo};

impl CodeGenerator {
    /// Register function signature without generating body (for mutual recursion support)
    pub fn register_function_signature(&mut self, function: &AstFunction) -> Result<(), CompilerError> {
        // Store function definition for default parameter handling
        self.function_definitions.insert(function.name.clone(), function.clone());

        // Register function in function map - local functions get indices after all imports
        let function_index = self.imported_functions.len() as u32 + self.function_names.len() as u32;
        // Function registered with correct index accounting for imports
        self.function_map.insert(function.name.clone(), function_index);
        self.function_names.push(function.name.clone());

        // Prepare function type (signature) and add to function section
        let type_index = self.prepare_function_type_and_declare(function)?;
        
        // Add function declaration to function section (type index only)
        self.function_section.function(type_index);

        Ok(())
    }

    /// Generate only the function body (assumes signature is already registered)
    pub fn generate_function_body_only(&mut self, function: &AstFunction) -> Result<(), CompilerError> {
        // Clear current function state
        self.current_function_params.clear();
        self.current_function_locals.clear();
        self.current_function_param_count = 0;

        // Prepare function parameters
        self.setup_function_parameters(function)?;

        // Generate function body
        let mut instructions = Vec::new();
        self.generate_function_body(function, &mut instructions)?;

        // Collect local variable declarations for WASM function
        let mut local_declarations = Vec::new();
        for local_info in &self.current_function_locals {
            local_declarations.push((1u32, local_info.type_));
        }

        // Create WASM function with proper local variable declarations
        let mut wasm_function = Function::new(local_declarations);
        
        // Add all instructions
        for instruction in instructions {
            wasm_function.instruction(&instruction);
        }

        // NOTE: Add 'end' instruction to properly terminate function
        wasm_function.instruction(&Instruction::End);

        // Add function to code section
        self.code_section.function(&wasm_function);

        // Get the function index that was registered earlier
        let function_index = self.function_map.get(&function.name).copied()
            .ok_or_else(|| CompilerError::codegen_error(
                format!("Function {} was not registered in signature phase", function.name),
                None,
                None
            ))?;

        // Export function if it's public
        if matches!(function.visibility, crate::ast::Visibility::Public) {
            self.export_section.export(&function.name, wasm_encoder::ExportKind::Func, function_index);
        }

        Ok(())
    }

    /// Generate WASM function from AST function
    pub fn generate_function(&mut self, function: &AstFunction) -> Result<(), CompilerError> {
        // Clear current function state
        self.current_function_params.clear();
        self.current_function_locals.clear();
        self.current_function_param_count = 0;

        // Store function definition for default parameter handling
        self.function_definitions.insert(function.name.clone(), function.clone());

        // **NOTE FOR RECURSIVE FUNCTIONS**: Register function in function map BEFORE generating body
        // This allows recursive calls to find the function during body generation
        let function_index = self.imported_functions.len() as u32 + self.function_names.len() as u32;
        // Function generated with correct index accounting for imports
        self.function_map.insert(function.name.clone(), function_index);
        self.function_names.push(function.name.clone());

        // Prepare function parameters
        self.setup_function_parameters(function)?;

        // Prepare function type (signature)
        self.prepare_function_type(function)?;

        // Generate function body
        let mut instructions = Vec::new();
        self.generate_function_body(function, &mut instructions)?;

        // Collect local variable declarations for WASM function
        let mut local_declarations = Vec::new();
        for local_info in &self.current_function_locals {
            local_declarations.push((1u32, local_info.type_));
        }

        // Create WASM function with proper local variable declarations
        let mut wasm_function = Function::new(local_declarations);
        
        // Add all instructions
        for instruction in instructions {
            wasm_function.instruction(&instruction);
        }

        // NOTE: Add 'end' instruction to properly terminate function
        wasm_function.instruction(&Instruction::End);

        // Add function to code section
        self.code_section.function(&wasm_function);

        // Export function if it's public
        if matches!(function.visibility, crate::ast::Visibility::Public) {
            self.export_section.export(&function.name, wasm_encoder::ExportKind::Func, function_index);
        }

        Ok(())
    }

    fn setup_function_parameters(&mut self, function: &AstFunction) -> Result<(), CompilerError> {
        self.current_function_param_count = function.parameters.len() as u32;
        
        for (i, param) in function.parameters.iter().enumerate() {
            let param_type = self.ast_type_to_wasm_type(&param.type_)?;

            let local_info = LocalVarInfo {
                index: i as u32,
                type_: param_type.into(),
            };

            self.current_function_params.push(local_info.clone());
            self.variable_map.insert(param.name.clone(), local_info);
            // Also add parameter to variable_types so expression generator can access type info
            self.variable_types.insert(param.name.clone(), param.type_.clone());
        }

        // If we're in a class context (constructor or method), add class fields to variable_map
        if let Some(class_name) = &self.current_class_context {
            if let Some(class) = self.class_table.get(class_name).cloned() {
                let mut field_index = self.current_function_param_count;
                
                for field in &class.fields {
                    let field_type = self.ast_type_to_wasm_type(&field.type_)?;
                    
                    let local_info = LocalVarInfo {
                        index: field_index,
                        type_: field_type.into(),
                    };
                    
                    // Add field to current function locals so WASM knows about it
                    self.current_function_locals.push(local_info.clone());
                    
                    // Add field to variable_map so assignments and expressions can find it
                    self.variable_map.insert(field.name.clone(), local_info);
                    
                    field_index += 1;
                }
            }
        }

        Ok(())
    }

    fn generate_function_body(
        &mut self,
        function: &AstFunction,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // Handle default parameters
        self.generate_default_parameter_logic(function, instructions)?;

        // Check if the last statement is an expression that should be implicitly returned
        let has_implicit_return = self.has_implicit_return_expression(&function.body, &function.return_type);

        // Generate function body statements
        if has_implicit_return && !function.body.is_empty() {
            // Generate all statements except the last one normally
            for statement in &function.body[..function.body.len() - 1] {
                self.generate_statement(statement, instructions)?;
            }

            // Generate the last statement as an implicit return
            if let Some(Statement::Expression { expr, .. }) = function.body.last() {
                // Generate the expression without dropping its result
                self.generate_expression(expr, instructions)?;
            }
        } else {
            // Generate all statements normally
            for statement in &function.body {
                self.generate_statement(statement, instructions)?;
            }

            // Add implicit return if function has no explicit return
            if !self.has_explicit_return(&function.body) {
                match &function.return_type {
                    Type::Void => {
                        // No return value needed
                    },
                    return_type => {
                        // Generate default return value based on type
                        self.generate_default_return_value(return_type, instructions)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn generate_default_parameter_logic(
        &mut self,
        function: &AstFunction,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        // For functions with default parameters, generate logic to handle missing arguments
        for (i, param) in function.parameters.iter().enumerate() {
            if let Some(ref default_value) = param.default_value {
                // Check if parameter was provided (assuming undefined parameters are 0 for now)
                // This is a simplified implementation - proper handling would require runtime support
                
                instructions.push(Instruction::LocalGet(i as u32));
                
                // Check if parameter is "undefined" (0 in our simplified model)
                instructions.push(Instruction::I32Const(0));
                instructions.push(Instruction::I32Eq);
                
                // If parameter is undefined, use default value
                instructions.push(Instruction::If(wasm_encoder::BlockType::Empty));
                
                // Generate default value
                self.generate_expression(default_value, instructions)?;
                instructions.push(Instruction::LocalSet(i as u32));
                
                instructions.push(Instruction::End);
            }
        }
        
        Ok(())
    }

    fn generate_default_return_value(
        &mut self,
        return_type: &Type,
        instructions: &mut Vec<Instruction>,
    ) -> Result<(), CompilerError> {
        match return_type {
            Type::Integer => instructions.push(Instruction::I32Const(0)),
            Type::Number => instructions.push(Instruction::F64Const(0.0)),
            Type::Boolean => instructions.push(Instruction::I32Const(0)),
            Type::String => {
                // Return empty string
                let empty_str_ptr = self.allocate_string("")?;
                instructions.push(Instruction::I32Const(empty_str_ptr as i32));
            },
            Type::List(_) => {
                // Return null pointer for empty list
                instructions.push(Instruction::I32Const(0));
            },
            Type::Matrix(_) => {
                // Return null pointer for empty matrix
                instructions.push(Instruction::I32Const(0));
            },
            Type::Class { .. } => {
                // Return null pointer for null object
                instructions.push(Instruction::I32Const(0));
            },
            Type::Void => {
                // No return value
            },
            Type::Generic(..) => {
                // Default to i32 for generic types
                instructions.push(Instruction::I32Const(0));
            },
            // New Type variants - sized integers
            Type::IntegerSized { bits, unsigned: _ } => {
                match bits {
                    8 | 16 | 32 => instructions.push(Instruction::I32Const(0)),
                    64 => instructions.push(Instruction::I64Const(0)),
                    _ => instructions.push(Instruction::I32Const(0)), // Default to i32
                }
            },
            // New Type variants - sized numbers
            Type::NumberSized { bits } => {
                match bits {
                    32 => instructions.push(Instruction::F32Const(0.0)),
                    64 => instructions.push(Instruction::F64Const(0.0)),
                    _ => instructions.push(Instruction::F64Const(0.0)), // Default to f64
                }
            },
            // New Type variants - complex types return null pointers
            Type::Pairs(_, _) => {
                // Return null pointer for pair
                instructions.push(Instruction::I32Const(0));
            },
            Type::TypeParameter(_) => {
                // Return null pointer for type parameters
                instructions.push(Instruction::I32Const(0));
            },
            Type::Object(_) => {
                // Return null pointer for object
                instructions.push(Instruction::I32Const(0));
            },
            Type::Function(_, _) => {
                // Return null pointer for function
                instructions.push(Instruction::I32Const(0));
            },
            Type::Future(_) => {
                // Return null pointer for future
                instructions.push(Instruction::I32Const(0));
            },
            Type::Any => {
                // Return null pointer for any type
                instructions.push(Instruction::I32Const(0));
            },
        }
        
        Ok(())
    }

    fn has_explicit_return(&self, statements: &[Statement]) -> bool {
        for stmt in statements {
            match stmt {
                Statement::Return { .. } => return true,
                Statement::If { then_branch, else_branch, .. } => {
                    if self.has_explicit_return(then_branch) {
                        if let Some(else_stmts) = else_branch {
                            if self.has_explicit_return(else_stmts) {
                                return true;
                            }
                        }
                    }
                },
                _ => continue,
            }
        }
        false
    }

    fn has_implicit_return_expression(&self, statements: &[Statement], return_type: &Type) -> bool {
        // Only consider implicit returns if the function has a non-void return type
        if matches!(return_type, Type::Void) {
            return false;
        }

        // Check if the last statement is an expression statement
        if let Some(last_stmt) = statements.last() {
            match last_stmt {
                Statement::Expression { .. } => {
                    // Only treat as implicit return if there's no explicit return anywhere
                    !self.has_explicit_return(statements)
                },
                _ => false
            }
        } else {
            false
        }
    }

    /// Prepare function type signature and return type index for WASM function declaration
    pub fn prepare_function_type_and_declare(&mut self, function: &AstFunction) -> Result<u32, CompilerError> {
        let mut param_types = Vec::new();
        let mut return_types = Vec::new();

        // Process parameters
        for param in &function.parameters {
            let wasm_type = self.ast_type_to_wasm_type(&param.type_)?;
            param_types.push(self.wasm_type_to_val_type(wasm_type));
        }

        // Process return type
        if !matches!(function.return_type, Type::Void) {
            let wasm_return_type = self.ast_type_to_wasm_type(&function.return_type)?;
            return_types.push(self.wasm_type_to_val_type(wasm_return_type));
        }

        // Add function type to type section and get type index
        let type_index = self.type_manager.add_function_type(param_types, return_types);
        
        Ok(type_index)
    }

    /// Prepare function type signature for WASM
    pub fn prepare_function_type(&mut self, function: &AstFunction) -> Result<(), CompilerError> {
        let mut param_types = Vec::new();
        let mut return_types = Vec::new();

        // Process parameters
        for param in &function.parameters {
            let wasm_type = self.ast_type_to_wasm_type(&param.type_)?;
            param_types.push(self.wasm_type_to_val_type(wasm_type));
        }

        // Process return type
        if !matches!(function.return_type, Type::Void) {
            let wasm_return_type = self.ast_type_to_wasm_type(&function.return_type)?;
            return_types.push(self.wasm_type_to_val_type(wasm_return_type));
        }

        // Add function type to type section
        self.add_function_type(param_types, return_types);
        
        Ok(())
    }

    fn wasm_type_to_val_type(&self, wasm_type: WasmType) -> ValType {
        match wasm_type {
            WasmType::I32 => ValType::I32,
            WasmType::I64 => ValType::I64,
            WasmType::F32 => ValType::F32,
            WasmType::F64 => ValType::F64,
            WasmType::V128 => ValType::V128,
            WasmType::Unit => ValType::I32, // Unit represented as i32
        }
    }

    /// Generate constructor function for a class from Constructor struct
    pub fn generate_constructor_from_struct(
        &mut self, 
        class_name: &str,
        constructor: &crate::ast::Constructor
    ) -> Result<(), CompilerError> {
        // Convert Constructor to Function for code generation
        let constructor_func = crate::ast::Function {
            name: format!("{}_new", class_name),
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters: constructor.parameters.clone(),
            return_type: crate::ast::Type::Class {
                name: class_name.to_string(),
                type_args: Vec::new(),
            },
            body: constructor.body.clone(),
            description: None,
            syntax: crate::ast::FunctionSyntax::Simple,
            visibility: crate::ast::Visibility::Public,
            modifier: crate::ast::FunctionModifier::None,
            location: constructor.location.clone(),
        };

        // Set class context
        self.current_class_context = Some(class_name.to_string());

        // Generate the constructor as a regular function
        self.generate_function(&constructor_func)?;

        // Clear class context
        self.current_class_context = None;

        Ok(())
    }

    /// Generate constructor function for a class
    pub fn generate_constructor(
        &mut self, 
        class_name: &str,
        constructor: &AstFunction
    ) -> Result<(), CompilerError> {
        // Set class context
        self.current_class_context = Some(class_name.to_string());

        // Generate the constructor as a regular function
        self.generate_function(constructor)?;

        // Clear class context
        self.current_class_context = None;

        Ok(())
    }

    /// Register method signature without generating body
    pub fn register_method_signature(
        &mut self,
        class_name: &str,
        method: &AstFunction
    ) -> Result<(), CompilerError> {
        // Set class context
        self.current_class_context = Some(class_name.to_string());

        // Add 'this' parameter to method
        let mut method_with_this = method.clone();
        
        // Insert 'this' parameter at the beginning
        let this_param = crate::ast::Parameter {
            name: "this".to_string(),
            type_: Type::Class {
                name: class_name.to_string(),
                type_args: Vec::new(),
            },
            default_value: None,
        };
        method_with_this.parameters.insert(0, this_param);

        // Register the method signature only
        self.register_function_signature(&method_with_this)?;

        // Clear class context
        self.current_class_context = None;

        Ok(())
    }

    /// Generate method body only (assumes signature is already registered)
    pub fn generate_method_body_only(
        &mut self,
        class_name: &str,
        method: &AstFunction
    ) -> Result<(), CompilerError> {
        // Set class context
        self.current_class_context = Some(class_name.to_string());

        // Add 'this' parameter to method
        let mut method_with_this = method.clone();
        
        // Insert 'this' parameter at the beginning
        let this_param = crate::ast::Parameter {
            name: "this".to_string(),
            type_: Type::Class {
                name: class_name.to_string(),
                type_args: Vec::new(),
            },
            default_value: None,
        };
        method_with_this.parameters.insert(0, this_param);

        // Generate the method body only
        self.generate_function_body_only(&method_with_this)?;

        // Clear class context
        self.current_class_context = None;

        Ok(())
    }

    /// Generate method function for a class
    pub fn generate_method(
        &mut self,
        class_name: &str,
        method: &AstFunction
    ) -> Result<(), CompilerError> {
        // Set class context
        self.current_class_context = Some(class_name.to_string());

        // Add 'this' parameter to method
        let mut method_with_this = method.clone();
        
        // Insert 'this' parameter at the beginning
        let this_param = crate::ast::Parameter {
            name: "this".to_string(),
            type_: Type::Class {
                name: class_name.to_string(),
                type_args: Vec::new(),
            },
            default_value: None,
        };
        method_with_this.parameters.insert(0, this_param);

        // Generate the method as a regular function
        self.generate_function(&method_with_this)?;

        // Clear class context
        self.current_class_context = None;

        Ok(())
    }

    /// Generate static method function for a class
    pub fn generate_static_method(
        &mut self,
        class_name: &str,
        method: &AstFunction
    ) -> Result<(), CompilerError> {
        // Static methods don't need class context or 'this' parameter
        // Just generate as a regular function with class name prefix
        let mut static_method = method.clone();
        static_method.name = format!("{}::{}", class_name, method.name);

        self.generate_function(&static_method)?;

        Ok(())
    }

    /// Create _start function that calls the Clean Language start() function if it exists
    pub fn generate_start_function(&mut self) -> Result<(), CompilerError> {
        // Look for Clean Language entry point "start" function
        if let Some(&start_index) = self.function_map.get("start") {
            tracing::trace!("DEBUG _start: Will call start() function at index {}", start_index);
            tracing::trace!("DEBUG _start: Total imports = {}, Total functions = {}",
                self.imported_functions.len(), self.function_names.len());
            // NOTE: Declare the _start function in the function section FIRST
            // The _start function has no parameters and no return value
            let type_index = self.type_manager.add_function_type(&[], None)?;
            self.function_section.function(type_index);

            let mut instructions = Vec::new();

            // Call start function
            instructions.push(Instruction::Call(start_index));

            // Drop return value if any
            instructions.push(Instruction::Drop);

            // Create start function
            let start_function = Function::new(vec![]);

            // Add instructions
            let mut wasm_function = start_function;
            for instruction in instructions {
                wasm_function.instruction(&instruction);
            }

            // NOTE: Add 'end' instruction to properly terminate start function
            wasm_function.instruction(&Instruction::End);

            // Add to code section
            self.code_section.function(&wasm_function);

            // Export as start function - use correct index after all imports and existing functions
            let start_func_index = self.imported_functions.len() as u32 + self.function_names.len() as u32;
            // Start function exported with correct index
            self.export_section.export("_start", wasm_encoder::ExportKind::Func, start_func_index);

            // Update function tracking to keep counts consistent
            self.function_names.push("_start".to_string());
        }

        Ok(())
    }
}