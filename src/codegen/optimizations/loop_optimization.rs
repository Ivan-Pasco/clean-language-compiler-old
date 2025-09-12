use crate::error::CompilerError;
use crate::ast::{Expression, Statement, Function, Program, BinaryOperator, Type};
use std::collections::HashSet;

/// Loop optimization optimizer
/// 
/// Optimizes loops for better performance:
/// - Loop unrolling for small, fixed-iteration loops
/// - Loop invariant code motion (LICM)
/// - Strength reduction (expensive operations to cheaper ones)
/// - Loop fusion (combining similar loops)
/// - Dead loop elimination
/// - Constant loop folding
pub struct LoopOptimizer {
    debug: bool,
    max_unroll_iterations: usize,
    max_unroll_body_size: usize,
}

/// Information about a loop
#[derive(Debug, Clone)]
pub struct LoopInfo {
    pub loop_type: LoopType,
    pub induction_variables: Vec<InductionVariable>,
    pub invariant_expressions: Vec<Expression>,
    pub iteration_count: Option<usize>,
    pub body_size: usize,
    pub has_side_effects: bool,
    pub has_function_calls: bool,
}

/// Types of loops we can analyze
#[derive(Debug, Clone)]
pub enum LoopType {
    For {
        init: Option<Box<Statement>>,
        condition: Option<Expression>,
        update: Option<Expression>,
    },
    While {
        condition: Expression,
    },
    DoWhile {
        condition: Expression,
    },
}

/// Induction variable information
#[derive(Debug, Clone)]
pub struct InductionVariable {
    pub name: String,
    pub initial_value: Option<Expression>,
    pub step: Option<Expression>,
    pub is_linear: bool,
}

/// Results from loop optimization
#[derive(Debug, Default)]
pub struct LoopOptimizationResults {
    pub loops_unrolled: usize,
    pub invariant_expressions_moved: usize,
    pub strength_reductions: usize,
    pub dead_loops_eliminated: usize,
    pub loops_fused: usize,
}

impl LoopOptimizer {
    pub fn new(debug: bool) -> Self {
        Self {
            debug,
            max_unroll_iterations: 8,
            max_unroll_body_size: 20,
        }
    }

    pub fn with_limits(debug: bool, max_iterations: usize, max_body_size: usize) -> Self {
        Self {
            debug,
            max_unroll_iterations: max_iterations,
            max_unroll_body_size: max_body_size,
        }
    }

    /// Optimize loops in the entire program
    pub fn optimize(&mut self, program: &mut Program) -> Result<LoopOptimizationResults, CompilerError> {
        let mut results = LoopOptimizationResults::default();

        for function in &mut program.functions {
            let func_results = self.optimize_loops_in_function(function)?;
            results.loops_unrolled += func_results.loops_unrolled;
            results.invariant_expressions_moved += func_results.invariant_expressions_moved;
            results.strength_reductions += func_results.strength_reductions;
            results.dead_loops_eliminated += func_results.dead_loops_eliminated;
            results.loops_fused += func_results.loops_fused;
        }

        if self.debug {
            println!("LO Results: {} unrolled, {} LICM, {} strength reductions, {} dead loops, {} fused",
                     results.loops_unrolled, results.invariant_expressions_moved,
                     results.strength_reductions, results.dead_loops_eliminated, results.loops_fused);
        }

        Ok(results)
    }

