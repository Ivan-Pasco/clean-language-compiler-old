use crate::ast::{AssignmentTarget, Expression, Function, Program, Statement};
use crate::error::CompilerError;
use std::collections::HashSet;

/// Dead code elimination optimizer
///
/// Removes:
/// - Unused functions
/// - Unused variables
/// - Unreachable code
/// - Unused imports
/// - Dead stores
pub struct DeadCodeEliminator {
    debug: bool,
}

/// Results from dead code elimination
#[derive(Debug, Default)]
pub struct DCEResults {
    pub functions_removed: usize,
    pub variables_removed: usize,
    pub statements_removed: usize,
    pub unreachable_code_removed: usize,
}

impl DeadCodeEliminator {
    pub fn new(debug: bool) -> Self {
        Self { debug }
    }

    /// Eliminate dead code from the entire program
    pub fn eliminate(&mut self, program: &mut Program) -> Result<DCEResults, CompilerError> {
        let mut results = DCEResults::default();

        // Phase 1: Find all used functions
        let used_functions = self.find_used_functions(program);

        // Phase 2: Remove unused functions
        let original_func_count = program.functions.len();
        let mut functions_to_remove = Vec::new();

        for (i, func) in program.functions.iter().enumerate() {
            let is_used = used_functions.contains(&func.name)
                || func.name == "start"
                || self.is_exported_function(&func.name, program);

            if !is_used {
                if self.debug {
                    println!("DCE: Removing unused function '{}'", func.name);
                }
                functions_to_remove.push(i);
            }
        }

        // Remove functions in reverse order to maintain indices
        for &index in functions_to_remove.iter().rev() {
            program.functions.remove(index);
        }
        results.functions_removed = original_func_count - program.functions.len();

        // Phase 3: Remove dead code within functions
        for function in &mut program.functions {
            let func_results = self.eliminate_in_function(function)?;
            results.variables_removed += func_results.variables_removed;
            results.statements_removed += func_results.statements_removed;
            results.unreachable_code_removed += func_results.unreachable_code_removed;
        }

        if self.debug {
            println!("DCE Results: {} functions, {} variables, {} statements, {} unreachable blocks removed",
                     results.functions_removed, results.variables_removed,
                     results.statements_removed, results.unreachable_code_removed);
        }

        Ok(results)
    }

    /// Find all functions that are used (directly or indirectly)
    fn find_used_functions(&self, program: &Program) -> HashSet<String> {
        let mut used = HashSet::new();
        let mut to_visit = Vec::new();

        // Start with entry points
        if let Some(start_func) = &program.start_function {
            to_visit.push(start_func.name.clone());
        }

        // Add exported functions
        for func in &program.functions {
            if self.is_exported_function(&func.name, program) {
                to_visit.push(func.name.clone());
            }
        }

        // Transitively find all used functions
        while let Some(func_name) = to_visit.pop() {
            if used.insert(func_name.clone()) {
                // Find the function and analyze its body
                if let Some(function) = program.functions.iter().find(|f| f.name == func_name) {
                    let called_functions = self.find_called_functions_in_function(function);
                    to_visit.extend(called_functions);
                }
            }
        }

        used
    }

    /// Find all functions called within a function
    fn find_called_functions_in_function(&self, function: &Function) -> Vec<String> {
        let mut called = Vec::new();

        for statement in &function.body {
            self.find_called_functions_in_statement(statement, &mut called);
        }

        called
    }

    /// Find functions called in a statement
    fn find_called_functions_in_statement(&self, statement: &Statement, called: &mut Vec<String>) {
        match statement {
            Statement::Expression { expr, .. } => {
                self.find_called_functions_in_expression(expr, called);
            }
            Statement::VariableDecl {
                initializer: Some(expr),
                ..
            } => {
                self.find_called_functions_in_expression(expr, called);
            }
            Statement::Assignment { value, .. } => {
                self.find_called_functions_in_expression(value, called);
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.find_called_functions_in_expression(condition, called);
                for stmt in then_branch {
                    self.find_called_functions_in_statement(stmt, called);
                }
                if let Some(else_branch) = else_branch {
                    for stmt in else_branch {
                        self.find_called_functions_in_statement(stmt, called);
                    }
                }
            }
            Statement::Iterate {
                collection, body, ..
            } => {
                self.find_called_functions_in_expression(collection, called);
                for stmt in body {
                    self.find_called_functions_in_statement(stmt, called);
                }
            }
            Statement::RangeIterate {
                start,
                end,
                step,
                body,
                ..
            } => {
                self.find_called_functions_in_expression(start, called);
                self.find_called_functions_in_expression(end, called);
                if let Some(step) = step {
                    self.find_called_functions_in_expression(step, called);
                }
                for stmt in body {
                    self.find_called_functions_in_statement(stmt, called);
                }
            }
            Statement::Return {
                value: Some(expr), ..
            } => {
                self.find_called_functions_in_expression(expr, called);
            }
            Statement::FunctionsBlock { functions, .. } => {
                for func in functions {
                    for stmt in &func.body {
                        self.find_called_functions_in_statement(stmt, called);
                    }
                }
            }
            _ => {}
        }
    }

