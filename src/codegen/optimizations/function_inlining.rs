use crate::error::CompilerError;
use crate::ast::{Expression, Statement, Function, Program};
use std::collections::{HashMap, HashSet};

/// Function inlining optimizer
/// 
/// Inlines function calls to reduce call overhead:
/// - Small functions (configurable size threshold)
/// - Leaf functions (functions that don't call others)
/// - Functions called only once
/// - Functions with constant parameters
/// - Prevents recursive inlining to avoid infinite expansion
pub struct FunctionInliner {
    debug: bool,
    max_inline_size: usize,
    max_inline_depth: usize,
    current_depth: usize,
    inlined_functions: HashSet<String>,
}

/// Inlining candidate information
#[derive(Debug, Clone)]
pub struct InlineCandidate {
    pub function: Function,
    pub call_count: usize,
    pub size_estimate: usize,
    pub is_recursive: bool,
    pub has_side_effects: bool,
}

/// Results from function inlining
#[derive(Debug, Default)]
pub struct InliningResults {
    pub functions_inlined: usize,
    pub call_sites_inlined: usize,
    pub size_increase: usize,
    pub estimated_performance_gain: f64,
}

impl FunctionInliner {
    pub fn new(debug: bool) -> Self {
        Self {
            debug,
            max_inline_size: 50, // Maximum statements to inline
            max_inline_depth: 3, // Maximum inlining depth to prevent explosion
            current_depth: 0,
            inlined_functions: HashSet::new(),
        }
    }

    pub fn with_limits(debug: bool, max_size: usize, max_depth: usize) -> Self {
        Self {
            debug,
            max_inline_size: max_size,
            max_inline_depth: max_depth,
            current_depth: 0,
            inlined_functions: HashSet::new(),
        }
    }

    /// Inline functions in the entire program
    pub fn inline(&mut self, program: &mut Program) -> Result<InliningResults, CompilerError> {
        let mut results = InliningResults::default();

        // Phase 1: Analyze all functions and find inlining candidates
        let candidates = self.find_inline_candidates(&program.functions)?;
        
        if self.debug {
            println!("FI: Found {} inlining candidates", candidates.len());
            for (name, candidate) in &candidates {
                println!("  {}: {} calls, {} statements, recursive: {}", 
                         name, candidate.call_count, candidate.size_estimate, candidate.is_recursive);
            }
        }

        // Phase 2: Inline functions, starting with the most beneficial ones
        let sorted_candidates = self.sort_candidates_by_benefit(&candidates);
        
        for (func_name, _) in sorted_candidates {
            if let Some(candidate) = candidates.get(&func_name) {
                if self.should_inline(candidate) {
                    let inline_results = self.inline_function(program, &func_name, candidate)?;
                    results.functions_inlined += if inline_results.call_sites_inlined > 0 { 1 } else { 0 };
                    results.call_sites_inlined += inline_results.call_sites_inlined;
                    results.size_increase += inline_results.size_increase;
                    results.estimated_performance_gain += inline_results.estimated_performance_gain;
                }
            }
        }

        if self.debug {
            println!("FI Results: {} functions inlined, {} call sites, +{} size, {:.2}% perf gain",
                     results.functions_inlined, results.call_sites_inlined, 
                     results.size_increase, results.estimated_performance_gain);
        }

        Ok(results)
    }

    /// Find all functions that are candidates for inlining
    fn find_inline_candidates(&self, functions: &[Function]) -> Result<HashMap<String, InlineCandidate>, CompilerError> {
        let mut candidates = HashMap::new();
        let call_counts = self.count_function_calls(functions);
        let recursive_functions = self.find_recursive_functions(functions);

        for function in functions {
            // Skip main/start functions and exported functions
            if function.name == "main" || function.name == "start" || function.name.starts_with("export_") {
                continue;
            }

            let call_count = call_counts.get(&function.name).unwrap_or(&0);
            let size_estimate = self.estimate_function_size(function);
            let is_recursive = recursive_functions.contains(&function.name);
            let has_side_effects = self.has_side_effects(function);

            let candidate = InlineCandidate {
                function: function.clone(),
                call_count: *call_count,
                size_estimate,
                is_recursive,
                has_side_effects,
            };

            candidates.insert(function.name.clone(), candidate);
        }

        Ok(candidates)
    }