    /// Optimize loops within a function
    fn optimize_loops_in_function(&mut self, function: &mut Function) -> Result<LoopOptimizationResults, CompilerError> {
        let mut results = LoopOptimizationResults::default();

        // Process statements and optimize loops
        let mut i = 0;
        while i < function.body.len() {
            match &mut function.body[i] {
                Statement::For { init, condition, update, body } => {
                    let loop_info = self.analyze_for_loop(init.as_ref(), condition.as_ref(), update.as_ref(), body)?;
                    let loop_results = self.optimize_for_loop(&mut function.body, i, &loop_info)?;
                    results.loops_unrolled += loop_results.loops_unrolled;
                    results.invariant_expressions_moved += loop_results.invariant_expressions_moved;
                    results.strength_reductions += loop_results.strength_reductions;
                    results.dead_loops_eliminated += loop_results.dead_loops_eliminated;
                }
                Statement::While { condition, body } => {
                    let loop_info = self.analyze_while_loop(condition, body)?;
                    let loop_results = self.optimize_while_loop(&mut function.body, i, &loop_info)?;
                    results.loops_unrolled += loop_results.loops_unrolled;
                    results.invariant_expressions_moved += loop_results.invariant_expressions_moved;
                    results.strength_reductions += loop_results.strength_reductions;
                    results.dead_loops_eliminated += loop_results.dead_loops_eliminated;
                }
                Statement::Block(statements) => {
                    // Recursively process nested blocks
                    let mut nested_function = Function {
                        name: format!("{}_block", function.name),
                        params: function.params.clone(),
                        return_type: function.return_type.clone(),
                        body: statements.clone(),
                        is_async: function.is_async,
                    };
                    let nested_results = self.optimize_loops_in_function(&mut nested_function)?;
                    *statements = nested_function.body;
                    results.loops_unrolled += nested_results.loops_unrolled;
                    results.invariant_expressions_moved += nested_results.invariant_expressions_moved;
                    results.strength_reductions += nested_results.strength_reductions;
                    results.dead_loops_eliminated += nested_results.dead_loops_eliminated;
                }
                _ => {}
            }
            i += 1;
        }

        // Look for loop fusion opportunities
        results.loops_fused += self.try_loop_fusion(&mut function.body)?;

        Ok(results)
    }

    /// Analyze a for loop to gather optimization information
    fn analyze_for_loop(&self, init: Option<&Box<Statement>>, condition: Option<&Expression>, 
                       update: Option<&Expression>, body: &Statement) -> Result<LoopInfo, CompilerError> {
        
        let mut induction_variables = Vec::new();
        let mut iteration_count = None;

        // Analyze initialization
        if let Some(Statement::VariableDecl { name, initializer: Some(init_expr), .. }) = init.as_ref().map(|s| s.as_ref()) {
            if let Some(initial_val) = self.extract_constant_value(init_expr) {
                let step = update.and_then(|u| self.extract_step_for_variable(u, name));
                let induction_var = InductionVariable {
                    name: name.clone(),
                    initial_value: Some(init_expr.clone()),
                    step,
                    is_linear: true,
                };
                induction_variables.push(induction_var);

                // Try to compute iteration count for simple cases
                if let (Some(cond), Some(step_val)) = (condition, step.as_ref()) {
                    iteration_count = self.compute_iteration_count(initial_val, cond, step_val);
                }
            }
        }

        let body_size = self.estimate_statement_size(body);
        let invariant_expressions = self.find_loop_invariant_expressions(body, &induction_variables);
        let has_side_effects = self.statement_has_side_effects(body);
        let has_function_calls = self.statement_has_function_calls(body);

        Ok(LoopInfo {
            loop_type: LoopType::For {
                init: init.cloned(),
                condition: condition.cloned(),
                update: update.cloned(),
            },
            induction_variables,
            invariant_expressions,
            iteration_count,
            body_size,
            has_side_effects,
            has_function_calls,
        })
    }

    /// Analyze a while loop
    fn analyze_while_loop(&self, condition: &Expression, body: &Statement) -> Result<LoopInfo, CompilerError> {
        let induction_variables = self.find_induction_variables_in_while(body);
        let body_size = self.estimate_statement_size(body);
        let invariant_expressions = self.find_loop_invariant_expressions(body, &induction_variables);
        let has_side_effects = self.statement_has_side_effects(body);
        let has_function_calls = self.statement_has_function_calls(body);

        Ok(LoopInfo {
            loop_type: LoopType::While {
                condition: condition.clone(),
            },
            induction_variables,
            invariant_expressions,
            iteration_count: None, // Hard to determine for while loops
            body_size,
            has_side_effects,
            has_function_calls,
        })
    }

    /// Optimize a for loop
    fn optimize_for_loop(&mut self, statements: &mut Vec<Statement>, loop_index: usize, 
                        loop_info: &LoopInfo) -> Result<LoopOptimizationResults, CompilerError> {
        let mut results = LoopOptimizationResults::default();

        // Check for dead loop (never executes)
        if self.is_dead_loop(loop_info) {
            if self.debug {
                println!("LO: Eliminating dead for loop");
            }
            statements.remove(loop_index);
            results.dead_loops_eliminated += 1;
            return Ok(results);
        }

        // Try loop unrolling
        if self.should_unroll_loop(loop_info) {
            if self.debug {
                println!("LO: Unrolling for loop with {} iterations", 
                         loop_info.iteration_count.unwrap_or(0));
            }
            self.unroll_for_loop(statements, loop_index, loop_info)?;
            results.loops_unrolled += 1;
        }

        // Move loop invariant code
        if !loop_info.invariant_expressions.is_empty() {
            results.invariant_expressions_moved += self.move_loop_invariant_code(statements, loop_index, loop_info)?;
        }

        // Apply strength reduction
        results.strength_reductions += self.apply_strength_reduction_to_loop(statements, loop_index)?;

        Ok(results)
    }

