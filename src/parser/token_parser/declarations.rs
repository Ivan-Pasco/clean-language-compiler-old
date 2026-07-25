//! Declaration parsing methods for the token-driven parser.
//!
//! This module handles all declaration-level constructs for `TokenParser`:
//! - `parse_function` / `parse_start_function` / `parse_functions_block`
//! - `parse_function_in_block` — function inside `functions:` block
//! - `parse_class` / `parse_class_body` / `parse_field` / `parse_constructor`
//! - `parse_import` / `parse_import_item`
//! - `parse_parameter_list` / `parse_parameter` / `parse_type`
//! - `parse_block` — the indentation-aware statement block
//! - `expect_identifier` / `expect_name` — token utilities

use super::TokenParser;
use crate::ast::{
    Capability, CapabilityMethod, Class, Constructor, Expression, Field, Function,
    FunctionModifier, FunctionSyntax, ImportItem, ListBehavior, Parameter, Statement, Type,
    Visibility,
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
            // Private by default per the 2026-06-25 visibility flip;
            // a function becomes Public only when it appears inside a
            // `public:` sub-section (parse_public_functions_section).
            visibility: Visibility::Private,
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

                    // Check for inline public: sub-section
                    // Grammar (foundation/spec/grammar.ebnf §6.2a):
                    //   public_functions_section = INDENT+ "public" ":" NEWLINE function_declaration_line+
                    if matches!(self.current_kind(), TokenKind::Public) {
                        let public_funcs =
                            self.parse_public_functions_section(functions_block_level)?;
                        functions.extend(public_funcs);
                        continue;
                    }

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

    /// Parse an inline `public:` sub-section inside a `functions:` block.
    ///
    /// Called after the leading indent and the `public` keyword have been detected.
    /// Consumes `public` `:` NEWLINE then parses each function inside the section,
    /// setting `visibility: Visibility::Public` on each one before returning them.
    /// Functions outside this section remain `Visibility::Private` (the new default
    /// per the 2026-06-25 visibility flip).
    ///
    /// Grammar (foundation/spec/grammar.ebnf §6.2a):
    ///   public_functions_section = INDENT+ "public" ":" NEWLINE
    ///                              function_declaration_line
    ///                              { NEWLINE { empty_line } function_declaration_line }
    fn parse_public_functions_section(
        &mut self,
        parent_block_level: usize,
    ) -> Result<Vec<Function>, CompilerError> {
        // Consume "public"
        self.expect(&TokenKind::Public)?;
        self.skip_whitespace();

        // Consume ":"
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Consume trailing newline
        self.eat(&TokenKind::Newline);
        self.skip_whitespace();

        let mut public_functions: Vec<Function> = Vec::new();

        // The public functions are indented one level deeper than the parent block.
        // We parse them in the same manner as regular functions in a functions: block,
        // but mark each one public after parsing (overriding the new Private default).
        loop {
            self.skip_whitespace();

            if self.is_at_end() {
                break;
            }

            // A dedent at or below the parent block level exits the private section.
            if let TokenKind::Dedent(level) = self.current_kind() {
                if *level <= parent_block_level {
                    break;
                }
                self.bump();
                continue;
            }

            // Top-level keywords exit the private section.
            if matches!(
                self.current_kind(),
                TokenKind::Class
                    | TokenKind::Functions
                    | TokenKind::Tests
                    | TokenKind::Import
                    | TokenKind::State
                    | TokenKind::Start
            ) {
                break;
            }

            // An indent token that brings us to the functions block level or higher:
            // the private sub-section ends when indentation drops back to parent level.
            if let TokenKind::Indent(level) = self.current_kind() {
                let lvl = *level;
                if lvl <= parent_block_level {
                    // Back to parent level - private section is over.
                    break;
                }
                // Consume the deeper indent and continue parsing.
                self.bump();
                self.skip_whitespace();
            }

            if self.is_at_end() {
                break;
            }

            // Check if we're still inside the private section by peeking at what follows.
            match self.current_kind() {
                TokenKind::Identifier(_)
                | TokenKind::Test
                | TokenKind::Unit
                | TokenKind::Error
                | TokenKind::Input
                | TokenKind::Step
                | TokenKind::Description => match self.parse_function_in_block() {
                    Ok(mut func) => {
                        func.visibility = Visibility::Public;
                        public_functions.push(func);
                    }
                    Err(e) => return Err(e),
                },
                TokenKind::Newline => {
                    self.bump();
                }
                _ => {
                    // Nothing more belongs to this public section.
                    break;
                }
            }
        }

        Ok(public_functions)
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
            // Private by default per the 2026-06-25 visibility flip;
            // a function becomes Public only when it appears inside a
            // `public:` sub-section (parse_public_functions_section).
            visibility: Visibility::Private,
            modifier,
            location: Some(start_location),
        })
    }

    /// Parse a method declared inside an `events:` block.
    ///
    /// Per `foundation/spec/plugins/frame-ui.ebnf §events_section`:
    ///
    /// ```text
    /// event_handler_function = identifier , "(" , [ parameter_list ] , ")" , ":" , NEWLINE
    ///                        , INDENT , INDENT , INDENT , { statement } ;
    /// parameter_list         = parameter , { "," , parameter } ;
    /// parameter              = identifier , [ ":" , type ] ;
    /// ```
    ///
    /// The shape `name(params):` is intentionally distinct from
    /// `functions_method` — no return type (handlers are implicitly void),
    /// trailing colon required, and parameters use `name [":" type]` form
    /// instead of `type name`. This is what frame.ui v2.12.4+ emits from
    /// `expand_component` once `normalize_handlers` no longer rewrites
    /// `events:` to `functions:`. Treating events: methods as if they were
    /// ordinary functions: methods (the previous implementation) failed
    /// with "Unsupported statement type: Colon" because the trailing colon
    /// in `onMount():` had no place in the standard method grammar.
    pub(super) fn parse_event_handler_in_block(&mut self) -> Result<Function, CompilerError> {
        let start_location = self.current().location.clone();

        // Method name.
        let name_token = self.expect_name()?;
        let func_name = name_token.text.clone();
        self.skip_whitespace();

        // Parameters: spec form `name [":" type]`, comma-separated.
        self.expect(&TokenKind::LeftParen)?;
        self.skip_whitespace();

        let mut parameters = Vec::new();
        if !self.check(&TokenKind::RightParen) {
            loop {
                self.skip_whitespace();
                let param_name_token = self.expect_name()?;
                let param_name = param_name_token.text.clone();
                self.skip_whitespace();
                // Optional `: type` annotation. Defaults to Any when absent
                // so untyped handler params still resolve.
                let param_type = if self.eat(&TokenKind::Colon) {
                    self.skip_whitespace();
                    self.parse_type()?
                } else {
                    Type::Any
                };
                parameters.push(Parameter {
                    name: param_name,
                    type_: param_type,
                    default_value: None,
                });
                self.skip_whitespace();
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RightParen)?;
        self.skip_whitespace();

        // The trailing colon is REQUIRED in the spec form. We accept its
        // absence to also handle anyone still writing the typed-return form
        // (`void name()`) inside an events: block — though that's outside
        // the spec, callers may rely on the older shape until frame.ui's
        // emit settles. When the colon IS present we expect a newline after
        // it (statement body begins on the next line).
        self.eat(&TokenKind::Colon);
        self.skip_whitespace();

        // Body — same indented-block handling as functions: methods.
        let body = self.parse_block()?;

        Ok(Function {
            name: func_name,
            type_parameters: Vec::new(),
            type_constraints: Vec::new(),
            parameters,
            return_type: Type::Void,
            body,
            description: None,
            syntax: FunctionSyntax::Simple,
            visibility: Visibility::Public,
            modifier: FunctionModifier::None,
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

        // Capability conformance clause (optional): `can C1, C2, ...`
        // Grammar: can_clause = "can", identifier, { ",", identifier }
        // Order rule: `is` must precede `can` — this placement enforces it naturally.
        let mut class_capabilities: Vec<String> = Vec::new();
        if self.eat(&TokenKind::Can) {
            self.skip_whitespace();
            let first_cap = self.expect_identifier()?;
            class_capabilities.push(first_cap.text.clone());
            loop {
                self.skip_whitespace();
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
                self.skip_whitespace();
                let cap_token = self.expect_identifier()?;
                class_capabilities.push(cap_token.text.clone());
            }
            self.skip_whitespace();
        }

        // DON'T skip indentation here - let parse_class_body handle it

        // Record how many synthetic classes exist before parsing this class body.
        // Any new ones added are from this class's sub-sections.
        let synthetic_count_before = self.pending_synthetic_classes.len();

        // Class body
        let (mut fields, methods, constructor, invariants) = self.parse_class_body()?;

        // Rename any `Inputs` synthetic class created for this class to `<Name>Inputs`
        // so that multiple components in the same file don't collide on the type name.
        // Also update the corresponding `Inputs inputs` field type in this class.
        for synthetic in self.pending_synthetic_classes[synthetic_count_before..].iter_mut() {
            let unique_name = format!("{}Inputs", name);
            if synthetic.name == "Inputs" {
                synthetic.name = unique_name.clone();
                // Update the matching field in the parent class (Type::Object("Inputs") → unique)
                for field in fields.iter_mut() {
                    if field.type_ == Type::Object("Inputs".to_string()) {
                        field.type_ = Type::Object(unique_name.clone());
                    }
                }
            }
        }

        Ok(Class {
            name,
            type_parameters: Vec::new(),
            description: None,
            base_class,
            base_class_type_args: Vec::new(),
            fields,
            methods,
            constructor,
            invariants,
            capabilities: class_capabilities,
            location: Some(location),
        })
    }

    pub(super) fn parse_class_body(
        &mut self,
    ) -> Result<
        (
            Vec<Field>,
            Vec<Function>,
            Option<Constructor>,
            Vec<Expression>,
        ),
        CompilerError,
    > {
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut constructor = None;
        let mut invariants: Vec<Expression> = Vec::new();

        while !self.is_at_end()
            && !matches!(
                self.current_kind(),
                TokenKind::Class | TokenKind::Function | TokenKind::Start | TokenKind::Can
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

                        // Check for end of functions block (top-level declarations).
                        // `can` at top level terminates the class body.
                        if matches!(
                            self.current_kind(),
                            TokenKind::Class
                                | TokenKind::Start
                                | TokenKind::Function
                                | TokenKind::Tests
                                | TokenKind::Can
                        ) {
                            break;
                        }

                        // Check for inline public: sub-section inside class functions: block
                        // Grammar (foundation/spec/grammar.ebnf §6.4b):
                        //   public_class_methods_section = INDENT+ "public" ":" NEWLINE
                        //                                  { class_function_line | empty_line }
                        if matches!(self.current_kind(), TokenKind::Public) {
                            let public_methods =
                                self.parse_public_functions_section(functions_indent_level)?;
                            methods.extend(public_methods);
                            continue;
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
                TokenKind::Always => {
                    // Parse always: block
                    // always:
                    //     balance >= 0.0
                    //     balance <= 1000000.0
                    let parsed = self.parse_invariant_block()?;
                    invariants.extend(parsed);
                }
                TokenKind::Public => {
                    // Inline public: sub-section inside a class body.
                    // Handles public class fields (grammar §6.4a public_class_fields_section).
                    // A public: sub-section at the top of the class body (not inside functions:)
                    // contains field declarations that are visible from outside the class.
                    // Fields outside this section default to Private (SEM005).
                    self.bump(); // consume 'public'
                    self.skip_whitespace();
                    self.expect(&TokenKind::Colon)?;
                    self.skip_whitespace();
                    // Parse indented field declarations
                    if let TokenKind::Newline = self.current_kind() {
                        self.bump();
                    }
                    // Consume the indent that opens the public block
                    let public_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_))
                    {
                        if let TokenKind::Indent(level) = self.current_kind() {
                            let level = *level;
                            self.bump();
                            level
                        } else {
                            1
                        }
                    } else {
                        1
                    };

                    loop {
                        self.skip_whitespace();
                        if self.is_at_end() {
                            break;
                        }
                        // Dedent exits public block
                        if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                            if *dedent_level < public_indent_level {
                                break;
                            }
                            self.bump();
                            self.skip_whitespace();
                            continue;
                        }
                        // Skip continuation indent tokens
                        if let TokenKind::Indent(indent_level) = self.current_kind() {
                            if *indent_level < public_indent_level {
                                break;
                            }
                            self.bump();
                        }
                        self.skip_whitespace();
                        if self.is_at_end() {
                            break;
                        }
                        // Stop at any class-level section keyword
                        if matches!(
                            self.current_kind(),
                            TokenKind::Functions
                                | TokenKind::Constructor
                                | TokenKind::Always
                                | TokenKind::Class
                                | TokenKind::Start
                                | TokenKind::Can
                                | TokenKind::Dedent(_)
                        ) {
                            break;
                        }
                        if matches!(self.current_kind(), TokenKind::Identifier(_)) {
                            let mut field = self.parse_field()?;
                            // Fields in a public: sub-section are public (SEM005).
                            field.visibility = Visibility::Public;
                            fields.push(field);
                        } else {
                            break;
                        }
                    }
                }
                TokenKind::Identifier(_) => {
                    // HYDRATE_AUTO Gap 2 — `events:` class section.
                    //
                    // Spec: foundation/spec/plugins/frame-ui.ebnf §events_section.
                    // A class can declare an `events:` block sibling to `functions:`.
                    // Methods declared inside it have identical signatures and bodies
                    // to ordinary methods; the distinction is that the post-expansion
                    // shim pass emits a bare-named top-level dispatch function for
                    // each, so that the browser loader's
                    // `instance.exports[handlerName]()` lookup succeeds when a click
                    // event hits the wrapper. Without this branch every events:-block
                    // would have been swallowed by the generic identifier:colon
                    // sub-section handler below and re-emitted as a synthetic class
                    // of fields — which is what frame.ui v2.12.3 works around by
                    // rewriting `events:` to `functions:` in `normalize_handlers`.
                    let events_section = if let TokenKind::Identifier(name) = self.current_kind() {
                        name == "events" && matches!(self.look_ahead(1).kind, TokenKind::Colon)
                    } else {
                        false
                    };
                    if events_section {
                        self.bump(); // consume 'events'
                        self.skip_whitespace();
                        self.expect(&TokenKind::Colon)?;
                        self.skip_whitespace();

                        let events_indent_level =
                            if matches!(self.current_kind(), TokenKind::Indent(_)) {
                                if let TokenKind::Indent(level) = self.current_kind() {
                                    *level
                                } else {
                                    1
                                }
                            } else {
                                1
                            };

                        // Mirror the methods-parsing loop from the Functions arm.
                        while !self.is_at_end() {
                            self.skip_whitespace();
                            if self.is_at_end() {
                                break;
                            }
                            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                                if *dedent_level < events_indent_level {
                                    break;
                                }
                                self.bump();
                                self.skip_whitespace();
                            }
                            self.skip_indentation();
                            if self.is_at_end() {
                                break;
                            }
                            if matches!(
                                self.current_kind(),
                                TokenKind::Class
                                    | TokenKind::Start
                                    | TokenKind::Function
                                    | TokenKind::Tests
                                    | TokenKind::Functions
                                    | TokenKind::Always
                                    | TokenKind::Public
                                    | TokenKind::Constructor
                                    | TokenKind::Can
                            ) {
                                break;
                            }
                            if matches!(self.current_kind(), TokenKind::Identifier(_)) {
                                match self.parse_event_handler_in_block() {
                                    Ok(mut func) => {
                                        // Tag this method so the post-expansion shim
                                        // pass knows to emit a bare-named dispatch.
                                        func.modifier = crate::ast::FunctionModifier::EventHandler;
                                        methods.push(func);
                                    }
                                    Err(e) => {
                                        warn!(error = %e, "Failed to parse events: method");
                                        break;
                                    }
                                }
                            } else {
                                break;
                            }
                        }
                        continue;
                    }
                    // Look ahead to distinguish three cases:
                    //   1. `identifier ":" newline/indent`  → plugin-injected sub-section header
                    //      (e.g. `inputs:`, `state:` from frame.ui).
                    //      Consume the header and parse the indented field block beneath it.
                    //   2. `identifier ":" type-token`  → name-colon-type field
                    //      (e.g. `id: integer`, `email: string` as emitted by some plugin expanders).
                    //      Parse as a reversed field declaration.
                    //   3. Otherwise → normal `type name` field — delegate to parse_field().
                    if matches!(self.look_ahead(1).kind, TokenKind::Colon) {
                        let token_after_colon = &self.look_ahead(2).kind;
                        if matches!(
                            token_after_colon,
                            TokenKind::Newline | TokenKind::Indent(_) | TokenKind::Eof
                        ) {
                            // Case 1: plugin-injected sub-section header like `inputs:`
                            // These are generated by plugins (e.g. frame.ui) to declare a
                            // named-type field group. For example:
                            //   Inputs inputs        ← field of type Inputs
                            //   inputs:              ← sub-section defining Inputs fields
                            //       string title
                            // We synthesize a standalone `Inputs` class from the sub-section
                            // fields so that `inputs.title` resolves correctly in methods.
                            let section_name =
                                if let TokenKind::Identifier(name) = self.current_kind() {
                                    name.clone()
                                } else {
                                    String::new()
                                };
                            let section_location = self.current().location.clone();
                            self.bump(); // consume identifier (section name)
                            self.bump(); // consume ':'
                                         // Consume optional trailing newline after the colon
                            if matches!(self.current_kind(), TokenKind::Newline) {
                                self.bump();
                            }
                            // Consume the opening Indent token and record the section's indent level
                            let section_indent_level =
                                if let TokenKind::Indent(level) = self.current_kind() {
                                    let level = *level;
                                    self.bump();
                                    level
                                } else {
                                    1
                                };
                            // Parse field declarations inside the sub-section block into a
                            // temporary vector — these become fields of the synthetic class.
                            let mut sub_fields: Vec<Field> = Vec::new();
                            loop {
                                self.skip_whitespace();
                                if self.is_at_end() {
                                    break;
                                }
                                if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                                    if *dedent_level < section_indent_level {
                                        break;
                                    }
                                    self.bump();
                                    self.skip_whitespace();
                                    continue;
                                }
                                if let TokenKind::Indent(indent_level) = self.current_kind() {
                                    if *indent_level < section_indent_level {
                                        break;
                                    }
                                    self.bump();
                                }
                                self.skip_whitespace();
                                if self.is_at_end() {
                                    break;
                                }
                                // Stop at class-level section keywords or top-level declarations.
                                // `can` is a top-level declaration — stop if encountered here.
                                if matches!(
                                    self.current_kind(),
                                    TokenKind::Functions
                                        | TokenKind::Constructor
                                        | TokenKind::Always
                                        | TokenKind::Public
                                        | TokenKind::Class
                                        | TokenKind::Start
                                        | TokenKind::Can
                                        | TokenKind::Dedent(_)
                                ) {
                                    break;
                                }
                                if matches!(self.current_kind(), TokenKind::Identifier(_)) {
                                    sub_fields.push(self.parse_field()?);
                                } else {
                                    break;
                                }
                            }
                            // Synthesize a class from the sub-section if it has fields.
                            // e.g. `inputs:` + [string title] → class Inputs { string title }
                            //
                            // VISIBILITY-FLIP (spec change 2026-06-25): fields default to
                            // Private under the new model. A plugin sub-section like
                            // `inputs:` defines a component's external interface — the
                            // framework reads these fields via `inputs.<field>` from
                            // outside the synthesized class, so they must be Public or
                            // every component using `inputs:` trips SEM007.
                            if !section_name.is_empty() && !sub_fields.is_empty() {
                                let mut synthetic_name = section_name.clone();
                                if let Some(first) = synthetic_name.get_mut(0..1) {
                                    first.make_ascii_uppercase();
                                }
                                for field in sub_fields.iter_mut() {
                                    field.visibility = Visibility::Public;
                                }
                                self.pending_synthetic_classes.push(Class {
                                    name: synthetic_name,
                                    type_parameters: Vec::new(),
                                    description: None,
                                    base_class: None,
                                    base_class_type_args: Vec::new(),
                                    fields: sub_fields,
                                    methods: Vec::new(),
                                    constructor: None,
                                    invariants: Vec::new(),
                                    capabilities: Vec::new(),
                                    location: Some(section_location),
                                });
                            }
                        } else if matches!(token_after_colon, TokenKind::IntegerLiteral(_)) {
                            // Case 3: type-first field with a precision modifier (e.g. `number:64 area`).
                            // The `:` here is a size specifier, not a field-name separator.
                            fields.push(self.parse_field()?);
                        } else {
                            // Case 2: name-colon-type field declaration (e.g. `id: integer`)
                            fields.push(self.parse_field_name_colon_type()?);
                        }
                    } else {
                        // Case 3: normal type-first field declaration (e.g. `integer id`)
                        fields.push(self.parse_field()?);
                    }
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

        Ok((fields, methods, constructor, invariants))
    }

    /// Parse always: block — returns a list of boolean expressions.
    ///
    /// Grammar:
    ///   always_block = "always" ":" newline indent { expression newline } dedent
    pub(super) fn parse_invariant_block(&mut self) -> Result<Vec<Expression>, CompilerError> {
        self.expect(&TokenKind::Always)?;
        self.skip_whitespace();
        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Consume optional leading newline
        if let TokenKind::Newline = self.current_kind() {
            self.bump();
        }

        // Consume opening indent
        let invariant_indent_level = if matches!(self.current_kind(), TokenKind::Indent(_)) {
            if let TokenKind::Indent(level) = self.current_kind() {
                let level = *level;
                self.bump();
                level
            } else {
                1
            }
        } else {
            1
        };

        let mut conditions: Vec<Expression> = Vec::new();

        loop {
            self.skip_whitespace();
            if self.is_at_end() {
                break;
            }

            // A dedent at or below our level means we leave the block
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < invariant_indent_level {
                    break;
                }
                self.bump();
                continue;
            }

            // Continuation indent token at our level
            if let TokenKind::Indent(indent_level) = self.current_kind() {
                if *indent_level < invariant_indent_level {
                    break;
                }
                self.bump();
            }

            self.skip_whitespace();
            if self.is_at_end() {
                break;
            }

            // Stop at class-section keywords, top-level declarations, or EOF.
            // `can` is a top-level declaration — it must not appear inside a class body.
            if matches!(
                self.current_kind(),
                TokenKind::Functions
                    | TokenKind::Constructor
                    | TokenKind::Public
                    | TokenKind::Always
                    | TokenKind::Class
                    | TokenKind::Start
                    | TokenKind::Can
                    | TokenKind::Dedent(_)
                    | TokenKind::Eof
            ) {
                break;
            }

            // Parse the always: condition expression
            let condition = self.parse_expression()?;
            conditions.push(condition);

            // Consume trailing newline after expression
            if let TokenKind::Newline = self.current_kind() {
                self.bump();
            }
        }

        Ok(conditions)
    }

    /// Parse a top-level `can Name:` block into a [`Capability`].
    ///
    /// Grammar (foundation/spec/grammar.ebnf §6.4 and §6.4a):
    /// ```text
    /// can_declaration = "can", identifier, ":", NEWLINE,
    ///                   { empty_line },
    ///                   INDENT, { INDENT }, can_body_item,
    ///                   { NEWLINE, { empty_line }, INDENT, { INDENT }, can_body_item } ;
    ///
    /// can_body_item = can_method_signature, [ can_default_body ] ;
    ///
    /// can_method_signature = [ return_type ], identifier, "(", [ parameter_list ], ")" ;
    ///
    /// can_default_body = NEWLINE, INDENT, INDENT, statement, { NEWLINE, INDENT, INDENT, statement } ;
    /// ```
    ///
    /// A method without a default body is "required" (class must implement it).
    /// A method with an indented default body is "default" (class inherits or overrides).
    pub(super) fn parse_can_declaration(&mut self) -> Result<Capability, CompilerError> {
        let can_token = self.expect(&TokenKind::Can)?;
        let location = can_token.location.clone();
        self.skip_whitespace();

        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();
        self.skip_whitespace();

        self.expect(&TokenKind::Colon)?;
        self.skip_whitespace();

        // Consume optional leading newline after the colon
        if matches!(self.current_kind(), TokenKind::Newline) {
            self.bump();
        }

        // Consume the opening Indent token and record the capability body's indent level.
        // Method signatures sit at this level; default bodies are one level deeper.
        let body_indent_level = if let TokenKind::Indent(level) = self.current_kind() {
            let level = *level;
            self.bump();
            level
        } else {
            1
        };

        let mut methods: Vec<CapabilityMethod> = Vec::new();

        loop {
            self.skip_whitespace();
            if self.is_at_end() {
                break;
            }

            // A Dedent below our level exits the can body.
            if let TokenKind::Dedent(dedent_level) = self.current_kind() {
                if *dedent_level < body_indent_level {
                    break;
                }
                self.bump();
                self.skip_whitespace();
                continue;
            }

            // Continuation Indent tokens at our level.
            if let TokenKind::Indent(indent_level) = self.current_kind() {
                if *indent_level < body_indent_level {
                    break;
                }
                self.bump();
            }

            self.skip_whitespace();
            if self.is_at_end() {
                break;
            }

            // Top-level declarations terminate the can body.
            if matches!(
                self.current_kind(),
                TokenKind::Class
                    | TokenKind::Function
                    | TokenKind::Start
                    | TokenKind::Can
                    | TokenKind::Functions
                    | TokenKind::Import
                    | TokenKind::State
                    | TokenKind::Eof
            ) {
                break;
            }

            // Each line in the body is a can_body_item: method signature with optional default body.
            // Signature format: [return_type] name(params)
            // This mirrors parse_function_in_block's optional-return-type / parameter-list logic.
            let method_location = self.current().location.clone();
            let saved_cursor = self.cursor;

            // Parse [return_type] name — same optional-leading-type logic as parse_function_in_block.
            let (return_type, method_name) = match self.parse_type() {
                Ok(typ) => {
                    self.skip_whitespace();
                    if matches!(typ, Type::Object(_)) && self.check(&TokenKind::LeftParen) {
                        // The identifier was parsed as Type::Object but it's really the method name
                        // (void method, no return type prefix).
                        let name = if let Type::Object(n) = typ {
                            n
                        } else {
                            unreachable!()
                        };
                        (Type::Void, name)
                    } else {
                        // Parsed a real return type; next token is the method name.
                        let name_token = self.expect_name()?;
                        (typ, name_token.text.clone())
                    }
                }
                Err(_) => {
                    // parse_type failed — backtrack and treat the first token as the method name.
                    self.cursor = saved_cursor;
                    let first_token = self.expect_name()?;
                    (Type::Void, first_token.text.clone())
                }
            };

            self.skip_whitespace();

            // Parse parameter list: (type name, ...)
            self.expect(&TokenKind::LeftParen)?;
            self.skip_whitespace();

            let mut parameters: Vec<Parameter> = Vec::new();
            if !self.check(&TokenKind::RightParen) {
                loop {
                    let param_type = self.parse_type()?;
                    self.skip_whitespace();
                    let param_name_token = self.expect_name()?;
                    let param_name = param_name_token.text.clone();
                    self.skip_whitespace();

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

            // Decide whether there is a default body.
            // A default body starts with NEWLINE then an Indent token at a level
            // strictly deeper than the method signature's indent level (body_indent_level).
            // If the next Newline is followed by an Indent at our same level or a Dedent,
            // the next token is another method signature — no default body.
            let default_body: Option<Vec<Statement>> =
                if matches!(self.current_kind(), TokenKind::Newline) {
                    // Peek ahead past the newline to see the indent level.
                    let mut peek_offset = 1;
                    let next_indent_level = loop {
                        let tok = self.look_ahead(peek_offset);
                        match &tok.kind {
                            TokenKind::Indent(level) => break Some(*level),
                            TokenKind::Newline => {
                                peek_offset += 1;
                            }
                            _ => break None,
                        }
                    };

                    if matches!(next_indent_level, Some(level) if level > body_indent_level) {
                        // Consume the newline; parse_block will consume the Indent.
                        self.bump(); // consume Newline
                        Some(self.parse_block()?)
                    } else {
                        // No default body — consume just the newline to advance.
                        self.bump();
                        None
                    }
                } else {
                    // No newline after `)` — no default body.
                    None
                };

            methods.push(CapabilityMethod {
                name: method_name,
                parameters,
                return_type,
                default_body,
                location: Some(method_location),
            });
        }

        Ok(Capability {
            name,
            methods,
            location: Some(location),
        })
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
            // Private by default per the 2026-06-25 visibility flip;
            // a field becomes Public only when it appears inside the
            // class's `public:` sub-section (parse_class_body handler).
            visibility: Visibility::Private,
            is_static: false,
            default_value,
        })
    }

    /// Parse a field declaration in `name: type` format.
    ///
    /// This handles plugin-generated code where the field name comes first and the
    /// type follows after a colon (e.g. `id: integer`, `email: string`).
    /// The frame.ui plugin emits this format for `inputs:` sub-section fields.
    ///
    /// Grammar (plugin extension):
    ///   name_colon_type_field = identifier ":" type [ "=" expression ]
    pub(super) fn parse_field_name_colon_type(&mut self) -> Result<Field, CompilerError> {
        // Parse field name
        let name_token = self.expect_identifier()?;
        let name = name_token.text.clone();

        self.skip_whitespace();

        // Consume ':'
        self.expect(&TokenKind::Colon)?;

        self.skip_whitespace();

        // Parse type (e.g., "string", "integer", "list<integer>")
        let type_ = self.parse_type()?;

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
            // Private by default per the 2026-06-25 visibility flip;
            // a field becomes Public only when it appears inside the
            // class's `public:` sub-section (parse_class_body handler).
            visibility: Visibility::Private,
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
                    | TokenKind::Public
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
                                                       // Consume optional list_behavior suffix.
                                                       // Canonical order: .line then .unique then .pile
                                                       // Any other ordering is a parse error (grammar.ebnf list_behavior).
                    let behavior_order = ["line", "unique", "pile"];
                    let mut last_index: i32 = -1;
                    let mut has_line = false;
                    let mut has_unique = false;
                    let mut has_pile = false;
                    loop {
                        if !matches!(self.current_kind(), TokenKind::Dot) {
                            break;
                        }
                        let saved = self.cursor;
                        self.bump(); // consume '.'
                        if let TokenKind::Identifier(kw) = self.current_kind() {
                            let kw = kw.clone();
                            if let Some(idx) = behavior_order.iter().position(|&s| s == kw.as_str())
                            {
                                let idx = idx as i32;
                                if idx <= last_index {
                                    return Err(CompilerError::parse_error(
                                        format!(
                                            "List behavior modifier '.{}' is out of canonical order. \
                                             Modifiers must follow the order: .line → .unique → .pile",
                                            kw
                                        ),
                                        Some(self.current().location.clone()),
                                        Some("Use the canonical order: .line.unique.pile (omitting unused modifiers)".to_string()),
                                    ));
                                }
                                match kw.as_str() {
                                    "line" => has_line = true,
                                    "unique" => has_unique = true,
                                    "pile" => has_pile = true,
                                    _ => {}
                                }
                                last_index = idx;
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
                    let behavior = match (has_line, has_unique, has_pile) {
                        (false, false, false) => ListBehavior::Default,
                        (true, false, false) => ListBehavior::Line,
                        (false, true, false) => ListBehavior::Unique,
                        (false, false, true) => ListBehavior::Pile,
                        (true, true, false) => ListBehavior::LineUnique,
                        (true, false, true) => ListBehavior::LinePile,
                        (false, true, true) => ListBehavior::PileUnique,
                        (true, true, true) => ListBehavior::LineUniquePile,
                    };
                    Type::List(Box::new(inner_type), behavior)
                } else {
                    // Bare `list` = `list<any>` per root spec 04-type-system.md
                    // §"Bare Generic Type Shorthand". Previously fell through
                    // to Type::Object("list") which then failed SEM007 as an
                    // undefined type (dashboard #015b38a7 for the pairs
                    // variant; same class of bug for list).
                    Type::List(Box::new(Type::Any), ListBehavior::Default)
                }
            }
            "matrix" => {
                // Expect matrix<Type> — or bare `matrix` = `matrix<any>` per
                // root spec 04-type-system.md §"Bare Generic Type Shorthand".
                self.skip_whitespace();
                if matches!(self.current_kind(), TokenKind::Less) {
                    self.bump(); // consume '<'
                    self.skip_whitespace();
                    let inner_type = self.parse_type()?;
                    self.skip_whitespace();
                    self.expect(&TokenKind::Greater)?; // expect '>'
                    Type::Matrix(Box::new(inner_type))
                } else {
                    Type::Matrix(Box::new(Type::Any))
                }
            }
            "pairs" => {
                // Expect pairs<Type, Type> — or bare `pairs` = `pairs<string, any>`
                // per root spec 04-type-system.md §"Bare Generic Type Shorthand".
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
                    Type::Pairs(Box::new(Type::String), Box::new(Type::Any))
                }
            }
            // handler — first-class function reference (WASM i32 table index)
            // grammar.ebnf: sized_type alternatives include handler
            "handler" => Type::Handler,
            other => Type::Object(other.to_string()),
        };

        // Check for precision modifiers (e.g., integer:8, number:32u)
        // Lookahead: only treat colon as precision modifier if followed by an integer literal.
        // Without this guard, `field: type` patterns (where colon is a field separator, not a
        // precision modifier) would be consumed and then fail on the identifier that follows.
        if self.check(&TokenKind::Colon) {
            let saved_cursor = self.cursor;
            self.bump(); // tentatively consume ':'

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
                // Not a precision modifier — restore cursor so the colon stays for the caller
                self.cursor = saved_cursor;
                Ok(base_type)
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
            // source, build, spec, intent are contextual keywords that carry no syntactic
            // weight in most positions — allow them as plain identifiers so field names,
            // function names, and parameters can use these words without triggering a parse
            // error (e.g., `source: string nullable` in a data model field list).
            TokenKind::Source | TokenKind::Build | TokenKind::Spec | TokenKind::Intent => {
                Ok(self.bump())
            }
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