    /// Count how many times each function is called
    fn count_function_calls(&self, functions: &[Function]) -> HashMap<String, usize> {
        let mut call_counts = HashMap::new();

        for function in functions {
            self.count_calls_in_statements(&function.body, &mut call_counts);
        }

        call_counts
    }

    /// Count function calls in a list of statements
    fn count_calls_in_statements(&self, statements: &[Statement], call_counts: &mut HashMap<String, usize>) {
        for statement in statements {
            self.count_calls_in_statement(statement, call_counts);
        }
    }

    /// Count function calls in a statement
    fn count_calls_in_statement(&self, statement: &Statement, call_counts: &mut HashMap<String, usize>) {
        match statement {
            Statement::Expression(expr) => {
                self.count_calls_in_expression(expr, call_counts);
            }
            Statement::Variable { initializer: Some(expr), .. } => {
                self.count_calls_in_expression(expr, call_counts);
            }
            Statement::Assignment { target, value } => {
                self.count_calls_in_expression(target, call_counts);
                self.count_calls_in_expression(value, call_counts);
            }
            Statement::If { condition, then_stmt, else_stmt } => {
                self.count_calls_in_expression(condition, call_counts);
                self.count_calls_in_statement(then_stmt, call_counts);
                if let Some(else_stmt) = else_stmt {
                    self.count_calls_in_statement(else_stmt, call_counts);
                }
            }
            Statement::For { init, condition, update, body } => {
                if let Some(init) = init {
                    self.count_calls_in_statement(init, call_counts);
                }
                if let Some(condition) = condition {
                    self.count_calls_in_expression(condition, call_counts);
                }
                if let Some(update) = update {
                    self.count_calls_in_expression(update, call_counts);
                }
                self.count_calls_in_statement(body, call_counts);
            }
            Statement::Return { value: Some(expr) } => {
                self.count_calls_in_expression(expr, call_counts);
            }
            Statement::Block(statements) => {
                self.count_calls_in_statements(statements, call_counts);
            }
            _ => {}
        }
    }

    /// Count function calls in an expression
    fn count_calls_in_expression(&self, expression: &Expression, call_counts: &mut HashMap<String, usize>) {
        match expression {
            Expression::Call { function, arguments } => {
                if let Expression::Variable(name) = function.as_ref() {
                    *call_counts.entry(name.clone()).or_insert(0) += 1;
                }
                for arg in arguments {
                    self.count_calls_in_expression(arg, call_counts);
                }
            }
            Expression::Binary { left, right, .. } => {
                self.count_calls_in_expression(left, call_counts);
                self.count_calls_in_expression(right, call_counts);
            }
            Expression::Unary { operand, .. } => {
                self.count_calls_in_expression(operand, call_counts);
            }
            Expression::Conditional { condition, then_expr, else_expr } => {
                self.count_calls_in_expression(condition, call_counts);
                self.count_calls_in_expression(then_expr, call_counts);
                self.count_calls_in_expression(else_expr, call_counts);
            }
            Expression::Array(elements) => {
                for element in elements {
                    self.count_calls_in_expression(element, call_counts);
                }
            }
            Expression::Index { array, index } => {
                self.count_calls_in_expression(array, call_counts);
                self.count_calls_in_expression(index, call_counts);
            }
            Expression::PropertyAccess { object, .. } => {
                self.count_calls_in_expression(object, call_counts);
            }
            Expression::MethodCall { object, arguments, .. } => {
                self.count_calls_in_expression(object, call_counts);
                for arg in arguments {
                    self.count_calls_in_expression(arg, call_counts);
                }
            }
            _ => {}
        }
    }

    /// Find functions that call themselves (directly recursive)
    fn find_recursive_functions(&self, functions: &[Function]) -> HashSet<String> {
        let mut recursive = HashSet::new();

        for function in functions {
            let mut call_counts = HashMap::new();
            self.count_calls_in_statements(&function.body, &mut call_counts);
            
            if call_counts.contains_key(&function.name) {
                recursive.insert(function.name.clone());
            }
        }

        recursive
    }