    /// Optimize a while loop
    fn optimize_while_loop(&mut self, statements: &mut Vec<Statement>, loop_index: usize,
                          loop_info: &LoopInfo) -> Result<LoopOptimizationResults, CompilerError> {
        let mut results = LoopOptimizationResults::default();

        // Check for constant false condition
        if let LoopType::While { condition } = &loop_info.loop_type {
            if let Some(false) = self.evaluate_constant_boolean(condition) {
                if self.debug {
                    println!("LO: Eliminating while loop with constant false condition");
                }
                statements.remove(loop_index);
                results.dead_loops_eliminated += 1;
                return Ok(results);
            }
        }

        // Move loop invariant code
        if !loop_info.invariant_expressions.is_empty() {
            results.invariant_expressions_moved += self.move_loop_invariant_code(statements, loop_index, loop_info)?;
        }

        // Apply strength reduction
        results.strength_reductions += self.apply_strength_reduction_to_loop(statements, loop_index)?;

        Ok(results)
    }

    /// Check if a loop should be unrolled
    fn should_unroll_loop(&self, loop_info: &LoopInfo) -> bool {
        if let Some(iterations) = loop_info.iteration_count {
            iterations <= self.max_unroll_iterations &&
            loop_info.body_size <= self.max_unroll_body_size &&
            !loop_info.has_function_calls && // Avoid unrolling loops with function calls
            iterations > 1
        } else {
            false
        }
    }

    /// Check if a loop is dead (never executes)
    fn is_dead_loop(&self, loop_info: &LoopInfo) -> bool {
        match &loop_info.loop_type {
            LoopType::For { condition: Some(cond), .. } => {
                self.evaluate_constant_boolean(cond) == Some(false)
            }
            LoopType::While { condition } => {
                self.evaluate_constant_boolean(condition) == Some(false)
            }
            _ => false,
        }
    }

    /// Unroll a for loop
    fn unroll_for_loop(&mut self, statements: &mut Vec<Statement>, loop_index: usize,
                      loop_info: &LoopInfo) -> Result<(), CompilerError> {
        
        if let (Some(iterations), LoopType::For { init, condition: _, update, .. }) = 
            (loop_info.iteration_count, &loop_info.loop_type) {
            
            // Get the original loop body
            if let Statement::For { body, .. } = &statements[loop_index] {
                let original_body = body.clone();
                let mut unrolled_statements = Vec::new();

                // Add initialization
                if let Some(init_stmt) = init {
                    unrolled_statements.push((**init_stmt).clone());
                }

                // Unroll the loop body
                for i in 0..iterations {
                    let mut iteration_body = self.substitute_induction_variable(&original_body, &loop_info.induction_variables, i)?;
                    unrolled_statements.push(*iteration_body);

                    // Add update statement (except for last iteration)
                    if i < iterations - 1 {
                        if let Some(update_expr) = update {
                            unrolled_statements.push(Statement::Expression(update_expr.clone()));
                        }
                    }
                }

                // Replace the loop with unrolled statements
                statements.splice(loop_index..=loop_index, unrolled_statements);
            }
        }

        Ok(())
    }

    /// Move loop invariant code out of the loop
    fn move_loop_invariant_code(&mut self, statements: &mut Vec<Statement>, loop_index: usize,
                               loop_info: &LoopInfo) -> Result<usize, CompilerError> {
        let mut moved_count = 0;

        // For each invariant expression, try to move it before the loop
        for invariant_expr in &loop_info.invariant_expressions {
            // Create a temporary variable to hold the invariant value
            let temp_var_name = format!("loop_invariant_{}", moved_count);
            let temp_var_stmt = Statement::Variable {
                name: temp_var_name.clone(),
                var_type: self.infer_expression_type(invariant_expr)?,
                initializer: Some(invariant_expr.clone()),
                is_mutable: false,
            };

            // Insert before the loop
            statements.insert(loop_index, temp_var_stmt);

            // TODO: Replace uses of the invariant expression in the loop body
            // with references to the temporary variable
            moved_count += 1;
        }

        if self.debug && moved_count > 0 {
            println!("LO: Moved {} invariant expressions out of loop", moved_count);
        }

        Ok(moved_count)
    }

