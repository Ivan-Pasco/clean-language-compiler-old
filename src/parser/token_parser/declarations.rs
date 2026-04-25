//! Declaration parsing methods for the token-driven parser.
//!
//! This module handles all declaration-level constructs for `TokenParser`:
//! - `parse_function` / `parse_start_function` / `parse_functions_block`
//! - `parse_function_in_block` — function inside `functions:` block
//! - `parse_class` / `parse_class_body` / `parse_field` / `parse_constructor`
//! - `parse_import` / `parse_import_item` / `parse_private`
//! - `parse_parameter_list` / `parse_parameter` / `parse_type`
//! - `parse_block` — the indentation-aware statement block
//! - `expect_identifier` / `expect_name` — token utilities

use super::TokenParser;
use crate::ast::{
    Class, Constructor, Field, Function, FunctionModifier, FunctionSyntax, ImportItem, Parameter,
    Statement, Type, Visibility,
};
use crate::error::CompilerError;
use crate::lexer::specification_token::TokenKind;
use tracing::warn;

impl TokenParser {
    pub(super) fn parse_function(&mut self) -> Result<Function, CompilerError> {
        self.expect(&TokenKind::Function)?;
        self.skip_whitespace();

        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();
        let location = name_token.location.clone();

        self.skip_whitespace();

        // Parameters
        let parameters = if self.eat(&TokenKind::LeftParen) {
            self.parse_parameter_list()?
        } else {
            Vec::new()
        };

        self.skip_whitespace();

        // Return type (optional, defaults to Void)
        let return_type = if self.eat(&TokenKind::Returns) {
            self.skip_whitespace();
            self.parse_type()?
        } else {
            Type::Void
        };

        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        // Body
        let body = self.parse_block()?;

        Ok(Function {
            name,
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters,
            return_type,
            body,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(location),
        })
    }

    /// Parse start: block (entry point)
    /// Example: start:
    ///             print("Hello")
    pub(super) fn parse_start_function(&mut self) -> Result<Function, CompilerError> {
        let start_token = self.expect(&TokenKind::Start)?;
        let location = start_token.location.clone();

        self.skip_whitespace();

        // Expect colon for block syntax
        self.expect(&TokenKind::Colon)?;

        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        // Body
        let body = self.parse_block()?;

        Ok(Function {
            name: "start".to_string(),
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters: Vec::new(), // start: has no parameters
            return_type: Type::Void,
            body,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
            location: Some(location),
        })
    }