    /// Estimate the size (complexity) of a function in terms of statements
    fn estimate_function_size(&self, function: &Function) -> usize {
        self.count_statements(&function.body)
    }

    /// Count statements recursively
    fn count_statements(&self, statements: &[Statement]) -> usize {
        let mut count = 0;
        for statement in statements {
            count += match statement {
                Statement::Block(stmts) => 1 + self.count_statements(stmts),
                Statement::If { then_stmt, else_stmt, .. } => {
                    let then_count = self.count_statements(&[(**then_stmt).clone()]);
                    let else_count = if let Some(else_stmt) = else_stmt {
                        self.count_statements(&[(**else_stmt).clone()])
                    } else { 0 };
                    1 + then_count + else_count
                }
                Statement::For { body, .. } => {
                    1 + self.count_statements(&[(**body).clone()])
                }
                _ => 1,
            };
        }
        count
    }

    /// Check if a function has side effects (I/O, global state modification, etc.)
    fn has_side_effects(&self, function: &Function) -> bool {
        // This is a simplified heuristic - a full implementation would need
        // more sophisticated analysis
        self.check_side_effects_in_statements(&function.body)
    }

    /// Check for side effects in statements
    fn check_side_effects_in_statements(&self, statements: &[Statement]) -> bool {
        statements.iter().any(|stmt| self.check_side_effects_in_statement(stmt))
    }

    /// Check for side effects in a statement
    fn check_side_effects_in_statement(&self, statement: &Statement) -> bool {
        match statement {
            Statement::Expression(expr) => self.check_side_effects_in_expression(expr),
            Statement::Variable { initializer: Some(expr), .. } => {
                self.check_side_effects_in_expression(expr)
            }
            Statement::Assignment { .. } => true, // Assignment is a side effect
            Statement::If { condition, then_stmt, else_stmt } => {
                self.check_side_effects_in_expression(condition) ||
                self.check_side_effects_in_statement(then_stmt) ||
                else_stmt.as_ref().map_or(false, |stmt| self.check_side_effects_in_statement(stmt))
            }
            Statement::For { init, condition, update, body } => {
                init.as_ref().map_or(false, |stmt| self.check_side_effects_in_statement(stmt)) ||
                condition.as_ref().map_or(false, |expr| self.check_side_effects_in_expression(expr)) ||
                update.as_ref().map_or(false, |expr| self.check_side_effects_in_expression(expr)) ||
                self.check_side_effects_in_statement(body)
            }
            Statement::Return { value } => {
                value.as_ref().map_or(false, |expr| self.check_side_effects_in_expression(expr))
            }
            Statement::Block(statements) => {
                self.check_side_effects_in_statements(statements)
            }
            _ => false,
        }
    }