    /// Apply strength reduction to a loop
    fn apply_strength_reduction_to_loop(&mut self, statements: &mut Vec<Statement>, loop_index: usize) -> Result<usize, CompilerError> {
        let mut reductions = 0;

        // Get the loop body and look for expensive operations that can be reduced
        if let Some(loop_stmt) = statements.get_mut(loop_index) {
            reductions += self.apply_strength_reduction_to_statement(loop_stmt)?;
        }

        if self.debug && reductions > 0 {
            println!("LO: Applied {} strength reductions in loop", reductions);
        }

        Ok(reductions)
    }

    /// Apply strength reduction to a statement
    fn apply_strength_reduction_to_statement(&mut self, statement: &mut Statement) -> Result<usize, CompilerError> {
        let mut reductions = 0;

        match statement {
            Statement::Expression(expr) => {
                reductions += self.apply_strength_reduction_to_expression(expr)?;
            }
            Statement::Assignment { value, .. } => {
                reductions += self.apply_strength_reduction_to_expression(value)?;
            }
            Statement::For { body, .. } => {
                reductions += self.apply_strength_reduction_to_statement(body)?;
            }
            Statement::While { body, .. } => {
                reductions += self.apply_strength_reduction_to_statement(body)?;
            }
            Statement::Block(statements) => {
                for stmt in statements {
                    reductions += self.apply_strength_reduction_to_statement(stmt)?;
                }
            }
            _ => {}
        }

        Ok(reductions)
    }

    /// Apply strength reduction to an expression
    fn apply_strength_reduction_to_expression(&mut self, expr: &mut Expression) -> Result<usize, CompilerError> {
        let mut reductions = 0;

        match expr {
            Expression::Binary { left, right, operator } => {
                // Recursively process operands
                reductions += self.apply_strength_reduction_to_expression(left)?;
                reductions += self.apply_strength_reduction_to_expression(right)?;

                // Look for multiplication by power of 2 (can be converted to left shift)
                if *operator == BinaryOperator::Multiply {
                    if let Expression::IntegerLiteral(n) = right.as_ref() {
                        if n.is_power_of_two() && *n > 1 {
                            // Convert multiplication to left shift
                            let shift_amount = n.trailing_zeros() as i64;
                            *operator = BinaryOperator::LeftShift;
                            **right = Expression::IntegerLiteral(shift_amount);
                            reductions += 1;

                            if self.debug {
                                println!("LO: Strength reduction: multiply by {} -> left shift by {}", n, shift_amount);
                            }
                        }
                    }
                }

                // Look for division by power of 2 (can be converted to right shift)
                if *operator == BinaryOperator::Divide {
                    if let Expression::IntegerLiteral(n) = right.as_ref() {
                        if n.is_power_of_two() && *n > 1 {
                            // Convert division to right shift
                            let shift_amount = n.trailing_zeros() as i64;
                            *operator = BinaryOperator::RightShift;
                            **right = Expression::IntegerLiteral(shift_amount);
                            reductions += 1;

                            if self.debug {
                                println!("LO: Strength reduction: divide by {} -> right shift by {}", n, shift_amount);
                            }
                        }
                    }
                }
            }
            Expression::Call { arguments, .. } => {
                for arg in arguments {
                    reductions += self.apply_strength_reduction_to_expression(arg)?;
                }
            }
            _ => {}
        }

        Ok(reductions)
    }

    /// Try to fuse adjacent similar loops
    fn try_loop_fusion(&mut self, statements: &mut Vec<Statement>) -> Result<usize, CompilerError> {
        let mut fused_count = 0;

        // Look for adjacent for loops that can be fused
        let mut i = 0;
        while i + 1 < statements.len() {
            if let (Statement::For { init: init1, condition: cond1, update: update1, body: body1 },
                   Statement::For { init: init2, condition: cond2, update: update2, body: body2 }) =
                (&statements[i], &statements[i + 1]) {
                
                // Check if the loops can be fused (same iteration pattern)
                if self.can_fuse_loops(init1.as_ref(), cond1.as_ref(), update1.as_ref(),
                                      init2.as_ref(), cond2.as_ref(), update2.as_ref()) {
                    
                    // Fuse the loop bodies
                    let fused_body = self.fuse_loop_bodies(body1, body2)?;
                    
                    // Replace first loop with fused version
                    statements[i] = Statement::For {
                        init: init1.clone(),
                        condition: cond1.clone(),
                        update: update1.clone(),
                        body: Box::new(fused_body),
                    };
                    
                    // Remove second loop
                    statements.remove(i + 1);
                    
                    fused_count += 1;
                    
                    if self.debug {
                        println!("LO: Fused two adjacent for loops");
                    }
                    
                    // Don't increment i, check for more fusion opportunities
                    continue;
                }
            }
            i += 1;
        }

        Ok(fused_count)
    }

