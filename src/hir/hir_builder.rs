//! HIR Builder - converts AST to HIR (High-level Intermediate Representation)
//!
//! This module implements the transformation from AST to HIR, which includes:
//! - Desugaring syntactic constructs into normalized forms
//! - Validating semantic consistency (but not type checking)
//! - Converting implicit operations to explicit ones
//! - Maintaining source location information for error reporting

use crate::ast::SourceLocation;
use crate::ast::{
    AssignmentTarget, BinaryOperator, Class, Constructor, Expression, Function, ListBehavior,
    Parameter, Program, ResetTarget, Statement, Type, UnaryOperator, ValidateConstraint, Value,
};
use crate::error::CompilerError;
use crate::hir::*;

/// Registry entry for a callable's parameter list, used for named-argument resolution
/// (FUNC008–FUNC011 per foundation/spec/semantic-rules.md).
#[derive(Debug, Clone)]
struct CallableSignature {
    /// Ordered list of parameter names exactly as declared.
    param_names: Vec<String>,
}

/// HIR Builder - constructs HIR from AST
pub struct HirBuilder {
    type_inference_counter: usize,
    warnings: Vec<CompilerError>,
    constant_bindings: std::collections::HashSet<String>,
    /// Nesting depth: 0 = top-level, >0 = inside a function/block.
    /// Used to enforce that apply blocks and import statements may only
    /// appear at the top level of a program.
    scope_depth: usize,
    /// Registry of callable signatures populated during the pre-scan pass.
    /// Keys are function names (for free functions) or `ClassName` (for constructors)
    /// or `ClassName::method` (for methods).
    named_arg_registry: std::collections::HashMap<String, CallableSignature>,
}

impl HirBuilder {
    /// Create a new HIR builder
    pub fn new() -> Self {
        // named_arg_registry is populated lazily via prescan_signatures()

        Self {
            type_inference_counter: 0,
            warnings: Vec::new(),
            constant_bindings: std::collections::HashSet::new(),
            scope_depth: 0,
            named_arg_registry: std::collections::HashMap::new(),
        }
    }

    // =========================================================================
    // Named-argument resolution (FUNC008–FUNC011)
    // =========================================================================

    /// Pre-scan phase: walk the raw AST `Program` and populate `named_arg_registry`
    /// with all user-defined function signatures, constructor signatures, and method
    /// signatures.  This must be called before `build_hir` processes any expressions.
    fn prescan_signatures(&mut self, program: &Program) {
        // Free functions from program.functions
        for func in &program.functions {
            self.register_function_signature(&func.name, &func.parameters);
        }

        // start function
        if let Some(start) = &program.start_function {
            self.register_function_signature("start", &start.parameters);
        }

        // Functions from top-level FunctionsBlock statements
        for stmt in &program.statements {
            if let Statement::FunctionsBlock {
                functions: func_list,
                ..
            } = stmt
            {
                for func in func_list {
                    self.register_function_signature(&func.name, &func.parameters);
                }
            }
        }

        // Classes from program.classes
        for class in &program.classes {
            self.register_class_signatures(class);
        }

        // Classes from ClassDefinition statements
        for stmt in &program.statements {
            if let Statement::ClassDefinition { class, .. } = stmt {
                self.register_class_signatures(class);
            }
        }
    }

    /// Register a free function's parameter list under its name.
    fn register_function_signature(&mut self, name: &str, params: &[Parameter]) {
        let sig = CallableSignature {
            param_names: params.iter().map(|p| p.name.clone()).collect(),
        };
        self.named_arg_registry.insert(name.to_string(), sig);
    }

    /// Register a class constructor and all method signatures.
    fn register_class_signatures(&mut self, class: &Class) {
        // Constructor is registered under the class name itself.
        if let Some(ctor) = &class.constructor {
            let sig = CallableSignature {
                param_names: ctor.parameters.iter().map(|p| p.name.clone()).collect(),
            };
            self.named_arg_registry.insert(class.name.clone(), sig);
        }

        // Each method is registered under `ClassName::methodName`.
        for method in &class.methods {
            let key = format!("{}::{}", class.name, method.name);
            let sig = CallableSignature {
                param_names: method.parameters.iter().map(|p| p.name.clone()).collect(),
            };
            self.named_arg_registry.insert(key, sig);
        }
    }

    /// Check whether `args` contains any `NamedArgBinding` expressions.
    fn has_named_args(args: &[Expression]) -> bool {
        args.iter()
            .any(|a| matches!(a, Expression::NamedArgBinding { .. }))
    }