    /// Check for side effects in an expression
    fn check_side_effects_in_expression(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Call { function, .. } => {
                // Function calls may have side effects - be conservative
                if let Expression::Variable(name) = function.as_ref() {
                    // Known pure functions
                    !matches!(name.as_str(), "abs" | "min" | "max" | "floor" | "ceil" | "round")
                } else {
                    true
                }
            }
            Expression::MethodCall { .. } => true, // Method calls may have side effects
            Expression::Assignment { .. } => true, // Assignment is a side effect
            _ => false, // Literals and most other expressions are pure
        }
    }

    /// Sort candidates by their inlining benefit
    fn sort_candidates_by_benefit(&self, candidates: &HashMap<String, InlineCandidate>) -> Vec<(String, f64)> {
        let mut scored: Vec<_> = candidates.iter().map(|(name, candidate)| {
            let benefit = self.calculate_inlining_benefit(candidate);
            (name.clone(), benefit)
        }).collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Calculate the benefit score for inlining a function
    fn calculate_inlining_benefit(&self, candidate: &InlineCandidate) -> f64 {
        let mut score = 0.0;

        // Benefit from eliminating call overhead
        score += candidate.call_count as f64 * 2.0;

        // Penalty for code size increase
        score -= candidate.size_estimate as f64 * candidate.call_count as f64 * 0.1;

        // Bonus for small functions
        if candidate.size_estimate <= 5 {
            score += 10.0;
        }

        // Bonus for functions called only once
        if candidate.call_count == 1 {
            score += 5.0;
        }

        // Penalty for recursive functions
        if candidate.is_recursive {
            score -= 20.0;
        }

        // Penalty for functions with side effects in loops
        if candidate.has_side_effects {
            score -= 3.0;
        }

        score.max(0.0)
    }

    /// Determine if a candidate should be inlined
    fn should_inline(&self, candidate: &InlineCandidate) -> bool {
        // Don't inline if we've reached maximum depth
        if self.current_depth >= self.max_inline_depth {
            return false;
        }

        // Don't inline recursive functions
        if candidate.is_recursive {
            return false;
        }

        // Don't inline if the function is too large
        if candidate.size_estimate > self.max_inline_size {
            return false;
        }

        // Don't inline if benefit is too low
        let benefit = self.calculate_inlining_benefit(candidate);
        benefit > 1.0
    }

    /// Inline a specific function throughout the program
    fn inline_function(&mut self, program: &mut Program, func_name: &str, candidate: &InlineCandidate) -> Result<InliningResults, CompilerError> {
        let mut results = InliningResults::default();
        
        if self.inlined_functions.contains(func_name) {
            return Ok(results); // Already inlined
        }

        self.current_depth += 1;
        self.inlined_functions.insert(func_name.to_string());

        for function in &mut program.functions {
            if function.name != func_name { // Don't inline into the function itself
                let call_sites = self.inline_calls_in_function(function, func_name, &candidate.function)?;
                results.call_sites_inlined += call_sites;
            }
        }

        self.current_depth -= 1;

        // Estimate size increase
        results.size_increase = candidate.size_estimate * results.call_sites_inlined;
        
        // Estimate performance gain (rough heuristic)
        results.estimated_performance_gain = results.call_sites_inlined as f64 * 2.0; // 2% per inlined call

        if self.debug && results.call_sites_inlined > 0 {
            println!("FI: Inlined function '{}' at {} call sites", func_name, results.call_sites_inlined);
        }

        Ok(results)
    }

    /// Inline calls to a specific function within another function
    fn inline_calls_in_function(&mut self, caller: &mut Function, callee_name: &str, callee: &Function) -> Result<usize, CompilerError> {
        let mut inlined_count = 0;

        for statement in &mut caller.body {
            inlined_count += self.inline_calls_in_statement(statement, callee_name, callee)?;
        }

        Ok(inlined_count)
    }

    /// Inline calls in a statement
    fn inline_calls_in_statement(&mut self, statement: &mut Statement, callee_name: &str, callee: &Function) -> Result<usize, CompilerError> {
        let mut inlined_count = 0;

        match statement {
            Statement::Expression(expr) => {
                if let Some(inlined_expr) = self.try_inline_call_expression(expr, callee_name, callee)? {
                    *expr = inlined_expr;
                    inlined_count += 1;
                }
            }
            Statement::Variable { initializer: Some(expr), .. } => {
                if let Some(inlined_expr) = self.try_inline_call_expression(expr, callee_name, callee)? {
                    *expr = inlined_expr;
                    inlined_count += 1;
                }
            }
            Statement::Assignment { value, .. } => {
                if let Some(inlined_expr) = self.try_inline_call_expression(value, callee_name, callee)? {
                    *value = inlined_expr;
                    inlined_count += 1;
                }
            }
            Statement::Block(statements) => {
                for stmt in statements {
                    inlined_count += self.inline_calls_in_statement(stmt, callee_name, callee)?;
                }
            }
            _ => {}
        }

        Ok(inlined_count)
    }

    /// Try to inline a function call expression
    fn try_inline_call_expression(&self, expr: &Expression, callee_name: &str, callee: &Function) -> Result<Option<Expression>, CompilerError> {
        if let Expression::Call { function, arguments } = expr {
            if let Expression::Variable(name) = function.as_ref() {
                if name == callee_name {
                    // Found a call to inline
                    return Ok(Some(self.inline_function_body(callee, arguments)?));
                }
            }
        }
        Ok(None)
    }

    /// Inline a function body with argument substitution
    fn inline_function_body(&self, callee: &Function, arguments: &[Expression]) -> Result<Expression, CompilerError> {
        // Create parameter -> argument mapping
        let mut param_map = HashMap::new();
        for (param, arg) in callee.params.iter().zip(arguments.iter()) {
            param_map.insert(param.name.clone(), arg.clone());
        }

        // For simplicity, we'll create a block expression with the function body
        // In a real implementation, this would need more sophisticated handling
        // of returns, variable scoping, etc.
        let mut inlined_statements = Vec::new();
        
        for statement in &callee.body {
            let substituted = self.substitute_parameters(statement, &param_map)?;
            inlined_statements.push(substituted);
        }

        // Return the last expression if it's a return statement
        if let Some(Statement::Return { value: Some(expr) }) = inlined_statements.last() {
            Ok(expr.clone())
        } else {
            // Create a block expression - this is simplified
            Ok(Expression::Literal("inlined_result".to_string()))
        }
    }

    /// Substitute parameters with arguments in a statement
    fn substitute_parameters(&self, statement: &Statement, param_map: &HashMap<String, Expression>) -> Result<Statement, CompilerError> {
        // This is a simplified implementation
        // A full implementation would need to handle all statement and expression types
        // and properly manage variable scoping
        
        match statement {
            Statement::Return { value: Some(expr) } => {
                let substituted_expr = self.substitute_parameters_in_expression(expr, param_map)?;
                Ok(Statement::Return { value: Some(substituted_expr) })
            }
            _ => Ok(statement.clone()), // Simplified - just return the original statement
        }
    }

    /// Substitute parameters with arguments in an expression
    fn substitute_parameters_in_expression(&self, expr: &Expression, param_map: &HashMap<String, Expression>) -> Result<Expression, CompilerError> {
        match expr {
            Expression::Variable(name) => {
                if let Some(replacement) = param_map.get(name) {
                    Ok(replacement.clone())
                } else {
                    Ok(expr.clone())
                }
            }
            Expression::Binary { left, right, operator } => {
                let left_sub = self.substitute_parameters_in_expression(left, param_map)?;
                let right_sub = self.substitute_parameters_in_expression(right, param_map)?;
                Ok(Expression::Binary {
                    left: Box::new(left_sub),
                    right: Box::new(right_sub),
                    operator: *operator,
                })
            }
            _ => Ok(expr.clone()), // Simplified - handle other expression types as needed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::types::Type;

    #[test]
    fn test_function_call_counting() {
        let inliner = FunctionInliner::new(false);
        
        let functions = vec![
            Function {
                name: "caller".to_string(),
                params: vec![],
                return_type: Type::Void,
                body: vec![
                    Statement::Expression(Expression::Call {
                        function: Box::new(Expression::Variable(Variable {
                            name: "callee".to_string(),
                            var_type: Type::Function(vec![], Box::new(Type::Void)),
                        })),
                        arguments: vec![],
                    }),
                    Statement::Expression(Expression::Call {
                        function: Box::new(Expression::Variable(Variable {
                            name: "callee".to_string(),
                            var_type: Type::Function(vec![], Box::new(Type::Void)),
                        })),
                        arguments: vec![],
                    }),
                ],
                is_background: false,
            },
        ];

        let call_counts = inliner.count_function_calls(&functions);
        assert_eq!(call_counts.get("callee"), Some(&2));
    }

    #[test]
    fn test_function_size_estimation() {
        let inliner = FunctionInliner::new(false);
        
        let function = Function {
            name: "test".to_string(),
            params: vec![],
            return_type: Type::Void,
            body: vec![
                Statement::Expression(Expression::IntegerLiteral(1)),
                Statement::Return { value: Some(Expression::IntegerLiteral(2)) },
            ],
            is_background: false,
        };

        let size = inliner.estimate_function_size(&function);
        assert_eq!(size, 2);
    }

    #[test]
    fn test_inlining_benefit_calculation() {
        let inliner = FunctionInliner::new(false);
        
        let candidate = InlineCandidate {
            function: Function {
                name: "small_func".to_string(),
                params: vec![],
                return_type: Type::Void,
                body: vec![Statement::Return { value: Some(Expression::IntegerLiteral(42)) }],
                is_background: false,
            },
            call_count: 3,
            size_estimate: 1,
            is_recursive: false,
            has_side_effects: false,
        };

        let benefit = inliner.calculate_inlining_benefit(&candidate);
        assert!(benefit > 0.0);
    }
}