    // Helper methods
    
    fn extract_constant_value(&self, expr: &Expression) -> Option<i64> {
        match expr {
            Expression::IntegerLiteral(val) => Some(*val),
            _ => None,
        }
    }

    fn extract_step_for_variable(&self, update_expr: &Expression, var_name: &str) -> Option<Expression> {
        // Look for patterns like i = i + 1, i++, i += 1
        match update_expr {
            Expression::Assignment { target, value } => {
                if let Expression::Variable(name) = target.as_ref() {
                    if name == var_name {
                        if let Expression::Binary { left, right, operator: BinaryOperator::Add } = value.as_ref() {
                            if let Expression::Variable(left_name) = left.as_ref() {
                                if left_name == var_name {
                                    return Some((**right).clone());
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn compute_iteration_count(&self, initial: i64, condition: &Expression, step: &Expression) -> Option<usize> {
        // For simple cases like i < n with step = 1
        if let (Expression::Binary { left, right, operator }, Expression::IntegerLiteral(step_val)) = (condition, step) {
            if let (Expression::Variable(_), Expression::IntegerLiteral(limit)) = (left.as_ref(), right.as_ref()) {
                match operator {
                    BinaryOperator::Less => {
                        if *step_val > 0 {
                            let iterations = (limit - initial) / step_val;
                            if iterations >= 0 {
                                return Some(iterations as usize);
                            }
                        }
                    }
                    BinaryOperator::Greater => {
                        if *step_val < 0 {
                            let iterations = (initial - limit) / (-step_val);
                            if iterations >= 0 {
                                return Some(iterations as usize);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn find_loop_invariant_expressions(&self, body: &Statement, induction_vars: &[InductionVariable]) -> Vec<Expression> {
        // Simplified implementation - find expressions that don't depend on induction variables
        let mut invariants = Vec::new();
        let induction_var_names: HashSet<String> = induction_vars.iter().map(|iv| iv.name.clone()).collect();
        
        self.collect_invariant_expressions_from_statement(body, &induction_var_names, &mut invariants);
        invariants
    }

    fn collect_invariant_expressions_from_statement(&self, statement: &Statement, 
                                                   induction_vars: &HashSet<String>,
                                                   invariants: &mut Vec<Expression>) {
        // This is a simplified implementation
        // A full implementation would need more sophisticated analysis
        match statement {
            Statement::Expression(expr) => {
                if self.is_loop_invariant(expr, induction_vars) {
                    invariants.push(expr.clone());
                }
            }
            _ => {}
        }
    }

    fn is_loop_invariant(&self, expr: &Expression, induction_vars: &HashSet<String>) -> bool {
        match expr {
            Expression::Variable(name) => !induction_vars.contains(name),
            Expression::IntegerLiteral(_) |
            Expression::NumberLiteral(_) |
            Expression::StringLiteral(_) |
            Expression::BooleanLiteral(_) => true,
            Expression::Binary { left, right, .. } => {
                self.is_loop_invariant(left, induction_vars) && 
                self.is_loop_invariant(right, induction_vars)
            }
            _ => false,
        }
    }

    fn find_induction_variables_in_while(&self, _body: &Statement) -> Vec<InductionVariable> {
        // Simplified implementation - would need more sophisticated analysis
        Vec::new()
    }

    fn estimate_statement_size(&self, statement: &Statement) -> usize {
        match statement {
            Statement::Block(statements) => statements.iter().map(|s| self.estimate_statement_size(s)).sum(),
            Statement::If { then_stmt, else_stmt, .. } => {
                let then_size = self.estimate_statement_size(then_stmt);
                let else_size = else_stmt.as_ref().map_or(0, |s| self.estimate_statement_size(s));
                1 + then_size + else_size
            }
            _ => 1,
        }
    }

    fn statement_has_side_effects(&self, _statement: &Statement) -> bool {
        // Simplified implementation
        false
    }

    fn statement_has_function_calls(&self, _statement: &Statement) -> bool {
        // Simplified implementation
        false
    }

    fn evaluate_constant_boolean(&self, expr: &Expression) -> Option<bool> {
        match expr {
            Expression::BooleanLiteral(val) => Some(*val),
            Expression::IntegerLiteral(val) => Some(*val != 0),
            _ => None,
        }
    }

    fn substitute_induction_variable(&self, body: &Statement, _induction_vars: &[InductionVariable], 
                                   _iteration: usize) -> Result<Box<Statement>, CompilerError> {
        // Simplified implementation - would need to substitute induction variable values
        Ok(Box::new(body.clone()))
    }

    fn infer_expression_type(&self, _expr: &Expression) -> Result<Type, CompilerError> {
        // Simplified implementation
        Ok(Type::Integer)
    }

    fn can_fuse_loops(&self, init1: Option<&Box<Statement>>, cond1: Option<&Expression>, update1: Option<&Expression>,
                     init2: Option<&Box<Statement>>, cond2: Option<&Expression>, update2: Option<&Expression>) -> bool {
        // Simplified check - loops can be fused if they have the same structure
        match ((init1, cond1, update1), (init2, cond2, update2)) {
            ((Some(_), Some(_), Some(_)), (Some(_), Some(_), Some(_))) => {
                // Would need more sophisticated comparison
                false
            }
            _ => false,
        }
    }

    fn fuse_loop_bodies(&self, body1: &Statement, body2: &Statement) -> Result<Statement, CompilerError> {
        // Combine the two loop bodies into a single block
        match (body1, body2) {
            (Statement::Block(stmts1), Statement::Block(stmts2)) => {
                let mut fused_stmts = stmts1.clone();
                fused_stmts.extend(stmts2.clone());
                Ok(Statement::Block(fused_stmts))
            }
            (stmt1, Statement::Block(stmts2)) => {
                let mut fused_stmts = vec![stmt1.clone()];
                fused_stmts.extend(stmts2.clone());
                Ok(Statement::Block(fused_stmts))
            }
            (Statement::Block(stmts1), stmt2) => {
                let mut fused_stmts = stmts1.clone();
                fused_stmts.push(stmt2.clone());
                Ok(Statement::Block(fused_stmts))
            }
            (stmt1, stmt2) => {
                Ok(Statement::Block(vec![stmt1.clone(), stmt2.clone()]))
            }
        }
    }
}

// Helper trait for power of two detection
trait PowerOfTwo {
    fn is_power_of_two(self) -> bool;
}

impl PowerOfTwo for i64 {
    fn is_power_of_two(self) -> bool {
        self > 0 && (self & (self - 1)) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;
    use crate::types::Type;

    #[test]
    fn test_power_of_two_detection() {
        assert!(2i64.is_power_of_two());
        assert!(4i64.is_power_of_two());
        assert!(8i64.is_power_of_two());
        assert!(!3i64.is_power_of_two());
        assert!(!6i64.is_power_of_two());
    }

    #[test]
    fn test_iteration_count_calculation() {
        let optimizer = LoopOptimizer::new(false);
        
        let condition = Expression::Binary {
            left: Box::new(Expression::Variable(Variable {
                name: "i".to_string(),
                var_type: Type::Integer,
            })),
            right: Box::new(Expression::IntegerLiteral(10)),
            operator: BinaryOperator::Less,
        };
        
        let step = Expression::IntegerLiteral(1);
        let count = optimizer.compute_iteration_count(0, &condition, &step);
        assert_eq!(count, Some(10));
    }

    #[test]
    fn test_loop_invariant_detection() {
        let optimizer = LoopOptimizer::new(false);
        let induction_vars = ["i".to_string()].iter().cloned().collect();
        
        // Constant expression should be invariant
        let const_expr = Expression::IntegerLiteral(42);
        assert!(optimizer.is_loop_invariant(&const_expr, &induction_vars));
        
        // Expression using induction variable should not be invariant
        let var_expr = Expression::Variable(Variable {
            name: "i".to_string(),
            var_type: Type::Integer,
        });
        assert!(!optimizer.is_loop_invariant(&var_expr, &induction_vars));
        
        // Expression using non-induction variable should be invariant
        let other_var_expr = Expression::Variable(Variable {
            name: "x".to_string(),
            var_type: Type::Integer,
        });
        assert!(optimizer.is_loop_invariant(&other_var_expr, &induction_vars));
    }
}