    /// Parse a functions: block containing multiple function definitions
    pub(super) fn parse_functions_block(&mut self) -> Result<Vec<Function>, CompilerError> {
        // Consume "functions" keyword
        self.expect(&TokenKind::Functions)?;
        self.skip_whitespace();

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        let mut functions = Vec::new();

        // Determine the functions block's indentation level
        // Functions should be indented one level from the "functions:" line (which is at level 0)
        let functions_block_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1 // Default to level 1
            }
        } else {
            1 // Default to level 1
        };

        // Parse functions until we hit a dedent or EOF
        // Functions in a functions: block are indented relative to the "functions:" line
        let mut break_outer = false;
        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Consume Dedent tokens within the functions block
            // When we see a Dedent that exits the functions block (goes below our level),
            // DO NOT consume it - let the parent handle it
            while let TokenKind::Dedent(dedent_level) = self.current_kind() {
                let level = *dedent_level;
                if level < functions_block_level {
                    // This Dedent exits the functions block - DON'T consume it
                    // Set flag to break from outer loop
                    break_outer = true;
                    break;
                }
                self.bump(); // Only consume dedents at or above our level
                self.skip_whitespace();
            }

            if break_outer || self.is_at_end() {
                break;
            }

            // CRITICAL: Check if we're at a top-level construct (end of functions block)
            // This must happen BEFORE trying to parse as a function
            // Note: Start is not included here because it can be used as a method name
            if matches!(
                self.current_kind(),
                TokenKind::Class | TokenKind::Functions | TokenKind::Tests | TokenKind::Import
            ) {
                break;
            }

            // Check if Start is followed by Colon (top-level start: block, not a method name)
            if matches!(self.current_kind(), TokenKind::Start) {
                let saved = self.cursor;
                self.bump();
                self.skip_whitespace();
                let is_block = self.check(&TokenKind::Colon);
                self.cursor = saved;
                if is_block {
                    break;
                }
            }

            // Also check if we see a Dedent that would exit the block
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < functions_block_level {
                    // We're at a Dedent that exits the functions block
                    break;
                }
            }

            // Check if we're still in the functions block (indented content)
            // If we hit a non-indented line or EOF, we're done
            match self.current_kind() {
                TokenKind::Indent(_) => {
                    // Still in the block, parse the next function
                    self.skip_indentation();

                    // Check if this line starts a function signature
                    // Functions can optionally start with a return type or keyword that can be used as a name
                    match self.current_kind() {
                        TokenKind::Identifier(_)
                        | TokenKind::Test
                        | TokenKind::Unit
                        | TokenKind::Error
                        | TokenKind::Input
                        | TokenKind::Step
                        | TokenKind::Description
                        | TokenKind::Start => match self.parse_function_in_block() {
                            Ok(func) => functions.push(func),
                            Err(e) => return Err(e),
                        },
                        _ => {
                            // Not a function, might be end of block
                            break;
                        }
                    }
                }
                TokenKind::Identifier(_)
                | TokenKind::Test
                | TokenKind::Unit
                | TokenKind::Error
                | TokenKind::Input
                | TokenKind::Step
                | TokenKind::Description
                | TokenKind::Start => {
                    // Direct identifier or keyword without Indent token - still in functions block
                    // This happens when functions are at the same indentation level
                    match self.parse_function_in_block() {
                        Ok(func) => functions.push(func),
                        Err(e) => return Err(e),
                    }
                }
                _ => {
                    // No indentation = end of functions block
                    break;
                }
            }
        }

        // DON'T consume trailing Dedents - let parse_program handle them
        Ok(functions)
    }

    /// Parse a single function within a functions: block
    /// Functions in a block have the format: [return_type] name(params)
    pub(super) fn parse_function_in_block(&mut self) -> Result<Function, CompilerError> {
        let start_location = self.current().location.clone();

        // Save position in case we need to backtrack
        let saved_cursor = self.cursor;

        // Check for `async` modifier keyword (tokenized as Identifier("async"))
        // `async` is a function modifier indicating asynchronous execution.
        // The async scheduling is handled at the host/runtime level; the compiler
        // treats async functions identically to regular functions at the WASM level.
        let modifier = if matches!(self.current_kind(), TokenKind::Identifier(name) if name == "async")
        {
            self.bump(); // consume 'async'
            self.skip_whitespace();
            FunctionModifier::Background
        } else {
            FunctionModifier::None
        };

        // Try to parse as type (which handles precision modifiers like number:64)
        let (return_type, func_name) = match self.parse_type() {
            Ok(typ) => {
                self.skip_whitespace();
                // Check if this is actually a parameterless void function
                // (e.g., "first()" where "first" was parsed as Type::Object)
                if matches!(typ, Type::Object(_)) && self.check(&TokenKind::LeftParen) {
                    // This is a function name, not a type
                    // Extract the name from Type::Object
                    let name = if let Type::Object(n) = typ {
                        n
                    } else {
                        unreachable!()
                    };
                    (Type::Void, name)
                } else {
                    // Successfully parsed a type, expect function name next
                    let name_token = self.expect_name()?;
                    (typ, name_token.text.clone())
                }
            }
            Err(_) => {
                // Failed to parse as type, backtrack and try as function name
                self.cursor = saved_cursor;
                let first_token = self.expect_name()?;
                let first_name = first_token.text.clone();
                self.skip_whitespace();

                // Check if this is a parameterless function (name followed by '(')
                if self.check(&TokenKind::LeftParen) {
                    // No return type, first token is function name
                    (Type::Void, first_name)
                } else {
                    // This shouldn't happen since parse_type should have worked
                    // but handle it anyway - treat first token as return type
                    let return_type = match first_name.as_str() {
                        "void" => Type::Void,
                        "integer" => Type::Integer,
                        "number" => Type::Number,
                        "string" => Type::String,
                        "boolean" => Type::Boolean,
                        _ => Type::Object(first_name.clone()),
                    };
                    let name_token = self.expect_name()?;
                    (return_type, name_token.text.clone())
                }
            }
        };

        self.skip_whitespace();

        // Parse parameters: (param1, param2, ...)
        self.expect(&TokenKind::LeftParen)?;
        self.skip_whitespace();

        let mut parameters = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            loop {
                // Parse parameter: type name [= defaultValue]
                // Use parse_type() to handle precision modifiers like integer:8, number:32
                let param_type = self.parse_type()?;

                self.skip_whitespace();
                let name_token = self.expect_name()?;
                let param_name = name_token.text.clone();

                self.skip_whitespace();

                // Check for default value (e.g., name = defaultValue)
                let default_value = if self.eat(&TokenKind::Assign) {
                    self.skip_whitespace();
                    Some(self.parse_expression()?)
                } else {
                    None
                };

                parameters.push(Parameter {
                    name: param_name,
                    type_: param_type,
                    default_value,
                });

                self.skip_whitespace();

                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                self.skip_whitespace();
            }
        }

        self.expect(&TokenKind::RightParen)?;
        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        // Parse function body
        let body = self.parse_block()?;

        Ok(Function {
            name: func_name,
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters,
            return_type,
            body,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier,
            location: Some(start_location),
        })
    }

    pub(super) fn parse_class(&mut self) -> Result<Class, CompilerError> {
        self.expect(&TokenKind::Class)?;
        self.skip_whitespace();

        let name_token = self.expect_name()?;
        let name = name_token.text.clone();
        let location = name_token.location.clone();

        self.skip_whitespace();

        // Base class (optional, using "is" keyword)
        let base_class = if self.eat(&TokenKind::Is) {
            self.skip_whitespace();
            let parent_token = self.expect_identifier()?;
            Some(parent_token.text.clone())
        } else {
            None
        };

        self.skip_whitespace();
        // DON'T skip indentation here - let parse_class_body handle it

        // Class body
        let (fields, methods, constructor) = self.parse_class_body()?;

        Ok(Class {
            name,
            type_parameters: Vec::new(),
            description: None,
            base_class,
            base_class_type_args: Vec::new(),
            fields,
            methods,
            constructor,
            location: Some(location),
        })
    }

    pub(super) fn parse_class_body(
        &mut self,
    ) -> Result<(Vec<Field>, Vec<Function>, Option<Constructor>), CompilerError> {
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut constructor = None;

        while !self.is_at_end()
            && !matches!(
                self.current_kind(),
                TokenKind::Class | TokenKind::Function | TokenKind::Start
            )
        {
            self.skip_whitespace();
            self.skip_indentation();

            if self.is_at_end() {
                break;
            }

            // Check what comes next in the class body
            match self.current_kind() {
                TokenKind::Constructor => {
                    // Parse constructor
                    constructor = Some(self.parse_constructor()?);
                }
                TokenKind::Functions => {
                    // Parse functions: block
                    self.bump(); // consume 'functions'
                    self.skip_whitespace();
                    self.expect(&TokenKind::Colon)?;
                    self.skip_whitespace();

                    // Determine the functions block indentation level
                    let functions_indent_level =
                        if matches!(self.current_kind(), TokenKind::Indent(_)) {
                            if let TokenKind::Indent(level) = self.current_kind() {
                                *level
                            } else {
                                1
                            }
                        } else {
                            1
                        };

                    // Parse methods in the functions block
                    while !self.is_at_end() {
                        self.skip_whitespace();

                        if self.is_at_end() {
                            break;
                        }

                        // Check for Dedent that exits the functions block
                        if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                            if *dedent_level < functions_indent_level {
                                // This Dedent exits the functions block - DON'T consume it
                                break;
                            }
                            // Dedent at our level or higher - consume it and continue
                            self.bump();
                            self.skip_whitespace();
                        }

                        // Skip Indent tokens at our level
                        self.skip_indentation();

                        if self.is_at_end() {
                            break;
                        }

                        // Check for end of functions block (top-level declarations)
                        if matches!(
                            self.current_kind(),
                            TokenKind::Class
                                | TokenKind::Start
                                | TokenKind::Function
                                | TokenKind::Tests
                        ) {
                            break;
                        }

                        // Parse method (return_type name(params))
                        if matches!(self.current_kind(), TokenKind::Identifier(_)) {
                            match self.parse_function_in_block() {
                                Ok(func) => methods.push(func),
                                Err(e) => {
                                    // Log the error but don't break - try to continue
                                    // This allows recovery from individual method parse errors
                                    warn!(error = %e, "Failed to parse method");
                                    break;
                                }
                            }
                        } else {
                            // No more methods to parse
                            break;
                        }
                    }
                }
                TokenKind::Identifier(_) => {
                    // Could be a field (type name) - parse it
                    fields.push(self.parse_field()?);
                }
                TokenKind::Dedent(level) => {
                    // Dedent within the class body - consume it and continue
                    // Only exit if we dedent to level 0 or below (exiting the class)
                    if *level == 0 {
                        break; // Exit class body
                    }
                    // Otherwise, consume the Dedent and continue parsing the class body
                    self.bump();
                }
                _ => {
                    // Unknown token, exit class body
                    break;
                }
            }
        }

        Ok((fields, methods, constructor))
    }

    pub(super) fn parse_field(&mut self) -> Result<Field, CompilerError> {
        // Parse type first (e.g., "string", "integer", "list<integer>")
        let type_ = self.parse_type()?;

        self.skip_whitespace();

        // Parse field name
        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();

        self.skip_whitespace();

        // Optional default value
        let default_value = if self.eat(&TokenKind::Assign) {
            self.skip_whitespace();
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(Field {
            name,
            type_,
            visibility: Visibility::Public,
            is_static: false,
            default_value,
        })
    }

    pub(super) fn parse_constructor(&mut self) -> Result<Constructor, CompilerError> {
        let constructor_token = self.expect(&TokenKind::Constructor)?;
        let location = constructor_token.location.clone();

        self.skip_whitespace();

        // Parse parameter list (type name syntax, like functions: block)
        self.expect(&TokenKind::LeftParen)?;
        self.skip_whitespace();

        let mut parameters = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            loop {
                // Parse parameter: type name [= defaultValue]
                // Use parse_type() to handle precision modifiers like integer:8, number:32
                let param_type = self.parse_type()?;

                self.skip_whitespace();
                let name_token = self.expect_name()?;
                let param_name = name_token.text.clone();

                self.skip_whitespace();

                // Check for default value (e.g., name = defaultValue)
                let default_value = if self.eat(&TokenKind::Assign) {
                    self.skip_whitespace();
                    Some(self.parse_expression()?)
                } else {
                    None
                };

                parameters.push(Parameter {
                    name: param_name,
                    type_: param_type,
                    default_value,
                });

                self.skip_whitespace();

                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                self.skip_whitespace();
            }
        }

        self.expect(&TokenKind::RightParen)?;
        self.skip_whitespace();
        // DON'T skip indentation - let parse_block() handle it

        // Parse constructor body
        let body = self.parse_block()?;

        Ok(Constructor {
            parameters,
            body,
            location: Some(location),
        })
    }

    pub(super) fn parse_import(&mut self) -> Result<Vec<ImportItem>, CompilerError> {
        self.expect(&TokenKind::Import)?;
        self.skip_whitespace();

        let mut import_items = Vec::new();

        // Check for file path import: import "path/to/file.cln"
        if let TokenKind::StringLiteral(path) = self.current_kind() {
            let file_path = path.clone();
            self.bump(); // consume string literal
            import_items.push(ImportItem {
                name: file_path,
                alias: None,
                is_file_import: true,
            });
            return Ok(import_items);
        }

        // Only the block form is supported: import:\n\tmodule\n\t...
        // The inline form `import module` is not part of the specification.
        if !self.eat(&TokenKind::Colon) {
            return Err(CompilerError::parse_error(
                "Expected ':' after 'import'. Use the block form:\n  import:\n      module_name",
                None,
                Some("Inline import syntax is not supported. Use 'import:' followed by an indented block of module names.".to_string()),
            ));
        }

        self.skip_whitespace();

        // Parse indented import items
        while !self.is_at_end() {
            self.skip_whitespace();

            // Check for indentation or end of block
            if matches!(self.current_kind(), TokenKind::Indent(_)) {
                self.skip_indentation();

                // Parse import item
                import_items.push(self.parse_import_item()?);
                self.skip_whitespace();
            } else if matches!(self.current_kind(), TokenKind::Dedent(_)) {
                // End of import block
                break;
            } else if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Private
            ) {
                // Hit next top-level block
                break;
            } else {
                // Not indented and not a dedent = end of block
                break;
            }
        }

        Ok(import_items)
    }

    /// Parse a single import item with support for "Module.symbol" syntax and file paths
    /// Examples:
    ///   Math → whole module
    ///   math.sqrt → specific symbol
    ///   Utils as U → module alias
    ///   Json.decode as jd → symbol alias
    ///   "path/to/file.cln" → file path import
    pub(super) fn parse_import_item(&mut self) -> Result<ImportItem, CompilerError> {
        // Check for file path import (string literal)
        if let TokenKind::StringLiteral(path) = self.current_kind() {
            let file_path = path.clone();
            self.bump(); // consume string literal
            return Ok(ImportItem {
                name: file_path,
                alias: None,
                is_file_import: true,
            });
        }

        let mut name = String::new();

        // Parse first identifier
        let first_token = self.expect_identifier()?;
        name.push_str(&first_token.text);

        self.skip_whitespace();

        // Check for dot notation (Module.symbol)
        if self.eat(&TokenKind::Dot) {
            name.push('.');
            self.skip_whitespace();

            let symbol_token = self.expect_identifier()?;
            name.push_str(&symbol_token.text);

            self.skip_whitespace();
        }

        // Validate: reject plugin names in import: blocks
        // Plugin names look like "frame.ui", "frame.data" (word.word format)
        if Self::is_plugin_name(&name) {
            return Err(CompilerError::parse_error(
                format!(
                    "Use 'plugins:' block for framework plugins like '{}'. Change 'import:' to 'plugins:'",
                    name
                ),
                Some(first_token.location.clone()),
                Some(format!(
                    "Framework plugins should be declared in a plugins: block:\n\nplugins:\n    {}\n\nThe 'import:' block is for module imports, not plugins.",
                    name
                )),
            ));
        }

        // Check for alias (as ...)
        let alias = if let TokenKind::Identifier(id) = self.current_kind() {
            if id == "as" {
                self.bump(); // consume 'as'
                self.skip_whitespace();
                let alias_token = self.expect_identifier()?;
                Some(alias_token.text.clone())
            } else {
                None
            }
        } else {
            None
        };

        Ok(ImportItem {
            name,
            alias,
            is_file_import: false,
        })
    }

    /// Parse a private: block listing function names to mark as private
    /// Example: private:\n\thelperFunction\n\tinternalProcessor
    pub(super) fn parse_private(&mut self) -> Result<Statement, CompilerError> {
        let private_token = self.expect(&TokenKind::Private)?;
        let location = private_token.location.clone();

        self.skip_whitespace();

        // Expect colon after private keyword
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Skip newline after colon
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        let mut items = Vec::new();

        // Determine the private block's indentation level
        let block_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                *level
            } else {
                1
            }
        } else {
            1
        };

        // Parse indented function names
        while !self.is_at_end() {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // Check for Dedent that exits the private block
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < block_indent_level {
                    // This Dedent exits the private block - DON'T consume it
                    break;
                }
                // Dedent at our level or higher - consume it and continue
                self.bump();
                self.skip_whitespace();
            }

            // Check for indentation or end of block
            if matches!(self.current_kind(), TokenKind::Indent(_)) {
                self.skip_indentation();

                // Parse function name (identifier)
                if let TokenKind::Identifier(name) = self.current_kind() {
                    let name = name.clone();
                    self.bump();

                    // Create an Expression statement with the function name as a Variable
                    items.push(Statement::Expression {
                        expr: crate::ast::Expression::Variable(name),
                        location: Some(self.current().location.clone()),
                    });
                }
                self.skip_whitespace();
            } else if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Tests
                    | TokenKind::Import
            ) {
                // Hit next top-level block
                break;
            } else {
                // Not indented and not a dedent = end of block
                break;
            }
        }

        Ok(Statement::PrivateBlock {
            items,
            location: Some(location),
        })
    }

    pub(super) fn parse_parameter_list(&mut self) -> Result<Vec<Parameter>, CompilerError> {
        let mut parameters = Vec::new();

        self.skip_whitespace();

        while !self.check(&TokenKind::RightParen) && !self.is_at_end() {
            parameters.push(self.parse_parameter()?);
            self.skip_whitespace();

            if !self.eat(&TokenKind::Comma) {
                break;
            }
            self.skip_whitespace();
        }

        self.expect(&TokenKind::RightParen)?;

        Ok(parameters)
    }

    pub(super) fn parse_parameter(&mut self) -> Result<Parameter, CompilerError> {
        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();

        self.skip_whitespace();

        // Type annotation (required for parameters)
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();
        let type_ = self.parse_type()?;

        self.skip_whitespace();

        // Optional default value (e.g., name: type = defaultValue)
        let default_value = if self.eat(&TokenKind::Assign) {
            self.skip_whitespace();
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(Parameter {
            name,
            type_,
            default_value,
        })
    }

    pub(super) fn parse_type(&mut self) -> Result<Type, CompilerError> {
        let type_token = self.expect_identifier()?;

        // Check for generic type parameters (e.g., list<integer>, matrix<number>)
        // and precision modifiers (e.g., integer:8, number:32)
        let base_type = match type_token.text.as_str() {
            "integer" => Type::Integer,
            "number" => Type::Number,
            "string" => Type::String,
            "boolean" => Type::Boolean,
            "void" => Type::Void,
            "any" => Type::Any,
            "list" => {
                // Expect list<Type> [ list_behavior ]
                // spec/grammar.ebnf: list_type = "list" , "<" , type , ">" , [ list_behavior ] ;
                // list_behavior = "." , ( "line" | "pile" | "unique" | combinations )
                self.skip_whitespace();
                if matches!(self.current_kind(), TokenKind::Less) {
                    self.bump(); // consume '<'
                    self.skip_whitespace();
                    let inner_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Greater)?; // expect '>'
                                                       // Consume optional list_behavior suffix: .line .pile .unique (and combos)
                                                       // The behavior is parsed and discarded here — full codegen wiring is in TASKS.md.
                    let behavior_keywords = ["line", "pile", "unique"];
                    while matches!(self.current_kind(), TokenKind::Dot) {
                        // Peek at the identifier after the dot
                        let saved = self.cursor;
                        self.bump(); // consume '.'
                        if let TokenKind::Identifier(kw) = self.current_kind() {
                            if behavior_keywords.contains(&kw.as_str()) {
                                self.bump(); // consume the behavior keyword
                            } else {
                                // Not a behavior keyword — restore cursor (method call follows)
                                self.cursor = saved;
                                break;
                            }
                        } else {
                            self.cursor = saved;
                            break;
                        }
                    }
                    Type::List(Box::new(inner_type))
                } else {
                    // list without generic parameter - treat as Object
                    Type::Object("list".to_string())
                }
            }
            "matrix" => {
                // Expect matrix<Type>
                self.skip_whitespace();
                if matches!(self.current_kind(), TokenKind::Less) {
                    self.bump(); // consume '<'
                    self.skip_whitespace();
                    let inner_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Greater)?; // expect '>'
                    Type::Matrix(Box::new(inner_type))
                } else {
                    // matrix without generic parameter - treat as Object
                    Type::Object("matrix".to_string())
                }
            }
            "pairs" => {
                // Expect pairs<Type, Type>
                self.skip_whitespace();
                if matches!(self.current_kind(), TokenKind::Less) {
                    self.bump(); // consume '<'
                    self.skip_whitespace();
                    let first_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Comma)?; // expect ','
                    self.skip_whitespace();
                    let second_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Greater)?; // expect '>'
                    Type::Pairs(Box::new(first_type), Box::new(second_type))
                } else {
                    // pairs without generic parameters - treat as Object
                    Type::Object("pairs".to_string())
                }
            }
            other => Type::Object(other.to_string()),
        };

        // Check for precision modifiers (e.g., integer:8, number:32u)
        if self.check(&TokenKind::Colon) {
            self.bump(); // consume ':'

            // Expect an integer literal for the bit size
            if let TokenKind::IntegerLiteral(size) = self.current_kind() {
                let bits = *size as u8;
                self.bump(); // consume the size

                // Check for 'u' suffix for unsigned
                let unsigned = if let TokenKind::Identifier(suffix) = self.current_kind() {
                    if suffix == "u" {
                        self.bump(); // consume 'u'
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };

                // Apply precision modifier based on base type
                match base_type {
                    Type::Integer => Ok(Type::IntegerSized { bits, unsigned }),
                    Type::Number => Ok(Type::NumberSized { bits }),
                    _ => Err(CompilerError::parse_error(
                        format!("Precision modifiers are only supported for integer and number types, not {:?}", base_type),
                        Some(self.current().location.clone()),
                        Some("Use :bits syntax only with integer or number types".to_string()),
                    ))
                }
            } else {
                Err(CompilerError::parse_error(
                    "Expected integer literal for precision modifier".to_string(),
                    Some(self.current().location.clone()),
                    Some("Precision modifiers should be in format 'type:bits' (e.g., 'integer:8', 'number:32')".to_string()),
                ))
            }
        } else {
            Ok(base_type)
        }
    }

    /// Parse a block of statements at the current indentation level
    /// Returns when it encounters a Dedent that exits this block level
    pub(super) fn parse_block(&mut self) -> Result<Vec<Statement>, CompilerError> {
        let mut statements = Vec::new();

        // Consume the leading Indent token to determine our indentation level
        // If there's no Indent, we're at level 0 (top level of a function)
        let block_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                let level_value = *level;
                self.bump(); // Consume the Indent token
                level_value
            } else {
                0
            }
        } else {
            0
        };

        while !self.is_at_end() {
            self.skip_whitespace();

            // Skip any Indent tokens at our level (from line beginnings)
            // These indicate continued statements at the same indentation level
            while matches!(self.current_kind(), TokenKind::Indent(level) if *level == block_indent_level)
            {
                self.bump();
                self.skip_whitespace();
            }

            // Check for block terminators
            if self.is_at_end() {
                break;
            }

            // Check if we've encountered a Dedent token
            // A Dedent(N) token means "we're now at indentation level N"
            // We should exit this block if the dedent takes us BELOW our level
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < block_indent_level {
                    // We've exited this block's scope - DON'T consume the Dedent
                    // Let the parent handle it
                    break;
                }
                // Dedent at our level or higher - consume it and continue
                // (This shouldn't happen in practice, but handle it gracefully)
                self.bump();
                continue;
            }

            // Top-level declarations end the current block
            // Note: TokenKind::Test is NOT included here because "test" can be used as a
            // function name inside blocks (e.g., calling a function named "test()")
            if matches!(
                self.current_kind(),
                TokenKind::Function | TokenKind::Class | TokenKind::Functions
            ) {
                break;
            }

            match self.parse_statement() {
                Ok(stmt) => statements.push(stmt),
                Err(e) => {
                    self.errors.push(e);
                    self.bump(); // Skip error token
                }
            }
        }

        Ok(statements)
    }

    pub(super) fn expect_identifier(
        &mut self,
    ) -> Result<crate::lexer::specification_token::Token, CompilerError> {
        match self.current_kind() {
            TokenKind::Identifier(_) => Ok(self.bump()),
            _ => {
                let token = self.current();
                Err(CompilerError::parse_error(
                    format!("Expected identifier, found {:?}", token.kind),
                    Some(token.location.clone()),
                    None,
                ))
            }
        }
    }

    /// Get identifier or keyword text (for cases where keywords can be used as names)
    pub(super) fn expect_name(
        &mut self,
    ) -> Result<crate::lexer::specification_token::Token, CompilerError> {
        match self.current_kind() {
            TokenKind::Identifier(_) => Ok(self.bump()),
            // Allow keywords to be used as names in certain contexts (for property/method names and variable names)
            TokenKind::Test
            | TokenKind::Unit
            | TokenKind::Error
            | TokenKind::Input
            | TokenKind::Step
            | TokenKind::Description
            | TokenKind::And
            | TokenKind::Or
            | TokenKind::Not
            | TokenKind::Start
            | TokenKind::Rules
            | TokenKind::Computed
            | TokenKind::State
            | TokenKind::Guard
            | TokenKind::Watch
            | TokenKind::Reset
            | TokenKind::Screen
            | TokenKind::Source
            | TokenKind::Build
            | TokenKind::Spec
            | TokenKind::Intent => Ok(self.bump()),
            _ => {
                let token = self.current();
                Err(CompilerError::parse_error(
                    format!(
                        "Expected name (identifier or keyword), found {:?}",
                        token.kind
                    ),
                    Some(token.location.clone()),
                    None,
                ))
            }
        }
    }
}