    /// Resolve a mix of positional and named arguments into canonical positional
    /// order, enforcing FUNC008–FUNC011.
    ///
    /// # Arguments
    /// * `callee` — human-readable name of the callee for error messages
    /// * `sig`    — ordered parameter names of the callee
    /// * `args`   — raw argument list from the AST (may contain NamedArgBinding nodes)
    /// * `location` — source location for error reporting
    fn resolve_named_args<'a>(
        callee: &str,
        sig: &CallableSignature,
        args: &'a [Expression],
        location: &SourceLocation,
    ) -> Result<Vec<&'a Expression>, CompilerError> {
        let param_count = sig.param_names.len();
        let arg_count = args.len();

        // FUNC011: argument count must match parameter count.
        if arg_count != param_count {
            return Err(CompilerError::semantic_error(
                format!(
                    "FUNC011: `{}` expects {} argument(s), got {}",
                    callee, param_count, arg_count
                ),
                Some(format!(
                    "Provide exactly {} argument(s): {}",
                    param_count,
                    sig.param_names.join(", ")
                )),
                Some(location.clone()),
            ));
        }

        // Split into positional prefix and named args.
        let mut last_positional_idx: Option<usize> = None;
        let mut first_named_idx: Option<usize> = None;
        for (i, arg) in args.iter().enumerate() {
            if matches!(arg, Expression::NamedArgBinding { .. }) {
                if first_named_idx.is_none() {
                    first_named_idx = Some(i);
                }
            } else {
                // Positional argument.
                // FUNC010: positional args must precede all named args.
                if let Some(named_start) = first_named_idx {
                    return Err(CompilerError::semantic_error(
                        format!(
                            "FUNC010: positional argument at position {} appears after named \
                             argument at position {} in call to `{}`",
                            i, named_start, callee
                        ),
                        Some(
                            "All positional arguments must come before named arguments".to_string(),
                        ),
                        Some(location.clone()),
                    ));
                }
                last_positional_idx = Some(i);
            }
        }

        // Number of leading positional arguments.
        let positional_count = last_positional_idx.map(|i| i + 1).unwrap_or(0);

        // Build output slot array: None means "not yet filled".
        let mut slots: Vec<Option<&Expression>> = vec![None; param_count];

        // Fill positional slots first.
        for (i, arg) in args[..positional_count].iter().enumerate() {
            slots[i] = Some(arg);
        }

        // Fill named slots.
        // FUNC009: duplicate label detection via a simple scan.
        let mut seen_labels: Vec<&str> = Vec::new();
        for arg in args[positional_count..].iter() {
            if let Expression::NamedArgBinding {
                label,
                value: _,
                location: arg_loc,
            } = arg
            {
                // FUNC009: no duplicate labels.
                if seen_labels.contains(&label.as_str()) {
                    return Err(CompilerError::semantic_error(
                        format!(
                            "FUNC009: duplicate named argument `{}` in call to `{}`",
                            label, callee
                        ),
                        Some(format!("Remove the duplicate `{}:` argument", label)),
                        Some(arg_loc.clone()),
                    ));
                }
                seen_labels.push(label.as_str());

                // FUNC008: label must match a declared parameter name.
                let param_idx = sig.param_names.iter().position(|n| n == label);
                match param_idx {
                    None => {
                        return Err(CompilerError::semantic_error(
                            format!(
                                "FUNC008: `{}` has no parameter named `{}` in call to `{}`",
                                callee, label, callee
                            ),
                            Some(format!(
                                "Valid parameter names are: {}",
                                sig.param_names.join(", ")
                            )),
                            Some(arg_loc.clone()),
                        ));
                    }
                    Some(idx) => {
                        if idx < positional_count {
                            return Err(CompilerError::semantic_error(
                                format!(
                                    "FUNC011: parameter `{}` (position {}) is already covered \
                                     by a positional argument in call to `{}`",
                                    label, idx, callee
                                ),
                                Some(format!(
                                    "Remove the positional argument or the `{}:` label",
                                    label
                                )),
                                Some(arg_loc.clone()),
                            ));
                        }
                        slots[idx] = Some(arg);
                    }
                }
            }
        }

        // Verify all slots are filled (FUNC011 complete coverage).
        let mut result = Vec::with_capacity(param_count);
        for (i, slot) in slots.iter().enumerate() {
            match slot {
                Some(expr) => result.push(*expr),
                None => {
                    return Err(CompilerError::semantic_error(
                        format!(
                            "FUNC011: parameter `{}` (position {}) is not covered in call to `{}`",
                            sig.param_names[i], i, callee
                        ),
                        Some(format!(
                            "Add a `{}:` named argument or a positional argument for this \
                             parameter",
                            sig.param_names[i]
                        )),
                        Some(location.clone()),
                    ));
                }
            }
        }
        Ok(result)
    }

    /// Build HIR expressions from a raw argument list, resolving named arguments
    /// to positional order when the signature is known.
    ///
    /// When the signature is `None` (unknown callee — e.g., an imported function or
    /// a built-in), named arguments are not allowed and we emit a FUNC008 error if
    /// any are present.
    fn build_call_args(
        &mut self,
        callee: &str,
        args: &[Expression],
        location: &SourceLocation,
    ) -> Result<Vec<HirExpression>, CompilerError> {
        if !Self::has_named_args(args) {
            // Fast path: no named args; build directly.
            return args
                .iter()
                .map(|a| self.build_expression(a))
                .collect::<Result<Vec<_>, _>>();
        }

        // Look up the callee's signature.
        let sig = self.named_arg_registry.get(callee).cloned();
        match sig {
            None => {
                // Unknown callee — named args not supported (FUNC008).
                Err(CompilerError::semantic_error(
                    format!(
                        "FUNC008: named arguments are not supported for unknown or built-in \
                         function `{}`; only user-defined functions with known parameter lists \
                         support named arguments",
                        callee
                    ),
                    Some("Remove the named argument labels".to_string()),
                    Some(location.clone()),
                ))
            }
            Some(sig_owned) => {
                // Resolve to positional order.
                let ordered = Self::resolve_named_args(callee, &sig_owned, args, location)?;
                // Now build each expression in canonical order.
                ordered
                    .iter()
                    .map(|expr| {
                        // Unwrap NamedArgBinding to get the inner value expression.
                        let inner = if let Expression::NamedArgBinding { value, .. } = expr {
                            value.as_ref()
                        } else {
                            *expr
                        };
                        self.build_expression(inner)
                    })
                    .collect::<Result<Vec<_>, _>>()
            }
        }
    }

    /// Validate that the five core program sections appear in the required order:
    /// import → start → state → class → functions.
    ///
    /// Auxiliary items (watch, screen, framework, apply, private) are exempt from
    /// this ordering rule, as defined by foundation/spec/grammar.ebnf §6 "TOP-LEVEL DECLARATIONS".
    ///
    /// Returns `Err` with a descriptive `CompilerError` if a core section appears
    /// before a section that is supposed to precede it.
    fn validate_section_order(program: &Program) -> Result<(), CompilerError> {
        /// Numeric rank for the five ordered sections.  Auxiliary sections
        /// return `None` and are skipped during the scan.
        fn section_rank(stmt: &Statement) -> Option<(u8, &'static str, Option<SourceLocation>)> {
            match stmt {
                Statement::Import { location, .. } => Some((0, "import", location.clone())),
                // The start: block is represented as FunctionsBlock containing "start",
                // or directly as a standalone start function in program.start_function.
                // In the statement stream the start block appears as FunctionsBlock
                // where the first function is named "start".
                Statement::FunctionsBlock {
                    functions,
                    location,
                    ..
                } => {
                    if functions.iter().any(|f| f.name == "start") {
                        Some((1, "start", location.clone()))
                    } else {
                        Some((4, "functions", location.clone()))
                    }
                }
                Statement::StateBlockStmt { location, .. } => Some((2, "state", location.clone())),
                Statement::ClassDefinition { location, .. } => Some((3, "class", location.clone())),
                // Auxiliary — exempt from ordering.
                Statement::WatchBlockStmt { .. }
                | Statement::ScreenBlockStmt { .. }
                | Statement::ScreenBlock { .. }
                | Statement::FrameworkBlock { .. }
                | Statement::PrivateBlock { .. }
                | Statement::TestsBlock { .. } => None,
                _ => None,
            }
        }

        let mut highest_rank_seen: i8 = -1;
        let mut highest_name_seen: &'static str = "";

        for stmt in &program.statements {
            if let Some((rank, name, loc)) = section_rank(stmt) {
                if (rank as i8) < highest_rank_seen {
                    // This section appears after a section with a higher rank — violation.
                    let order_str = "import → start → state → class → functions";
                    return Err(CompilerError::syntax_error(
                        format!(
                            "'{name}' section must appear before '{highest_name_seen}' section \
                             (spec requires: {order_str})"
                        ),
                        Some(format!(
                            "Move the '{name}:' block so it appears before '{highest_name_seen}:'"
                        )),
                        loc,
                    ));
                }
                if (rank as i8) > highest_rank_seen {
                    highest_rank_seen = rank as i8;
                    highest_name_seen = name;
                }
            }
        }

        Ok(())
    }

    /// Build HIR from an AST program
    pub fn build_hir(&mut self, program: Program) -> Result<HirValidationResult, CompilerError> {
        // Enforce the spec-mandated section ordering before any other processing.
        Self::validate_section_order(&program)?;

        // Pre-scan phase: populate the named-argument signature registry so that
        // named args can be validated and reordered when expressions are lowered.
        self.prescan_signatures(&program);

        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut start_function = None;
        let mut imports = Vec::new();
        let mut tests: Vec<HirTest> = Vec::new();
        // Top-level `validate name:` blocks are desugared into HIR statements that must be
        // prepended to the start function's body (schemas are initialized before first use).
        let mut validate_preamble: Vec<HirStatement> = Vec::new();

        // Process top-level statements
        for statement in &program.statements {
            match statement {
                Statement::FunctionsBlock {
                    functions: func_list,
                    ..
                } => {
                    for func in func_list {
                        let hir_func = self.build_function(func)?;
                        if func.name == "start" {
                            start_function = Some(hir_func);
                        } else {
                            functions.push(hir_func);
                        }
                    }
                }
                Statement::ClassDefinition { class, .. } => {
                    classes.push(self.build_class(class)?);
                }
                Statement::Import {
                    imports: import_list,
                    ..
                } => {
                    for import_item in import_list {
                        // File path imports ("./module.cln") are resolved by the multi-file
                        // compiler before HIR is built — skip them here to avoid creating
                        // HirImport entries with empty module names (SEM007).
                        if import_item.is_file_import {
                            continue;
                        }

                        // Parse import name to separate module and symbol
                        // Examples:
                        //   "Math" → module: "Math", items: None (whole module)
                        //   "math.sqrt" → module: "math", items: Some(["sqrt"]) (specific symbol)
                        //   "Utils as U" → module: "Utils", items: None, (alias handled separately)
                        //   "Json.decode as jd" → module: "Json", items: Some(["decode"]), (alias handled separately)

                        let (module_name, symbol_items) =
                            if let Some(dot_pos) = import_item.name.find('.') {
                                // Contains dot - import specific symbol(s)
                                let module = &import_item.name[..dot_pos];
                                let symbol = &import_item.name[dot_pos + 1..];
                                (module.to_string(), Some(vec![symbol.to_string()]))
                            } else {
                                // No dot - import whole module
                                (import_item.name.clone(), None)
                            };

                        imports.push(HirImport {
                            module_name,
                            items: symbol_items,
                            location: SourceLocation::default(),
                        });
                    }
                }
                Statement::ValidateDeclaration { schema, location } => {
                    // Top-level `validate name:` blocks desugar into validator.* builder calls.
                    // These are prepended to the start function's body so that the schema
                    // variable is in scope when `.check` is used later in start:.
                    let loc = location.clone().unwrap_or_default();
                    let mut expanded = self.desugar_validate_declaration(schema, &loc)?;
                    validate_preamble.append(&mut expanded);
                }
                Statement::TestsBlock {
                    tests: test_cases,
                    location,
                } => {
                    // Lower each test case into a HirTest node whose body returns
                    // (test_expression == expected_value) as a boolean.
                    let loc = location.clone().unwrap_or_default();
                    for (idx, test_case) in test_cases.iter().enumerate() {
                        let test_loc = test_case.location.clone().unwrap_or_else(|| loc.clone());
                        match &test_case.kind {
                            crate::ast::TestCaseKind::Expression {
                                test_expression,
                                expected_value,
                            } => {
                                let lhs = self.build_expression(test_expression)?;
                                let rhs = self.build_expression(expected_value)?;
                                let equality = HirExpression::BinaryOp {
                                    left: Box::new(lhs),
                                    op: HirBinaryOp::Equal,
                                    right: Box::new(rhs),
                                    location: test_loc.clone(),
                                };
                                let body = HirBlock {
                                    statements: vec![HirStatement::Return {
                                        value: Some(equality),
                                        location: test_loc.clone(),
                                    }],
                                    location: test_loc.clone(),
                                };
                                let name = test_case
                                    .description
                                    .clone()
                                    .unwrap_or_else(|| format!("test_{}", idx));
                                tests.push(HirTest {
                                    name,
                                    description: test_case.description.clone(),
                                    body,
                                    location: test_loc,
                                });
                            }
                            crate::ast::TestCaseKind::Endpoint(endpoint) => {
                                let name = test_case
                                    .description
                                    .clone()
                                    .unwrap_or_else(|| format!("test_{}", idx));
                                let body =
                                    self.build_endpoint_test_body(endpoint, test_loc.clone())?;
                                tests.push(HirTest {
                                    name,
                                    description: test_case.description.clone(),
                                    body,
                                    location: test_loc,
                                });
                            }
                        }
                    }
                }
                _ => {
                    // Handle other top-level statements if needed
                }
            }
        }

        // Process imports from program.imports (top-level import declarations)
        // The parser stores `import Module` statements in program.imports, not in statements.
        for import_item in &program.imports {
            // File path imports ("./module.cln") are resolved by the multi-file compiler
            // before HIR is built — skip them here to avoid SEM007 (empty module name).
            if import_item.is_file_import {
                continue;
            }

            let (module_name, symbol_items) = if let Some(dot_pos) = import_item.name.find('.') {
                let module = &import_item.name[..dot_pos];
                let symbol = &import_item.name[dot_pos + 1..];
                (module.to_string(), Some(vec![symbol.to_string()]))
            } else {
                (import_item.name.clone(), None)
            };
            imports.push(crate::hir::HirImport {
                module_name,
                items: symbol_items,
                location: SourceLocation::default(),
            });
        }

        // Process standalone functions from program.functions
        for func in &program.functions {
            let hir_func = self.build_function(func)?;
            if func.name == "start" {
                start_function = Some(hir_func);
            } else {
                functions.push(hir_func);
            }
        }

        // Process classes from program.classes
        for class in &program.classes {
            classes.push(self.build_class(class)?);
        }

        // Handle the start function if it exists
        if let Some(start_func) = &program.start_function {
            start_function = Some(self.build_function(start_func)?);
        }

        // Process test cases from program.tests (parser stores them directly, not in statements).
        // Each TestCase becomes a HirTest whose body is a single Return(lhs == rhs).
        for (idx, test_case) in program.tests.iter().enumerate() {
            let test_loc = test_case.location.clone().unwrap_or_default();
            match &test_case.kind {
                crate::ast::TestCaseKind::Expression {
                    test_expression,
                    expected_value,
                } => {
                    let lhs = self.build_expression(test_expression)?;
                    let rhs = self.build_expression(expected_value)?;
                    let equality = HirExpression::BinaryOp {
                        left: Box::new(lhs),
                        op: HirBinaryOp::Equal,
                        right: Box::new(rhs),
                        location: test_loc.clone(),
                    };
                    let body = HirBlock {
                        statements: vec![HirStatement::Return {
                            value: Some(equality),
                            location: test_loc.clone(),
                        }],
                        location: test_loc.clone(),
                    };
                    let name = test_case
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("test_{}", idx));
                    tests.push(HirTest {
                        name,
                        description: test_case.description.clone(),
                        body,
                        location: test_loc,
                    });
                }
                crate::ast::TestCaseKind::Endpoint(endpoint) => {
                    let name = test_case
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("test_{}", idx));
                    let body = self.build_endpoint_test_body(endpoint, test_loc.clone())?;
                    tests.push(HirTest {
                        name,
                        description: test_case.description.clone(),
                        body,
                        location: test_loc,
                    });
                }
            }
        }

        // Process state block if present
        let state = if let Some(ast_state) = &program.state {
            Some(self.build_state_block(ast_state)?)
        } else {
            None
        };

        // Process watch blocks
        let watch_blocks = program
            .watch_blocks
            .iter()
            .map(|wb| self.build_watch_block(wb))
            .collect::<Result<Vec<_>, _>>()?;

        // Process external functions (WASM imports)
        let externals = program
            .externals
            .iter()
            .map(|ext| self.build_external_function(ext))
            .collect::<Result<Vec<_>, _>>()?;

        // Process screen blocks (each has its own local state scope for SCOPE005)
        let mut screen_blocks: Vec<crate::hir::HirScreenBlock> = Vec::new();
        for stmt in &program.screen_blocks {
            if let crate::ast::Statement::ScreenBlockStmt {
                name,
                state: screen_state,
                watch_blocks: screen_watches,
                functions: screen_fns,
                location,
            } = stmt
            {
                let hir_state = if let Some(ast_state) = screen_state {
                    Some(self.build_state_block(ast_state)?)
                } else {
                    None
                };
                let hir_watches = screen_watches
                    .iter()
                    .map(|wb| self.build_watch_block(wb))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut hir_fns = screen_fns
                    .iter()
                    .map(|f| self.build_function(f))
                    .collect::<Result<Vec<_>, _>>()?;
                // Set owner_screen on each function so the resolver can enforce SCOPE005
                // while resolving the function body, and promote them to the global
                // function list so they are callable from start: and other functions.
                for hir_fn in hir_fns.iter_mut() {
                    hir_fn.owner_screen = Some(name.clone());
                    functions.push(hir_fn.clone());
                }
                screen_blocks.push(crate::hir::HirScreenBlock {
                    name: name.clone(),
                    state: hir_state,
                    watch_blocks: hir_watches,
                    functions: hir_fns,
                    location: location.clone().unwrap_or_default(),
                });
            }
        }

        // Prepend top-level validate schema initializations to the start function body.
        // This ensures schema variables are declared and initialized before any `.check` usage.
        if !validate_preamble.is_empty() {
            if let Some(ref mut start_fn) = start_function {
                let mut new_stmts = validate_preamble;
                new_stmts.append(&mut start_fn.body.statements);
                start_fn.body.statements = new_stmts;
            }
        }

        let hir_program = HirProgram {
            functions,
            classes,
            start_function,
            imports,
            tests,
            state,
            watch_blocks,
            externals,
            screen_blocks,
            location: program.location.unwrap_or_default(),
        };

        Ok(HirValidationResult {
            hir: hir_program,
            warnings: self.warnings.clone(),
            type_inference_count: self.type_inference_counter,
        })
    }

    /// Convert AST function to HIR function
    fn build_function(&mut self, func: &Function) -> Result<HirFunction, CompilerError> {
        let parameters = func
            .parameters
            .iter()
            .map(|param| self.build_parameter(param))
            .collect::<Result<Vec<_>, _>>()?;

        let return_type = if func.return_type == Type::Void {
            None
        } else {
            Some(self.build_type(&func.return_type)?)
        };

        let body = self.build_block(&func.body)?;

        Ok(HirFunction {
            name: func.name.clone(),
            parameters,
            return_type,
            body,
            is_start: func.name == "start",
            is_private: func.visibility == crate::ast::Visibility::Private,
            owner_screen: None, // Set to Some(screen_name) when building screen functions
            location: func.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST external function to HIR external function
    fn build_external_function(
        &mut self,
        ext: &crate::ast::ExternalFunction,
    ) -> Result<HirExternalFunction, CompilerError> {
        let parameters = ext
            .parameters
            .iter()
            .map(|param| self.build_parameter(param))
            .collect::<Result<Vec<_>, _>>()?;

        let return_type = self.build_type(&ext.return_type)?;

        Ok(HirExternalFunction {
            name: ext.name.clone(),
            parameters,
            return_type,
            module: ext.module.clone(),
            location: ext.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST class to HIR class
    fn build_class(&mut self, class: &Class) -> Result<HirClass, CompilerError> {
        let fields = class
            .fields
            .iter()
            .map(|field| self.build_field(field))
            .collect::<Result<Vec<_>, _>>()?;

        let constructor = if let Some(ctor) = &class.constructor {
            Some(self.build_constructor(ctor, &class.fields)?)
        } else {
            None
        };

        let methods = class
            .methods
            .iter()
            .map(|method| self.build_method(method))
            .collect::<Result<Vec<_>, _>>()?;

        // Build class always: block expressions
        let invariants = class
            .invariants
            .iter()
            .map(|expr| self.build_expression(expr))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(HirClass {
            name: class.name.clone(),
            // Populate generic type parameters from the AST class definition
            type_parameters: class.type_parameters.clone(),
            parent: class.base_class.clone(),
            fields,
            constructor,
            methods,
            invariants,
            location: class.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST field to HIR field
    fn build_field(&mut self, field: &crate::ast::Field) -> Result<HirField, CompilerError> {
        let field_type = self.build_type(&field.type_)?;
        let initializer = if let Some(init) = &field.default_value {
            Some(self.build_expression(init)?)
        } else {
            None
        };

        Ok(HirField {
            name: field.name.clone(),
            field_type,
            initializer,
            is_private: field.visibility == crate::ast::Visibility::Private,
            location: SourceLocation::default(),
        })
    }

    /// Convert AST constructor to HIR constructor
    fn build_constructor(
        &mut self,
        ctor: &Constructor,
        class_fields: &[crate::ast::Field],
    ) -> Result<HirConstructor, CompilerError> {
        let parameters = ctor
            .parameters
            .iter()
            .map(|param| self.build_parameter(param))
            .collect::<Result<Vec<_>, _>>()?;

        let mut body = self.build_block(&ctor.body)?;

        // NOTE: Auto-storing fields feature
        // When constructor body is empty and parameter names match field names,
        // automatically generate field assignments: field = parameter
        if body.statements.is_empty() {
            let mut auto_assignments = Vec::new();

            for param in &ctor.parameters {
                // Check if there's a field with matching name
                if let Some(_field) = class_fields.iter().find(|f| f.name == param.name) {
                    // Generate: field = parameter
                    let assignment = HirStatement::Assignment {
                        target: HirLValue::Variable {
                            name: param.name.clone(),
                            location: SourceLocation::default(),
                        },
                        value: HirExpression::Variable {
                            name: param.name.clone(),
                            location: SourceLocation::default(),
                        },
                        location: SourceLocation::default(),
                    };
                    auto_assignments.push(assignment);
                }
            }

            body.statements = auto_assignments;
        }

        Ok(HirConstructor {
            parameters,
            body,
            location: ctor.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST method to HIR method  
    fn build_method(&mut self, method: &Function) -> Result<HirMethod, CompilerError> {
        let parameters = method
            .parameters
            .iter()
            .map(|param| self.build_parameter(param))
            .collect::<Result<Vec<_>, _>>()?;

        let return_type = self.build_type(&method.return_type)?;
        let body = self.build_block(&method.body)?;

        Ok(HirMethod {
            name: method.name.clone(),
            parameters,
            return_type,
            body,
            is_private: method.visibility == crate::ast::Visibility::Private,
            location: method.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST parameter to HIR parameter
    fn build_parameter(&mut self, param: &Parameter) -> Result<HirParameter, CompilerError> {
        let param_type = self.build_type(&param.type_)?;
        let default_value = if let Some(default_expr) = &param.default_value {
            Some(self.build_expression(default_expr)?)
        } else {
            None
        };

        Ok(HirParameter {
            name: param.name.clone(),
            param_type,
            default_value,
            location: SourceLocation::default(),
        })
    }

    /// Convert AST type to HIR type
    fn build_type(&mut self, ast_type: &Type) -> Result<HirType, CompilerError> {
        match ast_type {
            Type::Boolean => Ok(HirType::Boolean),
            Type::Integer => Ok(HirType::Integer),
            Type::Number => Ok(HirType::Number),
            Type::String => Ok(HirType::String),
            Type::Void => Ok(HirType::Void),
            Type::IntegerSized { bits, unsigned } => match (bits, unsigned) {
                (8, false) => Ok(HirType::Integer8),
                (8, true) => Ok(HirType::Integer8u),
                (16, false) => Ok(HirType::Integer16),
                (16, true) => Ok(HirType::Integer16u),
                (32, false) => Ok(HirType::Integer32),
                (32, true) => Ok(HirType::Integer32u),
                (64, false) => Ok(HirType::Integer64),
                (64, true) => Ok(HirType::Integer64u),
                _ => Err(CompilerError::syntax_error(
                    format!("Unsupported integer size: {bits} bits, unsigned: {unsigned}"),
                    Some("Only 8, 16, 32, and 64 bit integers are supported".to_string()),
                    None,
                )),
            },
            Type::NumberSized { bits } => match bits {
                32 => Ok(HirType::Number32),
                64 => Ok(HirType::Number64),
                _ => Err(CompilerError::syntax_error(
                    format!("Unsupported number size: {bits} bits"),
                    Some("Only 32 and 64 bit numbers are supported".to_string()),
                    None,
                )),
            },
            Type::List(inner, _behavior) => {
                // Behaviour is carried separately on `HirStatement::VariableDeclaration`
                // (set when the var-decl path lowers the AST type). HirType itself
                // is intentionally behaviour-free so list values flowing through the
                // type system don't need to track it everywhere.
                let inner_type = self.build_type(inner)?;
                Ok(HirType::List(Box::new(inner_type)))
            }
            Type::Matrix(inner) => {
                let inner_type = self.build_type(inner)?;
                Ok(HirType::Matrix(Box::new(inner_type)))
            }
            Type::Pairs(key_type, value_type) => {
                let key_hir_type = self.build_type(key_type)?;
                let value_hir_type = self.build_type(value_type)?;
                Ok(HirType::Pairs(
                    Box::new(key_hir_type),
                    Box::new(value_hir_type),
                ))
            }
            Type::Object(name) | Type::Class { name, .. } => Ok(HirType::Named {
                name: name.clone(),
                location: SourceLocation::default(),
            }),
            Type::Any => {
                // 'any' is the top type — compatible with all other types.
                // Map to HirType::Any so the type checker accepts it everywhere,
                // and so HirValidator::validate_type does not reject it as an
                // unknown Named type (it falls through to the _ arm → valid).
                Ok(HirType::Any)
            }
            Type::Handler => {
                // handler is a first-class function reference — at WASM level it is
                // an i32 function-table index, so we map it to Integer in HIR.
                Ok(HirType::Integer)
            }
            _ => {
                // For unsupported types, create an inferred type for now
                self.type_inference_counter += 1;
                Ok(HirType::Inferred {
                    id: self.type_inference_counter,
                    location: SourceLocation::default(),
                })
            }
        }
    }

    /// Convert statements to HIR block
    fn build_block(&mut self, statements: &[Statement]) -> Result<HirBlock, CompilerError> {
        // Enter a nested scope: apply blocks and imports are now forbidden.
        self.scope_depth += 1;
        let result = self.build_block_inner(statements);
        self.scope_depth -= 1;
        result
    }

    /// Inner implementation of build_block, called after scope_depth has been incremented.
    fn build_block_inner(&mut self, statements: &[Statement]) -> Result<HirBlock, CompilerError> {
        let mut hir_statements = Vec::new();

        for stmt in statements {
            // Validate that imports do not appear inside nested scopes.
            // Apply blocks (type, constant, function, method) are valid inside function bodies per the spec
            // (grammar.ebnf line ~426: statement includes apply_block).
            if let Statement::Import { location, .. } = stmt {
                let loc = location.clone().unwrap_or_default();
                return Err(CompilerError::validation_error(
                    "import statements must appear at the top level of a program, not inside a function or block",
                    loc,
                ));
            }

            // Special handling for TypeApplyBlock - expand into multiple statements
            if let Statement::TypeApplyBlock {
                type_,
                assignments,
                location,
            } = stmt
            {
                tracing::debug!(
                    type_ = ?type_,
                    assignments_count = assignments.len(),
                    "Expanding TypeApplyBlock into variable declarations"
                );
                // Convert each assignment in the apply block to a variable declaration
                for assignment in assignments {
                    let var_type = self.build_type(type_)?;
                    let init_expr = if let Some(init) = &assignment.initializer {
                        Some(self.build_expression(init)?)
                    } else {
                        None
                    };

                    tracing::debug!(
                        variable_name = %assignment.name,
                        var_type = ?var_type,
                        "Created VariableDeclaration from TypeApplyBlock"
                    );

                    let decl_loc = location.clone().unwrap_or_default();
                    hir_statements.push(HirStatement::VariableDeclaration {
                        name: assignment.name.clone(),
                        var_type,
                        initializer: init_expr,
                        is_mutable: true, // Apply blocks create mutable variables
                        location: decl_loc.clone(),
                    });
                    // Inject behavior flags for list type apply blocks with non-Default behavior.
                    if let Type::List(_, behavior) = type_ {
                        if *behavior != ListBehavior::Default {
                            hir_statements.push(self.make_set_behavior_stmt(
                                &assignment.name,
                                *behavior,
                                decl_loc,
                            ));
                        }
                    }
                }
            } else if let Statement::ConstantApplyBlock {
                constants,
                location,
            } = stmt
            {
                // Convert each constant in the apply block to a variable declaration
                // Constants are treated as immutable variables in HIR
                for constant in constants {
                    let var_type = self.build_type(&constant.type_)?;
                    let init_expr = Some(self.build_expression(&constant.value)?);

                    // Track this as a constant binding for resolver
                    self.constant_bindings.insert(constant.name.clone());

                    hir_statements.push(HirStatement::VariableDeclaration {
                        name: constant.name.clone(),
                        var_type,
                        initializer: init_expr,
                        is_mutable: false, // Constant apply blocks create immutable variables
                        location: location.clone().unwrap_or_default(),
                    });
                }
            } else if let Statement::ValidateDeclaration { schema, location } = stmt {
                // Desugar `validate name:` into a sequence of validator.* calls that build
                // the ValidationRules object and store it as a local variable.
                let loc = location.clone().unwrap_or_default();
                let mut expanded = self.desugar_validate_declaration(schema, &loc)?;
                hir_statements.append(&mut expanded);
            } else if let Statement::ValidateCheck { check, location } = stmt {
                // Desugar `schemaName.check expr:` into a validator.run + isOk branch.
                let loc = location.clone().unwrap_or_default();
                let mut expanded = self.desugar_validate_check(check, &loc)?;
                hir_statements.append(&mut expanded);
            } else {
                // Regular statement processing
                hir_statements.push(self.build_statement(stmt)?);
                // Inject behavior flags for list declarations with non-Default behavior.
                if let Statement::VariableDecl {
                    name,
                    type_: Type::List(_, behavior),
                    location,
                    ..
                } = stmt
                {
                    if *behavior != ListBehavior::Default {
                        let loc = location.clone().unwrap_or_default();
                        hir_statements.push(self.make_set_behavior_stmt(name, *behavior, loc));
                    }
                }
            }
        }

        Ok(HirBlock {
            statements: hir_statements,
            location: SourceLocation::default(),
        })
    }

    /// Convert AST statement to HIR statement
    fn build_statement(&mut self, stmt: &Statement) -> Result<HirStatement, CompilerError> {
        match stmt {
            Statement::VariableDecl {
                name,
                type_,
                initializer,
                location,
            } => {
                let var_type = self.build_type(type_)?;
                let init_expr = if let Some(init) = initializer {
                    Some(self.build_expression(init)?)
                } else {
                    None
                };

                Ok(HirStatement::VariableDeclaration {
                    name: name.clone(),
                    var_type,
                    initializer: init_expr,
                    is_mutable: true, // Regular variable declarations are mutable by default
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Assignment {
                target,
                value,
                location,
            } => {
                let loc = location.clone().unwrap_or_default();
                let lvalue = match target {
                    AssignmentTarget::Variable(name) => HirLValue::Variable {
                        name: name.clone(),
                        location: loc.clone(),
                    },
                    AssignmentTarget::Index { collection, index } => {
                        let array_expr = HirExpression::Variable {
                            name: collection.clone(),
                            location: loc.clone(),
                        };
                        let index_expr = self.build_expression(index)?;
                        HirLValue::Index {
                            array: Box::new(array_expr),
                            index: Box::new(index_expr),
                            location: loc.clone(),
                        }
                    }
                    AssignmentTarget::Property { object, path } => {
                        // Build a chain of FieldAccess expressions for nested paths.
                        // For a single-element path `obj.field`, there is exactly one field.
                        // For `obj.a.b`, we nest: FieldAccess(FieldAccess(Variable(obj), a), b).
                        let base = HirExpression::Variable {
                            name: object.clone(),
                            location: loc.clone(),
                        };
                        let (last_field, prefix) = path.split_last().expect(
                            "AssignmentTarget::Property requires at least one path element",
                        );
                        let inner =
                            prefix
                                .iter()
                                .fold(base, |acc, seg| HirExpression::FieldAccess {
                                    object: Box::new(acc),
                                    field: seg.clone(),
                                    location: loc.clone(),
                                });
                        HirLValue::FieldAccess {
                            object: Box::new(inner),
                            field: last_field.clone(),
                            location: loc.clone(),
                        }
                    }
                };
                let hir_value = self.build_expression(value)?;

                Ok(HirStatement::Assignment {
                    target: lvalue,
                    value: hir_value,
                    location: loc,
                })
            }

            Statement::Print {
                expression,
                newline,
                location,
            } => {
                let hir_expr = self.build_expression(expression)?;
                Ok(HirStatement::Print {
                    expression: hir_expr,
                    newline: *newline,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Return { value, location } => {
                let return_value = if let Some(expr) = value {
                    Some(self.build_expression(expr)?)
                } else {
                    None
                };

                Ok(HirStatement::Return {
                    value: return_value,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::If {
                condition,
                then_branch,
                else_branch,
                location,
            } => {
                let hir_condition = self.build_expression(condition)?;
                let hir_then = self.build_block(then_branch)?;
                let hir_else = if let Some(else_stmts) = else_branch {
                    Some(self.build_block(else_stmts)?)
                } else {
                    None
                };

                Ok(HirStatement::If {
                    condition: hir_condition,
                    then_branch: hir_then,
                    else_branch: hir_else,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Iterate {
                iterator,
                collection,
                body,
                location,
            } => {
                let hir_iterable = self.build_expression(collection)?;
                let hir_body = self.build_block(body)?;

                Ok(HirStatement::For {
                    variable: iterator.clone(),
                    iterable: hir_iterable,
                    body: hir_body,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::RangeIterate {
                iterator,
                start,
                end,
                step,
                inclusive,
                body,
                location,
            } => {
                // Convert RangeIterate to For with a Range expression as the iterable
                let hir_start = self.build_expression(start)?;
                let hir_end = self.build_expression(end)?;
                let hir_step = if let Some(s) = step {
                    Some(Box::new(self.build_expression(s)?))
                } else {
                    None
                };

                let range_expr = HirExpression::Range {
                    start: Box::new(hir_start),
                    end: Box::new(hir_end),
                    step: hir_step,
                    inclusive: *inclusive,
                    location: location.clone().unwrap_or_default(),
                };

                let hir_body = self.build_block(body)?;

                Ok(HirStatement::For {
                    variable: iterator.clone(),
                    iterable: range_expr,
                    body: hir_body,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::While {
                condition,
                body,
                location,
            } => {
                let hir_condition = self.build_expression(condition)?;
                let hir_body = self.build_block(body)?;

                Ok(HirStatement::While {
                    condition: hir_condition,
                    body: hir_body,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Expression { expr, location } => {
                let hir_expr = self.build_expression(expr)?;
                Ok(HirStatement::Expression {
                    expression: hir_expr,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::LaterAssignment {
                variable,
                expression,
                location,
            } => {
                let hir_expr = self.build_expression(expression)?;
                Ok(HirStatement::LaterAssignment {
                    variable: variable.clone(),
                    expression: hir_expr,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Background {
                expression,
                location,
            } => {
                let hir_expr = self.build_expression(expression)?;
                Ok(HirStatement::Background {
                    expression: hir_expr,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Break { location } => Ok(HirStatement::Break {
                location: location.clone().unwrap_or_default(),
            }),

            Statement::Continue { location } => Ok(HirStatement::Continue {
                location: location.clone().unwrap_or_default(),
            }),

            Statement::Require {
                condition,
                location,
            } => {
                let hir_condition = self.build_expression(condition)?;
                Ok(HirStatement::Require {
                    condition: hir_condition,
                    location: location.clone().unwrap_or_default(),
                })
            }

            Statement::Ensure {
                condition,
                location,
            } => {
                let hir_condition = self.build_expression(condition)?;
                Ok(HirStatement::Ensure {
                    condition: hir_condition,
                    location: location.clone().unwrap_or_default(),
                })
            }

            // reset_statement — emit a host call: _state_reset_all or _state_reset_named
            // grammar.ebnf: reset_statement = "reset" , ( "state" | identifier ) ;
            Statement::ResetStmt { target, location } => {
                let loc = location.clone().unwrap_or_default();
                match target {
                    ResetTarget::AllState => {
                        // reset state → call _state_reset_all()
                        Ok(HirStatement::Expression {
                            expression: HirExpression::Call {
                                function: "_state_reset_all".to_string(),
                                arguments: vec![],
                                location: loc.clone(),
                            },
                            location: loc,
                        })
                    }
                    ResetTarget::Variable(name) => {
                        // reset <name> → call _state_reset_named(name_ptr, name_len)
                        // We encode the name as a string literal argument
                        Ok(HirStatement::Expression {
                            expression: HirExpression::Call {
                                function: "_state_reset_named".to_string(),
                                arguments: vec![HirExpression::Literal {
                                    value: Value::String(name.clone()),
                                    location: loc.clone(),
                                }],
                                location: loc.clone(),
                            },
                            location: loc,
                        })
                    }
                }
            }

            // TypeApplyBlock is handled in build_block() where it can expand into multiple statements
            _ => {
                // For unsupported statements, create a dummy expression statement
                Ok(HirStatement::Expression {
                    expression: HirExpression::Literal {
                        value: Value::Void,
                        location: SourceLocation::default(),
                    },
                    location: SourceLocation::default(),
                })
            }
        }
    }

    /// Convert AST expression to HIR expression
    fn build_expression(&mut self, expr: &Expression) -> Result<HirExpression, CompilerError> {
        match expr {
            Expression::Literal(value) => {
                // NOTE: Array literals must be converted to HirExpression::Array
                // not HirExpression::Literal, otherwise they get converted to Null in type inference
                match value {
                    Value::List(elements) => {
                        // Convert list literal to Array expression
                        let mut hir_elements = Vec::new();
                        for elem in elements {
                            hir_elements
                                .push(self.build_expression(&Expression::Literal(elem.clone()))?);
                        }

                        // Infer element type from first element, or use Void for empty lists
                        let element_type = if let Some(first_elem) = elements.first() {
                            self.value_to_hir_type(first_elem)
                        } else {
                            HirType::Void // Will be inferred from context in type checker
                        };

                        Ok(HirExpression::Array {
                            elements: hir_elements,
                            element_type,
                            location: SourceLocation::default(),
                        })
                    }
                    _ => Ok(HirExpression::Literal {
                        value: value.clone(),
                        location: SourceLocation::default(),
                    }),
                }
            }

            Expression::Variable(name) => Ok(HirExpression::Variable {
                name: name.clone(),
                location: SourceLocation::default(),
            }),

            Expression::Binary(left, op, right) => {
                let hir_left = self.build_expression(left)?;
                let hir_right = self.build_expression(right)?;
                let hir_op = self.convert_binary_op(op);

                Ok(HirExpression::BinaryOp {
                    left: Box::new(hir_left),
                    op: hir_op,
                    right: Box::new(hir_right),
                    location: SourceLocation::default(),
                })
            }

            Expression::Unary(op, operand) => {
                let hir_operand = self.build_expression(operand)?;
                let hir_op = self.convert_unary_op(op);

                Ok(HirExpression::UnaryOp {
                    op: hir_op,
                    operand: Box::new(hir_operand),
                    location: SourceLocation::default(),
                })
            }

            Expression::Call(name, args) => {
                // NOTE: Detect base() calls for parent constructor invocation
                // base() is a special function call that invokes the parent class constructor
                if name == "base" {
                    let hir_args =
                        self.build_call_args("base", args, &SourceLocation::default())?;
                    Ok(HirExpression::BaseCall {
                        arguments: hir_args,
                        location: SourceLocation::default(),
                    })
                } else {
                    let hir_args = self.build_call_args(name, args, &SourceLocation::default())?;
                    Ok(HirExpression::Call {
                        function: name.clone(),
                        arguments: hir_args,
                        location: SourceLocation::default(),
                    })
                }
            }

            Expression::MethodCall {
                object,
                method,
                arguments,
                location,
            } => {
                let hir_object = self.build_expression(object)?;

                // For method calls, attempt named-argument resolution by looking up
                // the method in the registry under the receiver's type name if known.
                // Since we don't have full type information at HIR stage, we try a best-
                // effort lookup: if any class exposes a method with this name and the arg
                // count matches, use that signature.  If multiple matches or none, fall
                // back to an error when named args are present.
                let hir_args = if Self::has_named_args(arguments) {
                    // Attempt resolution: search for `AnyClass::method` in the registry.
                    let candidates: Vec<CallableSignature> = self
                        .named_arg_registry
                        .iter()
                        .filter(|(k, sig)| {
                            k.ends_with(&format!("::{}", method))
                                && sig.param_names.len() == arguments.len()
                        })
                        .map(|(_, v)| v.clone())
                        .collect();

                    match candidates.len() {
                        0 => {
                            return Err(CompilerError::semantic_error(
                                format!(
                                    "FUNC008: named arguments used in method call `.{}()` but no \
                                     matching method signature was found in any class",
                                    method
                                ),
                                Some(
                                    "Ensure the method is defined in a class visible to this call \
                                     site, or use positional arguments"
                                        .to_string(),
                                ),
                                Some(location.clone()),
                            ));
                        }
                        1 => {
                            let sig = candidates
                                .into_iter()
                                .next()
                                .expect("invariant: match arm len==1 guarantees one element");
                            let ordered =
                                Self::resolve_named_args(method, &sig, arguments, location)?;
                            ordered
                                .iter()
                                .map(|expr| {
                                    let inner =
                                        if let Expression::NamedArgBinding { value, .. } = expr {
                                            value.as_ref()
                                        } else {
                                            *expr
                                        };
                                    self.build_expression(inner)
                                })
                                .collect::<Result<Vec<_>, _>>()?
                        }
                        _ => {
                            return Err(CompilerError::semantic_error(
                                format!(
                                    "FUNC008: named arguments in method call `.{}()` are \
                                     ambiguous — multiple classes define a method with this \
                                     name and argument count",
                                    method
                                ),
                                Some(
                                    "Use positional arguments to avoid ambiguity, or qualify \
                                     the receiver type"
                                        .to_string(),
                                ),
                                Some(location.clone()),
                            ));
                        }
                    }
                } else {
                    arguments
                        .iter()
                        .map(|arg| self.build_expression(arg))
                        .collect::<Result<Vec<_>, _>>()?
                };

                Ok(HirExpression::MethodCall {
                    receiver: Box::new(hir_object),
                    method: method.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            // ChainedMethodCall — foundation/spec/grammar.ebnf `chained_method_call`.
            // Lowered to a left-to-right fold of HirExpression::MethodCall nodes.
            Expression::ChainedMethodCall {
                receiver,
                chain,
                location,
            } => {
                let mut hir_recv = self.build_expression(receiver)?;
                for (method, args) in chain {
                    let hir_args = args
                        .iter()
                        .map(|arg| self.build_expression(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    hir_recv = HirExpression::MethodCall {
                        receiver: Box::new(hir_recv),
                        method: method.clone(),
                        arguments: hir_args,
                        location: location.clone(),
                    };
                }
                Ok(hir_recv)
            }

            // MultipleMethodCall — foundation/spec/grammar.ebnf `multiple_method_call`.
            // Structurally identical to ChainedMethodCall; lowered the same way.
            Expression::MultipleMethodCall {
                receiver,
                chain,
                location,
            } => {
                let mut hir_recv = self.build_expression(receiver)?;
                for (method, args) in chain {
                    let hir_args = args
                        .iter()
                        .map(|arg| self.build_expression(arg))
                        .collect::<Result<Vec<_>, _>>()?;
                    hir_recv = HirExpression::MethodCall {
                        receiver: Box::new(hir_recv),
                        method: method.clone(),
                        arguments: hir_args,
                        location: location.clone(),
                    };
                }
                Ok(hir_recv)
            }

            // ThreeLevelMethodCall — foundation/spec/grammar.ebnf `three_level_method_call`.
            // `first.second.method(args)` — lowered to nested FieldAccess then MethodCall.
            Expression::ThreeLevelMethodCall {
                first,
                second,
                method,
                arguments,
                location,
            } => {
                let base = HirExpression::Variable {
                    name: first.clone(),
                    location: location.clone(),
                };
                let intermediate = HirExpression::FieldAccess {
                    object: Box::new(base),
                    field: second.clone(),
                    location: location.clone(),
                };
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(HirExpression::MethodCall {
                    receiver: Box::new(intermediate),
                    method: method.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            // PropertyMethodCall — foundation/spec/grammar.ebnf `property_method_call`.
            // `obj.a.b...method(args)` — lowered to a chain of FieldAccess then MethodCall.
            Expression::PropertyMethodCall {
                object,
                path,
                method,
                arguments,
                location,
            } => {
                let base = HirExpression::Variable {
                    name: object.clone(),
                    location: location.clone(),
                };
                let receiver = path
                    .iter()
                    .fold(base, |acc, seg| HirExpression::FieldAccess {
                        object: Box::new(acc),
                        field: seg.clone(),
                        location: location.clone(),
                    });
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(HirExpression::MethodCall {
                    receiver: Box::new(receiver),
                    method: method.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::PropertyAccess {
                object,
                property,
                location,
            } => {
                let hir_object = self.build_expression(object)?;

                Ok(HirExpression::FieldAccess {
                    object: Box::new(hir_object),
                    field: property.clone(),
                    location: location.clone(),
                })
            }

            Expression::ListAccess(array, index) => {
                let hir_array = self.build_expression(array)?;
                let hir_index = self.build_expression(index)?;

                Ok(HirExpression::Index {
                    array: Box::new(hir_array),
                    index: Box::new(hir_index),
                    location: SourceLocation::default(),
                })
            }

            Expression::ObjectCreation {
                class_name,
                arguments,
                location,
            } => {
                // The constructor signature is registered under the class name.
                let hir_args = self.build_call_args(class_name, arguments, location)?;

                Ok(HirExpression::Constructor {
                    class_name: class_name.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::NamespaceCall {
                namespace,
                function,
                arguments,
                location,
            } => {
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::NamespaceCall {
                    namespace: namespace.clone(),
                    function: function.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::StaticMethodCall {
                namespace,
                class_name,
                method,
                arguments,
                location,
            } => {
                let hir_args = arguments
                    .iter()
                    .map(|arg| self.build_expression(arg))
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(HirExpression::StaticMethodCall {
                    namespace: namespace.clone(),
                    class_name: class_name.clone(),
                    method: method.clone(),
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::OnError {
                expression,
                fallback,
                location,
            } => {
                let hir_expression = self.build_expression(expression)?;
                let hir_fallback = self.build_expression(fallback)?;

                Ok(HirExpression::OnError {
                    expression: Box::new(hir_expression),
                    fallback: Box::new(hir_fallback),
                    location: location.clone(),
                })
            }

            Expression::Conditional {
                condition,
                then_expr,
                else_expr,
                location,
            } => {
                let hir_condition = self.build_expression(condition)?;
                let hir_then = self.build_expression(then_expr)?;
                let hir_else = self.build_expression(else_expr)?;

                Ok(HirExpression::Conditional {
                    condition: Box::new(hir_condition),
                    then_expr: Box::new(hir_then),
                    else_expr: Box::new(hir_else),
                    location: location.clone(),
                })
            }

            // NOTE: Handle base() calls from AST
            // The parser creates Expression::BaseCall, so we must handle it here
            Expression::BaseCall {
                arguments,
                location,
            } => {
                // For `base()`, we look up the parent class constructor.
                // At this point we don't know the parent class name, so we use "base"
                // as the registry key and fall back to positional if not found.
                let hir_args = self.build_call_args("base", arguments, location)?;

                Ok(HirExpression::BaseCall {
                    arguments: hir_args,
                    location: location.clone(),
                })
            }

            Expression::Range {
                start,
                end,
                inclusive,
                location,
            } => {
                let hir_start = self.build_expression(start)?;
                let hir_end = self.build_expression(end)?;

                Ok(HirExpression::Range {
                    start: Box::new(hir_start),
                    end: Box::new(hir_end),
                    step: None, // Expression::Range doesn't have step; it's in Statement::RangeIterate
                    inclusive: *inclusive,
                    location: location.clone(),
                })
            }

            // `start expr` — async launch marker; strip the wrapper and build the inner call.
            // The async dispatch is handled at statement level (LaterAssignment / Background).
            Expression::StartExpression { expression, .. } => self.build_expression(expression),

            // NamedArgBinding is consumed entirely within build_call_args.
            // If we reach here it means the parser emitted a NamedArgBinding in a
            // position that is not a call argument list (e.g. a standalone expression).
            // Emit a clear semantic error rather than silently producing garbage code.
            Expression::NamedArgBinding {
                label, location, ..
            } => Err(CompilerError::semantic_error(
                format!(
                    "FUNC008: named argument `{}:` used outside of a function call argument list",
                    label
                ),
                Some(
                    "Named argument syntax `label: value` is only valid inside call parentheses"
                        .to_string(),
                ),
                Some(location.clone()),
            )),

            _ => {
                // For unsupported expressions, create a void literal
                Ok(HirExpression::Literal {
                    value: Value::Void,
                    location: SourceLocation::default(),
                })
            }
        }
    }

    /// Convert AST binary operator to HIR binary operator
    fn convert_binary_op(&self, op: &BinaryOperator) -> HirBinaryOp {
        match op {
            BinaryOperator::Add => HirBinaryOp::Add,
            BinaryOperator::Subtract => HirBinaryOp::Subtract,
            BinaryOperator::Multiply => HirBinaryOp::Multiply,
            BinaryOperator::Divide => HirBinaryOp::Divide,
            BinaryOperator::Modulo => HirBinaryOp::Modulo,
            BinaryOperator::Power => HirBinaryOp::Power,
            BinaryOperator::Equal => HirBinaryOp::Equal,
            BinaryOperator::NotEqual => HirBinaryOp::NotEqual,
            BinaryOperator::Less => HirBinaryOp::Less,
            BinaryOperator::Greater => HirBinaryOp::Greater,
            BinaryOperator::LessEqual => HirBinaryOp::LessEqual,
            BinaryOperator::GreaterEqual => HirBinaryOp::GreaterEqual,
            BinaryOperator::Is => HirBinaryOp::Is,
            BinaryOperator::Not => HirBinaryOp::IsNot,
            BinaryOperator::And => HirBinaryOp::And,
            BinaryOperator::Or => HirBinaryOp::Or,
            BinaryOperator::Default => HirBinaryOp::NullCoalesce,
        }
    }

    /// Convert AST unary operator to HIR unary operator
    fn convert_unary_op(&self, op: &UnaryOperator) -> HirUnaryOp {
        match op {
            UnaryOperator::Negate => HirUnaryOp::Negate,
            UnaryOperator::Not => HirUnaryOp::Not,
            UnaryOperator::RequiredAssert => HirUnaryOp::Required,
        }
    }

    fn value_to_hir_type(&self, value: &Value) -> HirType {
        match value {
            Value::Integer(_) => HirType::Integer,
            Value::Number(_) => HirType::Number,
            Value::String(_) => HirType::String,
            Value::Boolean(_) => HirType::Boolean,
            Value::Integer8(_) => HirType::Integer8,
            Value::Integer8u(_) => HirType::Integer8u,
            Value::Integer16(_) => HirType::Integer16,
            Value::Integer16u(_) => HirType::Integer16u,
            Value::Integer32(_) => HirType::Integer32,
            Value::Integer64(_) => HirType::Integer64,
            Value::Number32(_) => HirType::Number32,
            Value::Number64(_) => HirType::Number64,
            Value::List(elements) => {
                let element_type = if let Some(first) = elements.first() {
                    Box::new(self.value_to_hir_type(first))
                } else {
                    // Empty list - use Void as placeholder, will be inferred from context
                    Box::new(HirType::Void)
                };
                HirType::List(element_type)
            }
            Value::Matrix(_) => {
                // Matrix type will be inferred properly in type checker
                HirType::Matrix(Box::new(HirType::Number))
            }
            Value::Pairs(_) => {
                // Pairs type will be inferred properly in type checker
                HirType::Pairs(Box::new(HirType::Void), Box::new(HirType::Void))
            }
            Value::None | Value::Void => HirType::Void,
        }
    }

    /// Convert AST state block to HIR state block
    fn build_state_block(
        &mut self,
        state_block: &crate::ast::StateBlock,
    ) -> Result<HirStateBlock, CompilerError> {
        let declarations = state_block
            .declarations
            .iter()
            .map(|decl| self.build_state_declaration(decl))
            .collect::<Result<Vec<_>, _>>()?;

        let computed = state_block
            .computed
            .iter()
            .map(|comp| self.build_computed_declaration(comp))
            .collect::<Result<Vec<_>, _>>()?;

        // Convert rules from AST to HIR expressions
        let rules = if let Some(ref rules_block) = state_block.rules {
            rules_block
                .rules
                .iter()
                .map(|expr| self.build_expression(expr))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };

        let scope = match state_block.scope {
            crate::ast::StateScope::App => HirStateScope::App,
            crate::ast::StateScope::Screen => HirStateScope::Screen,
        };

        Ok(HirStateBlock {
            declarations,
            computed,
            rules,
            scope,
            location: state_block.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST state declaration to HIR state declaration
    fn build_state_declaration(
        &mut self,
        decl: &crate::ast::StateDeclaration,
    ) -> Result<HirStateDeclaration, CompilerError> {
        let state_type = self.build_type(&decl.type_)?;
        let initializer = self.build_expression(&decl.initializer)?;

        let guard = if let Some(ast_guard) = &decl.guard {
            Some(HirGuardClause {
                condition: self.build_expression(&ast_guard.condition)?,
                error_message: ast_guard.error_message.clone(),
                location: ast_guard.location.clone().unwrap_or_default(),
            })
        } else {
            None
        };

        Ok(HirStateDeclaration {
            name: decl.name.clone(),
            state_type,
            initializer,
            guard,
            is_private: decl.is_private,
            location: decl.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST computed declaration to HIR computed declaration
    fn build_computed_declaration(
        &mut self,
        comp: &crate::ast::ComputedDeclaration,
    ) -> Result<HirComputedDeclaration, CompilerError> {
        let computed_type = self.build_type(&comp.type_)?;
        let body = self.build_block(&comp.body)?;

        Ok(HirComputedDeclaration {
            name: comp.name.clone(),
            computed_type,
            body,
            location: comp.location.clone().unwrap_or_default(),
        })
    }

    /// Convert AST watch block to HIR watch block
    fn build_watch_block(
        &mut self,
        watch: &crate::ast::WatchBlock,
    ) -> Result<HirWatchBlock, CompilerError> {
        let body = self.build_block(&watch.body)?;

        Ok(HirWatchBlock {
            targets: watch.targets.clone(),
            body,
            location: watch.location.clone().unwrap_or_default(),
        })
    }

    // =========================================================================
    // Validate block desugaring
    //
    // `validate name:` is syntactic sugar for a sequence of `validator.*` calls
    // that build up a ValidationRules object. We desugar directly in the HIR
    // builder so that the rest of the pipeline sees only ordinary Call nodes.
    //
    // The desugaring for a schema named `foo` with two fields looks like:
    //
    //   foo = validator.createWithName("foo")
    //   foo = validator.field(foo, "name")
    //   foo = validator.required(foo, 1)
    //   foo = validator.minLength(foo, 1)
    //   foo = validator.maxLength(foo, 50)
    //   foo = validator.field(foo, "email")
    //   foo = validator.required(foo, 1)
    //   foo = validator.match(foo, "email")
    //   foo = validator.message(foo, "Please enter a valid email address")
    //   foo = validator.message(foo, "Please check this field")   // default
    //
    // `schemaName.check expr:` desugars to:
    //
    //   __result = validator.run(schemaName, expr)
    //   if validator.isOk(__result):
    //       value = validator.getValue(__result)
    //       <ok_branch>
    //   else:
    //       errors = validator.getErrors(__result)
    //       <error_branch>
    // =========================================================================

    /// Build a `name.setFlags(flags)` method call statement.
    /// Emitted after a list variable declaration when the type has a non-Default behavior.
    fn make_set_behavior_stmt(
        &self,
        name: &str,
        behavior: ListBehavior,
        loc: SourceLocation,
    ) -> HirStatement {
        let flags = behavior.to_flags();
        HirStatement::Expression {
            expression: HirExpression::MethodCall {
                receiver: Box::new(HirExpression::Variable {
                    name: name.to_string(),
                    location: loc.clone(),
                }),
                method: "setFlags".to_string(),
                arguments: vec![HirExpression::Literal {
                    value: Value::Integer(flags as i64),
                    location: loc.clone(),
                }],
                location: loc.clone(),
            },
            location: loc,
        }
    }

    /// Desugar a `validate name:` block into a list of HIR statements.
    fn desugar_validate_declaration(
        &mut self,
        schema: &crate::ast::ValidateBlock,
        loc: &SourceLocation,
    ) -> Result<Vec<HirStatement>, CompilerError> {
        let mut stmts: Vec<HirStatement> = Vec::new();
        let schema_name = schema.name.clone();

        // Helper: emit `schemaName = expr` as a VariableDeclaration (first time) or Assignment.
        // We use VariableDeclaration for the first statement and assignments for subsequent ones.
        // Since HIR doesn't distinguish "first use", we use VariableDeclaration with `is_mutable: true`
        // for the create call and Assignment for all subsequent calls that update the same variable.

        // --- Step 1: validator.createWithName(schema_name) ---
        let create_call = HirExpression::Call {
            function: "validator.createWithName".to_string(),
            arguments: vec![HirExpression::Literal {
                value: Value::String(schema_name.clone()),
                location: loc.clone(),
            }],
            location: loc.clone(),
        };
        stmts.push(HirStatement::VariableDeclaration {
            name: schema_name.clone(),
            var_type: HirType::Integer, // rules pointer is i32
            initializer: Some(create_call),
            is_mutable: true,
            location: loc.clone(),
        });

        // --- Step 2: for each field, emit field + constraint calls ---
        let assign_schema = |expr: HirExpression| -> HirStatement {
            HirStatement::Assignment {
                target: HirLValue::Variable {
                    name: schema_name.clone(),
                    location: loc.clone(),
                },
                value: expr,
                location: loc.clone(),
            }
        };

        for field in &schema.fields {
            // validator.field(schema, "fieldName")
            let field_call = HirExpression::Call {
                function: "validator.field".to_string(),
                arguments: vec![
                    HirExpression::Variable {
                        name: schema_name.clone(),
                        location: loc.clone(),
                    },
                    HirExpression::Literal {
                        value: Value::String(field.name.clone()),
                        location: loc.clone(),
                    },
                ],
                location: loc.clone(),
            };
            stmts.push(assign_schema(field_call));

            // validator.type(schema, "typeName") — set the field's expected type name
            let type_name = match field.field_type {
                crate::ast::ValidateFieldType::String => "string",
                crate::ast::ValidateFieldType::Integer => "integer",
                crate::ast::ValidateFieldType::Number => "number",
                crate::ast::ValidateFieldType::Boolean => "boolean",
            };
            let type_call = HirExpression::Call {
                function: "validator.type".to_string(),
                arguments: vec![
                    HirExpression::Variable {
                        name: schema_name.clone(),
                        location: loc.clone(),
                    },
                    HirExpression::Literal {
                        value: Value::String(type_name.to_string()),
                        location: loc.clone(),
                    },
                ],
                location: loc.clone(),
            };
            stmts.push(assign_schema(type_call));

            // Each constraint
            for constraint in &field.constraints {
                let constraint_call = match constraint {
                    ValidateConstraint::Required => HirExpression::Call {
                        function: "validator.required".to_string(),
                        arguments: vec![
                            HirExpression::Variable {
                                name: schema_name.clone(),
                                location: loc.clone(),
                            },
                            HirExpression::Literal {
                                value: Value::Integer(1),
                                location: loc.clone(),
                            },
                        ],
                        location: loc.clone(),
                    },
                    ValidateConstraint::Trim => HirExpression::Call {
                        function: "validator.trim".to_string(),
                        arguments: vec![HirExpression::Variable {
                            name: schema_name.clone(),
                            location: loc.clone(),
                        }],
                        location: loc.clone(),
                    },
                    ValidateConstraint::Length { min, max } => {
                        // Type check: length constraint arguments must be integer literals.
                        // spec/semantic-rules.md: `length` constraint bounds must be integer.
                        if let Expression::Literal(ref v) = **min {
                            if !matches!(
                                v,
                                Value::Integer(_)
                                    | Value::Integer8(_)
                                    | Value::Integer8u(_)
                                    | Value::Integer16(_)
                                    | Value::Integer16u(_)
                                    | Value::Integer32(_)
                                    | Value::Integer64(_)
                            ) {
                                return Err(CompilerError::validation_error(
                                    format!(
                                        "validate length constraint: min argument must be integer, found {}",
                                        v
                                    ),
                                    loc.clone(),
                                ));
                            }
                        }
                        if let Expression::Literal(ref v) = **max {
                            if !matches!(
                                v,
                                Value::Integer(_)
                                    | Value::Integer8(_)
                                    | Value::Integer8u(_)
                                    | Value::Integer16(_)
                                    | Value::Integer16u(_)
                                    | Value::Integer32(_)
                                    | Value::Integer64(_)
                            ) {
                                return Err(CompilerError::validation_error(
                                    format!(
                                        "validate length constraint: max argument must be integer, found {}",
                                        v
                                    ),
                                    loc.clone(),
                                ));
                            }
                        }
                        // Emit minLength and maxLength as two separate calls.
                        let min_hir = self.build_expression(min)?;
                        let max_hir = self.build_expression(max)?;
                        // First emit minLength
                        let min_call = HirExpression::Call {
                            function: "validator.minLength".to_string(),
                            arguments: vec![
                                HirExpression::Variable {
                                    name: schema_name.clone(),
                                    location: loc.clone(),
                                },
                                min_hir,
                            ],
                            location: loc.clone(),
                        };
                        stmts.push(assign_schema(min_call));
                        // Then emit maxLength (we'll emit via the regular path below)
                        HirExpression::Call {
                            function: "validator.maxLength".to_string(),
                            arguments: vec![
                                HirExpression::Variable {
                                    name: schema_name.clone(),
                                    location: loc.clone(),
                                },
                                max_hir,
                            ],
                            location: loc.clone(),
                        }
                    }
                    ValidateConstraint::Min(expr) => {
                        // Type check: min constraint argument must be number or integer.
                        // spec/semantic-rules.md: `min` constraint value must be numeric.
                        if let Expression::Literal(ref v) = **expr {
                            if !matches!(
                                v,
                                Value::Integer(_)
                                    | Value::Integer8(_)
                                    | Value::Integer8u(_)
                                    | Value::Integer16(_)
                                    | Value::Integer16u(_)
                                    | Value::Integer32(_)
                                    | Value::Integer64(_)
                                    | Value::Number(_)
                                    | Value::Number32(_)
                                    | Value::Number64(_)
                            ) {
                                return Err(CompilerError::validation_error(
                                    format!(
                                        "validate min constraint: argument must be number or integer, found {}",
                                        v
                                    ),
                                    loc.clone(),
                                ));
                            }
                        }
                        let hir_expr = self.build_expression(expr)?;
                        HirExpression::Call {
                            function: "validator.range".to_string(),
                            arguments: vec![
                                HirExpression::Variable {
                                    name: schema_name.clone(),
                                    location: loc.clone(),
                                },
                                hir_expr,
                                HirExpression::Literal {
                                    value: Value::Integer(i64::MAX),
                                    location: loc.clone(),
                                },
                            ],
                            location: loc.clone(),
                        }
                    }
                    ValidateConstraint::Max(expr) => {
                        // Type check: max constraint argument must be number or integer.
                        // spec/semantic-rules.md: `max` constraint value must be numeric.
                        if let Expression::Literal(ref v) = **expr {
                            if !matches!(
                                v,
                                Value::Integer(_)
                                    | Value::Integer8(_)
                                    | Value::Integer8u(_)
                                    | Value::Integer16(_)
                                    | Value::Integer16u(_)
                                    | Value::Integer32(_)
                                    | Value::Integer64(_)
                                    | Value::Number(_)
                                    | Value::Number32(_)
                                    | Value::Number64(_)
                            ) {
                                return Err(CompilerError::validation_error(
                                    format!(
                                        "validate max constraint: argument must be number or integer, found {}",
                                        v
                                    ),
                                    loc.clone(),
                                ));
                            }
                        }
                        let hir_expr = self.build_expression(expr)?;
                        HirExpression::Call {
                            function: "validator.range".to_string(),
                            arguments: vec![
                                HirExpression::Variable {
                                    name: schema_name.clone(),
                                    location: loc.clone(),
                                },
                                HirExpression::Literal {
                                    value: Value::Integer(i64::MIN),
                                    location: loc.clone(),
                                },
                                hir_expr,
                            ],
                            location: loc.clone(),
                        }
                    }
                    ValidateConstraint::Match(pattern) => {
                        // SEM010 (validate context): match constraint argument must be a
                        // known pattern name literal.
                        // spec/semantic-rules.md §SEM010
                        const VALID_PATTERNS: &[&str] = &[
                            "email",
                            "url",
                            "uuid",
                            "phone",
                            "date",
                            "integer",
                            "number",
                            "alphanumeric",
                        ];
                        if !VALID_PATTERNS.contains(&pattern.as_str()) {
                            return Err(CompilerError::validation_error(
                                format!(
                                    "Unknown pattern name '{}' in validate match constraint. Valid patterns: email, url, uuid, phone, date, integer, number, alphanumeric",
                                    pattern
                                ),
                                loc.clone(),
                            ));
                        }
                        HirExpression::Call {
                            function: "validator.match".to_string(),
                            arguments: vec![
                                HirExpression::Variable {
                                    name: schema_name.clone(),
                                    location: loc.clone(),
                                },
                                HirExpression::Literal {
                                    value: Value::String(pattern.clone()),
                                    location: loc.clone(),
                                },
                            ],
                            location: loc.clone(),
                        }
                    }
                    ValidateConstraint::OneOf(values) => {
                        // Build a list literal expression containing the oneOf values
                        let hir_values: Result<Vec<HirExpression>, CompilerError> =
                            values.iter().map(|v| self.build_expression(v)).collect();
                        let hir_values = hir_values?;
                        let list_expr = HirExpression::Array {
                            elements: hir_values,
                            element_type: HirType::String, // oneOf values are strings or integers; treat as Any
                            location: loc.clone(),
                        };
                        HirExpression::Call {
                            function: "validator.allowedValues".to_string(),
                            arguments: vec![
                                HirExpression::Variable {
                                    name: schema_name.clone(),
                                    location: loc.clone(),
                                },
                                list_expr,
                            ],
                            location: loc.clone(),
                        }
                    }
                    ValidateConstraint::Custom(fn_name) => HirExpression::Call {
                        function: "validator.custom".to_string(),
                        arguments: vec![
                            HirExpression::Variable {
                                name: schema_name.clone(),
                                location: loc.clone(),
                            },
                            HirExpression::Variable {
                                name: fn_name.clone(),
                                location: loc.clone(),
                            },
                        ],
                        location: loc.clone(),
                    },
                };
                stmts.push(assign_schema(constraint_call));
            }
        }

        // --- Step 3: messages ---
        if let Some(messages) = &schema.messages {
            // Emit per-field message overrides first
            for (field_name, msg_text) in &messages.field_messages {
                // To set a per-field message we need to:
                // 1. Re-select the field: validator.field(schema, fieldName) updates the "current field" pointer
                // 2. Call validator.message(schema, "text")
                let re_field_call = HirExpression::Call {
                    function: "validator.field".to_string(),
                    arguments: vec![
                        HirExpression::Variable {
                            name: schema_name.clone(),
                            location: loc.clone(),
                        },
                        HirExpression::Literal {
                            value: Value::String(field_name.clone()),
                            location: loc.clone(),
                        },
                    ],
                    location: loc.clone(),
                };
                stmts.push(assign_schema(re_field_call));

                let msg_call = HirExpression::Call {
                    function: "validator.message".to_string(),
                    arguments: vec![
                        HirExpression::Variable {
                            name: schema_name.clone(),
                            location: loc.clone(),
                        },
                        HirExpression::Literal {
                            value: Value::String(msg_text.clone()),
                            location: loc.clone(),
                        },
                    ],
                    location: loc.clone(),
                };
                stmts.push(assign_schema(msg_call));
            }

            // Emit default message if present.
            // `validator.message` on the schema level (without a preceding `validator.field`)
            // is treated as the default. We use `validator.createWithName` already set the name,
            // so we call `validator.message` with a special "default" field sentinel handled
            // by the runtime, or we use `validator.field("__default__")` + `validator.message`.
            // The validator runtime's `message` call applies the message to the *last active field*
            // in the current call chain. There is no dedicated "set default" call in the runtime.
            // We emit `validator.field(schema, "__default__") + validator.message(schema, text)`
            // and the runtime silently ignores the unknown "__default__" field name when setting
            // the message, storing it as the schema-level default.
            // Alternatively, we add a dedicated `validator.defaultMessage` call.
            // Looking at the runtime more carefully (generate_validator_message):
            // the runtime stores message per-field in the field_entry. For a true default we
            // need to store it separately. We'll use the approach of adding a field entry named
            // "" (empty string) which the runtime won't find for any real field, and rely on
            // the runtime's fallback logic.
            // The simplest correct approach: just call validator.message with the schema ptr
            // directly — the last field in the schema will receive it. Instead, we emit
            // a dedicated "default" message by calling validator.field("") then validator.message.
            if let Some(default_msg) = &messages.default_message {
                // Use an empty field name as sentinel for the default message.
                // The validator runtime stores this per-field; if none of the real fields
                // have a message set, the runtime returns this as the fallback.
                let default_field_call = HirExpression::Call {
                    function: "validator.field".to_string(),
                    arguments: vec![
                        HirExpression::Variable {
                            name: schema_name.clone(),
                            location: loc.clone(),
                        },
                        HirExpression::Literal {
                            value: Value::String(String::new()),
                            location: loc.clone(),
                        },
                    ],
                    location: loc.clone(),
                };
                stmts.push(assign_schema(default_field_call));

                let default_msg_call = HirExpression::Call {
                    function: "validator.message".to_string(),
                    arguments: vec![
                        HirExpression::Variable {
                            name: schema_name.clone(),
                            location: loc.clone(),
                        },
                        HirExpression::Literal {
                            value: Value::String(default_msg.clone()),
                            location: loc.clone(),
                        },
                    ],
                    location: loc.clone(),
                };
                stmts.push(assign_schema(default_msg_call));
            }
        }

        Ok(stmts)
    }

    /// Desugar a `schemaName.check expr:` block into a list of HIR statements.
    fn desugar_validate_check(
        &mut self,
        check: &crate::ast::ValidateCheckBlock,
        loc: &SourceLocation,
    ) -> Result<Vec<HirStatement>, CompilerError> {
        let schema_name = &check.schema_name;

        // Internal temporary name for the ValidationResult pointer.
        // Use a mangled name to avoid shadowing user variables.
        let result_var = format!("__validate_result_{}__", schema_name);

        // 1. __validate_result_foo__ = validator.run(foo, inputExpr)
        let input_hir = self.build_expression(&check.input)?;
        let run_call = HirExpression::Call {
            function: "validator.run".to_string(),
            arguments: vec![
                HirExpression::Variable {
                    name: schema_name.clone(),
                    location: loc.clone(),
                },
                input_hir,
            ],
            location: loc.clone(),
        };
        let result_decl = HirStatement::VariableDeclaration {
            name: result_var.clone(),
            var_type: HirType::Integer, // ValidationResult pointer is i32
            initializer: Some(run_call),
            is_mutable: false,
            location: loc.clone(),
        };

        // 2. if validator.isOk(__validate_result_foo__):
        //        value = validator.getValue(__validate_result_foo__)
        //        <ok_branch>
        //    else:
        //        errors = validator.getErrors(__validate_result_foo__)
        //        <error_branch>

        // is_ok condition
        let is_ok_cond = HirExpression::Call {
            function: "validator.isOk".to_string(),
            arguments: vec![HirExpression::Variable {
                name: result_var.clone(),
                location: loc.clone(),
            }],
            location: loc.clone(),
        };

        // ok branch: bind `value` then run ok statements
        let get_value_call = HirExpression::Call {
            function: "validator.getValue".to_string(),
            arguments: vec![HirExpression::Variable {
                name: result_var.clone(),
                location: loc.clone(),
            }],
            location: loc.clone(),
        };
        let value_decl = HirStatement::VariableDeclaration {
            name: "value".to_string(),
            var_type: HirType::Any,
            initializer: Some(get_value_call),
            is_mutable: false,
            location: loc.clone(),
        };
        let mut ok_hir_stmts = vec![value_decl];
        for stmt in &check.ok_branch {
            ok_hir_stmts.push(self.build_statement(stmt)?);
        }
        let ok_block = HirBlock {
            statements: ok_hir_stmts,
            location: loc.clone(),
        };

        // error branch: bind `errors` then run error statements
        let get_errors_call = HirExpression::Call {
            function: "validator.getErrors".to_string(),
            arguments: vec![HirExpression::Variable {
                name: result_var.clone(),
                location: loc.clone(),
            }],
            location: loc.clone(),
        };
        let errors_decl = HirStatement::VariableDeclaration {
            name: "errors".to_string(),
            var_type: HirType::List(Box::new(HirType::String)),
            initializer: Some(get_errors_call),
            is_mutable: false,
            location: loc.clone(),
        };
        let mut error_hir_stmts = vec![errors_decl];
        for stmt in &check.error_branch {
            error_hir_stmts.push(self.build_statement(stmt)?);
        }
        let error_block = HirBlock {
            statements: error_hir_stmts,
            location: loc.clone(),
        };

        let if_stmt = HirStatement::If {
            condition: is_ok_cond,
            then_branch: ok_block,
            else_branch: Some(error_block),
            location: loc.clone(),
        };

        Ok(vec![result_decl, if_stmt])
    }
}

impl Default for HirBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HirBuilder {
    /// Build a HirTest body for an endpoint test case.
    ///
    /// Generates:
    ///   integer __handle = _test_http_request_clean(method, path, body, "", "", "")
    ///   boolean __ok = true
    ///   for each assertion:
    ///     Status   → __ok = __ok && (_test_response_status(__handle) op value)
    ///     JsonField → __ok = __ok && (json.get(json.decode(_test_response_body(__handle)), path) op value)
    ///   return __ok
    pub(crate) fn build_endpoint_test_body(
        &mut self,
        endpoint: &crate::ast::EndpointTest,
        loc: SourceLocation,
    ) -> Result<HirBlock, CompilerError> {
        use crate::ast::{HttpComparisonOp, HttpMethod, HttpTestAssertion};

        let empty_str = || HirExpression::Literal {
            value: Value::String(String::new()),
            location: loc.clone(),
        };

        let method_str = match endpoint.request.method {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
        };

        // _test_http_request_clean(method, path, body, "", "", "")
        // body: use empty string for now (body key/value pairs require serialization)
        let handle_call = HirExpression::Call {
            function: "_test_http_request_clean".to_string(),
            arguments: vec![
                HirExpression::Literal {
                    value: Value::String(method_str.to_string()),
                    location: loc.clone(),
                },
                HirExpression::Literal {
                    value: Value::String(endpoint.request.path.clone()),
                    location: loc.clone(),
                },
                empty_str(), // body
                empty_str(), // header key
                empty_str(), // header value
            ],
            location: loc.clone(),
        };

        let mut stmts: Vec<HirStatement> = vec![
            HirStatement::VariableDeclaration {
                name: "__handle".to_string(),
                var_type: HirType::Integer,
                initializer: Some(handle_call),
                is_mutable: false,
                location: loc.clone(),
            },
            HirStatement::VariableDeclaration {
                name: "__ok".to_string(),
                var_type: HirType::Boolean,
                initializer: Some(HirExpression::Literal {
                    value: Value::Boolean(true),
                    location: loc.clone(),
                }),
                is_mutable: true,
                location: loc.clone(),
            },
        ];

        let hir_op = |op: &HttpComparisonOp| match op {
            HttpComparisonOp::Equal => HirBinaryOp::Equal,
            HttpComparisonOp::NotEqual => HirBinaryOp::NotEqual,
            HttpComparisonOp::Less => HirBinaryOp::Less,
            HttpComparisonOp::Greater => HirBinaryOp::Greater,
            HttpComparisonOp::LessEqual => HirBinaryOp::LessEqual,
            HttpComparisonOp::GreaterEqual => HirBinaryOp::GreaterEqual,
        };

        for assertion in &endpoint.assertions {
            let check = match assertion {
                HttpTestAssertion::Status { op, value } => HirExpression::BinaryOp {
                    left: Box::new(HirExpression::Call {
                        function: "_test_response_status".to_string(),
                        arguments: vec![HirExpression::Variable {
                            name: "__handle".to_string(),
                            location: loc.clone(),
                        }],
                        location: loc.clone(),
                    }),
                    op: hir_op(op),
                    right: Box::new(HirExpression::Literal {
                        value: Value::Integer(*value),
                        location: loc.clone(),
                    }),
                    location: loc.clone(),
                },
                HttpTestAssertion::JsonField { path, op, value } => {
                    let body_expr = HirExpression::Call {
                        function: "_test_response_body".to_string(),
                        arguments: vec![HirExpression::Variable {
                            name: "__handle".to_string(),
                            location: loc.clone(),
                        }],
                        location: loc.clone(),
                    };
                    let decoded = HirExpression::Call {
                        function: "json.decode".to_string(),
                        arguments: vec![body_expr],
                        location: loc.clone(),
                    };
                    // Chain json.get calls for each path segment
                    let field_expr = path.iter().fold(decoded, |acc, seg| HirExpression::Call {
                        function: "json.get".to_string(),
                        arguments: vec![
                            acc,
                            HirExpression::Literal {
                                value: Value::String(seg.clone()),
                                location: loc.clone(),
                            },
                        ],
                        location: loc.clone(),
                    });
                    let rhs = self.build_expression(value)?;
                    HirExpression::BinaryOp {
                        left: Box::new(field_expr),
                        op: hir_op(op),
                        right: Box::new(rhs),
                        location: loc.clone(),
                    }
                }
                HttpTestAssertion::JsonFieldNotNull { path } => {
                    let body_expr = HirExpression::Call {
                        function: "_test_response_body".to_string(),
                        arguments: vec![HirExpression::Variable {
                            name: "__handle".to_string(),
                            location: loc.clone(),
                        }],
                        location: loc.clone(),
                    };
                    let decoded = HirExpression::Call {
                        function: "json.decode".to_string(),
                        arguments: vec![body_expr],
                        location: loc.clone(),
                    };
                    let field_expr = path.iter().fold(decoded, |acc, seg| HirExpression::Call {
                        function: "json.get".to_string(),
                        arguments: vec![
                            acc,
                            HirExpression::Literal {
                                value: Value::String(seg.clone()),
                                location: loc.clone(),
                            },
                        ],
                        location: loc.clone(),
                    });
                    HirExpression::BinaryOp {
                        left: Box::new(field_expr),
                        op: HirBinaryOp::NotEqual,
                        right: Box::new(HirExpression::Literal {
                            value: Value::None,
                            location: loc.clone(),
                        }),
                        location: loc.clone(),
                    }
                }
            };

            // __ok = __ok && check
            stmts.push(HirStatement::Assignment {
                target: HirLValue::Variable {
                    name: "__ok".to_string(),
                    location: loc.clone(),
                },
                value: HirExpression::BinaryOp {
                    left: Box::new(HirExpression::Variable {
                        name: "__ok".to_string(),
                        location: loc.clone(),
                    }),
                    op: HirBinaryOp::And,
                    right: Box::new(check),
                    location: loc.clone(),
                },
                location: loc.clone(),
            });
        }

        stmts.push(HirStatement::Return {
            value: Some(HirExpression::Variable {
                name: "__ok".to_string(),
                location: loc.clone(),
            }),
            location: loc.clone(),
        });

        Ok(HirBlock {
            statements: stmts,
            location: loc,
        })
    }
}