    /// Find functions called in an expression
    fn find_called_functions_in_expression(
        &self,
        expression: &Expression,
        called: &mut Vec<String>,
    ) {
        match expression {
            Expression::Call(function, arguments) => {
                called.push(function.clone());
                // Also check arguments
                for arg in arguments {
                    self.find_called_functions_in_expression(arg, called);
                }
            }
            Expression::Binary(left, _, right) => {
                self.find_called_functions_in_expression(left, called);
                self.find_called_functions_in_expression(right, called);
            }
            Expression::Unary(_, operand) => {
                self.find_called_functions_in_expression(operand, called);
            }
            Expression::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                self.find_called_functions_in_expression(condition, called);
                self.find_called_functions_in_expression(then_expr, called);
                self.find_called_functions_in_expression(else_expr, called);
            }
            // Arrays are now represented as List literals in Value
            Expression::Literal(_) => {
                // Literals contain no function calls
            }
            Expression::ListAccess(array, index) => {
                self.find_called_functions_in_expression(array, called);
                self.find_called_functions_in_expression(index, called);
            }
            Expression::MatrixAccess(matrix, row, col) => {
                self.find_called_functions_in_expression(matrix, called);
                self.find_called_functions_in_expression(row, called);
                self.find_called_functions_in_expression(col, called);
            }
            Expression::PropertyAccess { object, .. } => {
                self.find_called_functions_in_expression(object, called);
            }
            Expression::MethodCall {
                object, arguments, ..
            } => {
                self.find_called_functions_in_expression(object, called);
                for arg in arguments {
                    self.find_called_functions_in_expression(arg, called);
                }
            }
            _ => {}
        }
    }

    /// Check if a function is exported
    fn is_exported_function(&self, name: &str, _program: &Program) -> bool {
        // For now, assume main functions and certain patterns are exported
        name == "main" || name.starts_with("export_") || name.starts_with("_")
    }

    /// Eliminate dead code within a function
    fn eliminate_in_function(
        &mut self,
        function: &mut Function,
    ) -> Result<DCEResults, CompilerError> {
        let mut results = DCEResults::default();

        // Find used variables
        let used_vars = self.find_used_variables(&function.body);

        // Remove unused variables (this would need more sophisticated analysis)
        let original_param_count = function.parameters.len();
        function
            .parameters
            .retain(|param| used_vars.contains(&param.name));
        results.variables_removed += original_param_count - function.parameters.len();

        // Remove unreachable code
        let original_stmt_count = function.body.len();
        self.remove_unreachable_code(&mut function.body);
        results.statements_removed += original_stmt_count - function.body.len();

        // Remove dead stores
        self.remove_dead_stores(&mut function.body);

        Ok(results)
    }

    /// Find all variables that are used in the function body
    fn find_used_variables(&self, statements: &[Statement]) -> HashSet<String> {
        let mut used = HashSet::new();

        for statement in statements {
            self.find_used_variables_in_statement(statement, &mut used);
        }

        used
    }

    /// Find used variables in a statement
    fn find_used_variables_in_statement(&self, statement: &Statement, used: &mut HashSet<String>) {
        match statement {
            Statement::Expression { expr, .. } => {
                self.find_used_variables_in_expression(expr, used);
            }
            Statement::VariableDecl {
                initializer: Some(expr),
                ..
            } => {
                self.find_used_variables_in_expression(expr, used);
            }
            Statement::Assignment { value, .. } => {
                self.find_used_variables_in_expression(value, used);
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.find_used_variables_in_expression(condition, used);
                for stmt in then_branch {
                    self.find_used_variables_in_statement(stmt, used);
                }
                if let Some(else_branch) = else_branch {
                    for stmt in else_branch {
                        self.find_used_variables_in_statement(stmt, used);
                    }
                }
            }
            Statement::Iterate {
                collection, body, ..
            } => {
                self.find_used_variables_in_expression(collection, used);
                for stmt in body {
                    self.find_used_variables_in_statement(stmt, used);
                }
            }
            Statement::RangeIterate {
                start,
                end,
                step,
                body,
                ..
            } => {
                self.find_used_variables_in_expression(start, used);
                self.find_used_variables_in_expression(end, used);
                if let Some(step) = step {
                    self.find_used_variables_in_expression(step, used);
                }
                for stmt in body {
                    self.find_used_variables_in_statement(stmt, used);
                }
            }
            Statement::Return {
                value: Some(expr), ..
            } => {
                self.find_used_variables_in_expression(expr, used);
            }
            Statement::FunctionsBlock { functions, .. } => {
                for func in functions {
                    for stmt in &func.body {
                        self.find_used_variables_in_statement(stmt, used);
                    }
                }
            }
            _ => {}
        }
    }

    /// Find used variables in an expression
    fn find_used_variables_in_expression(
        &self,
        expression: &Expression,
        used: &mut HashSet<String>,
    ) {
        match expression {
            Expression::Variable(name) => {
                used.insert(name.clone());
            }
            Expression::Binary(left, _, right) => {
                self.find_used_variables_in_expression(left, used);
                self.find_used_variables_in_expression(right, used);
            }
            Expression::Unary(_, operand) => {
                self.find_used_variables_in_expression(operand, used);
            }
            Expression::Call(_, arguments) => {
                for arg in arguments {
                    self.find_used_variables_in_expression(arg, used);
                }
            }
            Expression::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                self.find_used_variables_in_expression(condition, used);
                self.find_used_variables_in_expression(then_expr, used);
                self.find_used_variables_in_expression(else_expr, used);
            }
            Expression::Literal(_) => {
                // Literals don't use variables
            }
            Expression::ListAccess(array, index) => {
                self.find_used_variables_in_expression(array, used);
                self.find_used_variables_in_expression(index, used);
            }
            Expression::MatrixAccess(matrix, row, col) => {
                self.find_used_variables_in_expression(matrix, used);
                self.find_used_variables_in_expression(row, used);
                self.find_used_variables_in_expression(col, used);
            }
            Expression::PropertyAccess { object, .. } => {
                self.find_used_variables_in_expression(object, used);
            }
            Expression::MethodCall {
                object, arguments, ..
            } => {
                self.find_used_variables_in_expression(object, used);
                for arg in arguments {
                    self.find_used_variables_in_expression(arg, used);
                }
            }
            _ => {}
        }
    }

    /// Remove unreachable code after return statements
    fn remove_unreachable_code(&mut self, statements: &mut Vec<Statement>) {
        let mut i = 0;
        while i < statements.len() {
            match &statements[i] {
                Statement::Return { .. } => {
                    // Remove all statements after return
                    let removed = statements.len() - i - 1;
                    statements.truncate(i + 1);
                    if self.debug && removed > 0 {
                        println!(
                            "DCE: Removed {} unreachable statements after return",
                            removed
                        );
                    }
                    break;
                }
                Statement::FunctionsBlock { .. } => {
                    // Function blocks are processed separately
                }
                Statement::If { .. } => {
                    // Process if/else branches
                    if let Statement::If {
                        then_branch,
                        else_branch,
                        ..
                    } = &mut statements[i]
                    {
                        self.remove_unreachable_code(then_branch);
                        if let Some(else_branch) = else_branch {
                            self.remove_unreachable_code(else_branch);
                        }
                    }
                }
                Statement::Iterate { .. } | Statement::RangeIterate { .. } => {
                    // Process iteration body
                    match &mut statements[i] {
                        Statement::Iterate { body, .. } | Statement::RangeIterate { body, .. } => {
                            self.remove_unreachable_code(body);
                        }
                        _ => unreachable!(),
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Remove assignments to variables that are never read
    fn remove_dead_stores(&mut self, statements: &mut Vec<Statement>) {
        // This is a simplified implementation
        // A full implementation would need more sophisticated dataflow analysis
        let mut assignments_to_remove = Vec::new();

        for (i, statement) in statements.iter().enumerate() {
            if let Statement::Assignment { target, .. } = statement {
                // Only simple variable assignments can be dead-code eliminated.
                // Index and property assignments always have observable side effects.
                let target_name = match target {
                    AssignmentTarget::Variable(name) => name.as_str(),
                    _ => continue,
                };
                // Check if this variable is used after this assignment
                let mut used_after = false;
                for later_stmt in &statements[i + 1..] {
                    let mut used_vars = HashSet::new();
                    self.find_used_variables_in_statement(later_stmt, &mut used_vars);
                    if used_vars.contains(target_name) {
                        used_after = true;
                        break;
                    }
                }

                if !used_after && self.debug {
                    println!(
                        "DCE: Dead store to variable '{}' at position {}",
                        target_name, i
                    );
                    assignments_to_remove.push(i);
                }
            }
        }

        // Remove dead stores in reverse order to maintain indices
        for &index in assignments_to_remove.iter().rev() {
            statements.remove(index);
        }
    }
}

// Dead code elimination is validated end-to-end via the integration test suite.
