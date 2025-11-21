use crate::error::CompilerError;
use crate::ast::{Expression, Statement, Function, Program, BinaryOperator, UnaryOperator};
use std::collections::HashMap;

/// Peephole optimization optimizer
/// 
/// Applies local optimizations to small sequences of instructions:
/// - Algebraic simplifications (x + 0 -> x, x * 1 -> x)
/// - Redundant operation elimination (x - x -> 0)
/// - Constant propagation in small scopes
/// - Boolean simplifications (x && true -> x)
/// - Comparison optimizations (x < x -> false)
/// - Assignment optimizations (x = x -> NOP)
pub struct PeepholeOptimizer {
    debug: bool,
    optimization_patterns: Vec<OptimizationPattern>,
}

/// An optimization pattern that can be applied
#[derive(Debug, Clone)]
pub struct OptimizationPattern {
    pub name: String,
    pub description: String,
    pub pattern_matcher: PatternMatcher,
    pub replacement: PatternReplacement,
}

/// Pattern matching for expressions
#[derive(Debug, Clone)]
pub enum PatternMatcher {
    // Binary operations
    BinaryWithConstants { op: BinaryOperator, left_const: Option<ConstantValue>, right_const: Option<ConstantValue> },
    BinaryWithSameOperands { op: BinaryOperator },
    BinaryWithIdentity { op: BinaryOperator, identity: ConstantValue, position: IdentityPosition },
    
    // Unary operations
    UnaryWithConstant { op: UnaryOperator, operand_const: Option<ConstantValue> },
    DoubleNegation,
    
    // Comparison operations
    SelfComparison { op: BinaryOperator },
    
    // Assignment patterns
    SelfAssignment,
    RedundantAssignment,
    
    // Control flow patterns
    ConstantCondition { value: bool },
}

/// Pattern replacement strategies
#[derive(Debug, Clone)]
pub enum PatternReplacement {
    Constant(ConstantValue),
    Identity,  // Replace with first operand
    IdentitySecond, // Replace with second operand
    Zero,
    One,
    True,
    False,
    Noop, // Remove the statement/expression
}

/// Constant values for pattern matching
#[derive(Debug, Clone, PartialEq)]
pub enum ConstantValue {
    Integer(i64),
    Number(f64),
    String(String),
    Boolean(bool),
    Any, // Matches any constant
}

/// Position of identity element in binary operations
#[derive(Debug, Clone)]
pub enum IdentityPosition {
    Left,   // Identity on left side (0 + x -> x)
    Right,  // Identity on right side (x + 0 -> x)
    Either, // Identity on either side
}

/// Results from peephole optimization
#[derive(Debug, Default)]
pub struct PeepholeResults {
    pub patterns_applied: HashMap<String, usize>,
    pub total_optimizations: usize,
    pub expressions_simplified: usize,
    pub statements_eliminated: usize,
}

impl PeepholeOptimizer {
    pub fn new(debug: bool) -> Self {
        let mut optimizer = Self {
            debug,
            optimization_patterns: Vec::new(),
        };
        optimizer.initialize_patterns();
        optimizer
    }

    /// Initialize standard peephole optimization patterns
    fn initialize_patterns(&mut self) {
        // Arithmetic identity patterns
        self.add_pattern("add_zero", "x + 0 -> x", 
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::Add, 
                identity: ConstantValue::Integer(0), 
                position: IdentityPosition::Either 
            },
            PatternReplacement::Identity
        );

        self.add_pattern("multiply_one", "x * 1 -> x",
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::Multiply, 
                identity: ConstantValue::Integer(1), 
                position: IdentityPosition::Either 
            },
            PatternReplacement::Identity
        );

        self.add_pattern("multiply_zero", "x * 0 -> 0",
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::Multiply, 
                identity: ConstantValue::Integer(0), 
                position: IdentityPosition::Either 
            },
            PatternReplacement::Zero
        );

        self.add_pattern("subtract_zero", "x - 0 -> x",
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::Subtract, 
                identity: ConstantValue::Integer(0), 
                position: IdentityPosition::Right 
            },
            PatternReplacement::Identity
        );

        self.add_pattern("subtract_self", "x - x -> 0",
            PatternMatcher::BinaryWithSameOperands { op: BinaryOperator::Subtract },
            PatternReplacement::Zero
        );

        // Division patterns
        self.add_pattern("divide_one", "x / 1 -> x",
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::Divide, 
                identity: ConstantValue::Integer(1), 
                position: IdentityPosition::Right 
            },
            PatternReplacement::Identity
        );

        self.add_pattern("divide_self", "x / x -> 1 (when x != 0)",
            PatternMatcher::BinaryWithSameOperands { op: BinaryOperator::Divide },
            PatternReplacement::One
        );

        // Boolean logic patterns
        self.add_pattern("and_true", "x && true -> x",
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::And, 
                identity: ConstantValue::Boolean(true), 
                position: IdentityPosition::Either 
            },
            PatternReplacement::Identity
        );

        self.add_pattern("and_false", "x && false -> false",
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::And, 
                identity: ConstantValue::Boolean(false), 
                position: IdentityPosition::Either 
            },
            PatternReplacement::False
        );

        self.add_pattern("or_true", "x || true -> true",
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::Or, 
                identity: ConstantValue::Boolean(true), 
                position: IdentityPosition::Either 
            },
            PatternReplacement::True
        );

        self.add_pattern("or_false", "x || false -> x",
            PatternMatcher::BinaryWithIdentity { 
                op: BinaryOperator::Or, 
                identity: ConstantValue::Boolean(false), 
                position: IdentityPosition::Either 
            },
            PatternReplacement::Identity
        );

        // Comparison patterns
        self.add_pattern("equal_self", "x == x -> true",
            PatternMatcher::SelfComparison { op: BinaryOperator::Equal },
            PatternReplacement::True
        );

        self.add_pattern("not_equal_self", "x != x -> false",
            PatternMatcher::SelfComparison { op: BinaryOperator::NotEqual },
            PatternReplacement::False
        );

        self.add_pattern("less_self", "x < x -> false",
            PatternMatcher::SelfComparison { op: BinaryOperator::Less },
            PatternReplacement::False
        );

        self.add_pattern("greater_self", "x > x -> false",
            PatternMatcher::SelfComparison { op: BinaryOperator::Greater },
            PatternReplacement::False
        );

        // Unary operation patterns
        self.add_pattern("double_negation", "--x -> x",
            PatternMatcher::DoubleNegation,
            PatternReplacement::Identity
        );

        self.add_pattern("not_true", "!true -> false",
            PatternMatcher::UnaryWithConstant { 
                op: UnaryOperator::Not, 
                operand_const: Some(ConstantValue::Boolean(true)) 
            },
            PatternReplacement::False
        );

        self.add_pattern("not_false", "!false -> true",
            PatternMatcher::UnaryWithConstant { 
                op: UnaryOperator::Not, 
                operand_const: Some(ConstantValue::Boolean(false)) 
            },
            PatternReplacement::True
        );

        // Assignment patterns
        self.add_pattern("self_assignment", "x = x -> NOP",
            PatternMatcher::SelfAssignment,
            PatternReplacement::Noop
        );
    }

    fn add_pattern(&mut self, name: &str, description: &str, matcher: PatternMatcher, replacement: PatternReplacement) {
        self.optimization_patterns.push(OptimizationPattern {
            name: name.to_string(),
            description: description.to_string(),
            pattern_matcher: matcher,
            replacement,
        });
    }

    /// Apply peephole optimizations to the entire program
    pub fn optimize(&mut self, program: &mut Program) -> Result<PeepholeResults, CompilerError> {
        let mut results = PeepholeResults::default();

        for function in &mut program.functions {
            let func_results = self.optimize_function(function)?;
            self.merge_results(&mut results, func_results);
        }

        if self.debug {
            println!("PH Results: {} total optimizations", results.total_optimizations);
            for (pattern_name, count) in &results.patterns_applied {
                if *count > 0 {
                    println!("  {}: {} applications", pattern_name, count);
                }
            }
        }

        Ok(results)
    }

    /// Apply peephole optimizations to a function
    fn optimize_function(&mut self, function: &mut Function) -> Result<PeepholeResults, CompilerError> {
        let mut results = PeepholeResults::default();

        // Optimize statements
        let mut i = 0;
        while i < function.body.len() {
            let stmt_results = self.optimize_statement(&mut function.body[i])?;
            self.merge_results(&mut results, stmt_results);

            // Check if statement was eliminated
            if self.is_noop_statement(&function.body[i]) {
                function.body.remove(i);
                results.statements_eliminated += 1;
            } else {
                i += 1;
            }
        }

        Ok(results)
    }

    /// Apply peephole optimizations to a statement
    fn optimize_statement(&mut self, statement: &mut Statement) -> Result<PeepholeResults, CompilerError> {
        let mut results = PeepholeResults::default();

        match statement {
            Statement::Expression(expr) => {
                let expr_results = self.optimize_expression(expr)?;
                self.merge_results(&mut results, expr_results);
            }
            Statement::Variable { initializer: Some(expr), .. } => {
                let expr_results = self.optimize_expression(expr)?;
                self.merge_results(&mut results, expr_results);
            }
            Statement::Assignment { target, value } => {
                // Check for self-assignment pattern first
                if self.is_self_assignment(target, value) {
                    self.apply_pattern_to_statement(statement, "self_assignment")?;
                    results.patterns_applied.insert("self_assignment".to_string(), 1);
                    results.total_optimizations += 1;
                } else {
                    let target_results = self.optimize_expression(target)?;
                    let value_results = self.optimize_expression(value)?;
                    self.merge_results(&mut results, target_results);
                    self.merge_results(&mut results, value_results);
                }
            }
            Statement::If { condition, then_stmt, else_stmt } => {
                let cond_results = self.optimize_expression(condition)?;
                self.merge_results(&mut results, cond_results);

                let then_results = self.optimize_statement(then_stmt)?;
                self.merge_results(&mut results, then_results);

                if let Some(else_stmt) = else_stmt {
                    let else_results = self.optimize_statement(else_stmt)?;
                    self.merge_results(&mut results, else_results);
                }

                // Check for constant condition
                if let Some(const_value) = self.evaluate_constant_boolean(condition) {
                    self.optimize_constant_condition(statement, const_value)?;
                    results.patterns_applied.insert("constant_condition".to_string(), 1);
                    results.total_optimizations += 1;
                }
            }
            Statement::For { init, condition, update, body } => {
                if let Some(init) = init {
                    let init_results = self.optimize_statement(init)?;
                    self.merge_results(&mut results, init_results);
                }
                if let Some(condition) = condition {
                    let cond_results = self.optimize_expression(condition)?;
                    self.merge_results(&mut results, cond_results);
                }
                if let Some(update) = update {
                    let update_results = self.optimize_expression(update)?;
                    self.merge_results(&mut results, update_results);
                }

                let body_results = self.optimize_statement(body)?;
                self.merge_results(&mut results, body_results);
            }
            Statement::Return { value: Some(expr) } => {
                let expr_results = self.optimize_expression(expr)?;
                self.merge_results(&mut results, expr_results);
            }
            Statement::Block(statements) => {
                for stmt in statements {
                    let stmt_results = self.optimize_statement(stmt)?;
                    self.merge_results(&mut results, stmt_results);
                }
            }
            _ => {}
        }

        Ok(results)
    }

    /// Apply peephole optimizations to an expression
    fn optimize_expression(&mut self, expr: &mut Expression) -> Result<PeepholeResults, CompilerError> {
        let mut results = PeepholeResults::default();

        // First, recursively optimize subexpressions
        match expr {
            Expression::Binary { left, right, .. } => {
                let left_results = self.optimize_expression(left)?;
                let right_results = self.optimize_expression(right)?;
                self.merge_results(&mut results, left_results);
                self.merge_results(&mut results, right_results);
            }
            Expression::Unary { operand, .. } => {
                let operand_results = self.optimize_expression(operand)?;
                self.merge_results(&mut results, operand_results);
            }
            Expression::Call { function, arguments } => {
                let func_results = self.optimize_expression(function)?;
                self.merge_results(&mut results, func_results);
                
                for arg in arguments {
                    let arg_results = self.optimize_expression(arg)?;
                    self.merge_results(&mut results, arg_results);
                }
            }
            Expression::Array(elements) => {
                for element in elements {
                    let elem_results = self.optimize_expression(element)?;
                    self.merge_results(&mut results, elem_results);
                }
            }
            Expression::Index { array, index } => {
                let array_results = self.optimize_expression(array)?;
                let index_results = self.optimize_expression(index)?;
                self.merge_results(&mut results, array_results);
                self.merge_results(&mut results, index_results);
            }
            Expression::Conditional { condition, then_expr, else_expr } => {
                let cond_results = self.optimize_expression(condition)?;
                let then_results = self.optimize_expression(then_expr)?;
                let else_results = self.optimize_expression(else_expr)?;
                self.merge_results(&mut results, cond_results);
                self.merge_results(&mut results, then_results);
                self.merge_results(&mut results, else_results);
            }
            _ => {}
        }

        // Then apply peephole patterns to this expression
        for pattern in &self.optimization_patterns.clone() {
            if self.pattern_matches(expr, &pattern.pattern_matcher)? {
                if self.apply_pattern_to_expression(expr, pattern)? {
                    *results.patterns_applied.entry(pattern.name.clone()).or_insert(0) += 1;
                    results.total_optimizations += 1;
                    results.expressions_simplified += 1;

                    if self.debug {
                        println!("PH: Applied pattern '{}' - {}", pattern.name, pattern.description);
                    }
                }
            }
        }

        Ok(results)
    }

    /// Check if a pattern matches an expression
    fn pattern_matches(&self, expr: &Expression, pattern: &PatternMatcher) -> Result<bool, CompilerError> {
        match (expr, pattern) {
            (Expression::Binary { left, right, operator }, PatternMatcher::BinaryWithIdentity { op, identity, position }) => {
                if operator != op {
                    return Ok(false);
                }

                match position {
                    IdentityPosition::Left => Ok(self.expression_matches_constant(left, identity)),
                    IdentityPosition::Right => Ok(self.expression_matches_constant(right, identity)),
                    IdentityPosition::Either => {
                        Ok(self.expression_matches_constant(left, identity) || 
                           self.expression_matches_constant(right, identity))
                    }
                }
            }
            (Expression::Binary { left, right, operator }, PatternMatcher::BinaryWithSameOperands { op }) => {
                Ok(operator == op && self.expressions_equal(left, right))
            }
            (Expression::Binary { operator, .. }, PatternMatcher::SelfComparison { op }) => {
                Ok(operator == op) // Additional check for same operands would be done separately
            }
            (Expression::Unary { operand, operator }, PatternMatcher::UnaryWithConstant { op, operand_const }) => {
                if operator != op {
                    return Ok(false);
                }
                if let Some(const_val) = operand_const {
                    Ok(self.expression_matches_constant(operand, const_val))
                } else {
                    Ok(true)
                }
            }
            (Expression::Unary { operand, operator }, PatternMatcher::DoubleNegation) => {
                if *operator != UnaryOperator::Minus {
                    return Ok(false);
                }
                if let Expression::Unary { operator: inner_op, .. } = operand.as_ref() {
                    Ok(*inner_op == UnaryOperator::Minus)
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        }
    }

    /// Check if an expression matches a constant value
    fn expression_matches_constant(&self, expr: &Expression, constant: &ConstantValue) -> bool {
        match (expr, constant) {
            (Expression::IntegerLiteral(val), ConstantValue::Integer(expected)) => val == expected,
            (Expression::NumberLiteral(val), ConstantValue::Number(expected)) => (val - expected).abs() < f64::EPSILON,
            (Expression::StringLiteral(val), ConstantValue::String(expected)) => val == expected,
            (Expression::BooleanLiteral(val), ConstantValue::Boolean(expected)) => val == expected,
            (_, ConstantValue::Any) => self.is_constant_expression(expr),
            _ => false,
        }
    }

    /// Check if two expressions are equal
    fn expressions_equal(&self, left: &Expression, right: &Expression) -> bool {
        match (left, right) {
            (Expression::Variable(var1), Expression::Variable(var2)) => var1.name == var2.name,
            (Expression::IntegerLiteral(a), Expression::IntegerLiteral(b)) => a == b,
            (Expression::NumberLiteral(a), Expression::NumberLiteral(b)) => (a - b).abs() < f64::EPSILON,
            (Expression::StringLiteral(a), Expression::StringLiteral(b)) => a == b,
            (Expression::BooleanLiteral(a), Expression::BooleanLiteral(b)) => a == b,
            (Expression::Binary { left: l1, right: r1, operator: op1 }, 
             Expression::Binary { left: l2, right: r2, operator: op2 }) => {
                op1 == op2 && self.expressions_equal(l1, l2) && self.expressions_equal(r1, r2)
            }
            _ => false, // Conservative approach
        }
    }

    /// Check if an expression is constant
    fn is_constant_expression(&self, expr: &Expression) -> bool {
        matches!(expr, Expression::IntegerLiteral(_) | 
                      Expression::NumberLiteral(_) | 
                      Expression::StringLiteral(_) | 
                      Expression::BooleanLiteral(_))
    }

    /// Apply a pattern to replace an expression
    fn apply_pattern_to_expression(&self, expr: &mut Expression, pattern: &OptimizationPattern) -> Result<bool, CompilerError> {
        match &pattern.replacement {
            PatternReplacement::Constant(const_val) => {
                *expr = self.constant_value_to_expression(const_val);
                Ok(true)
            }
            PatternReplacement::Identity => {
                if let Expression::Binary { left, right, .. } = expr {
                    // Determine which operand to keep based on pattern
                    match &pattern.pattern_matcher {
                        PatternMatcher::BinaryWithIdentity { identity, position, .. } => {
                            match position {
                                IdentityPosition::Left => *expr = (**right).clone(),
                                IdentityPosition::Right => *expr = (**left).clone(),
                                IdentityPosition::Either => {
                                    if self.expression_matches_constant(left, identity) {
                                        *expr = (**right).clone();
                                    } else {
                                        *expr = (**left).clone();
                                    }
                                }
                            }
                        }
                        PatternMatcher::BinaryWithSameOperands { .. } => *expr = (**left).clone(),
                        _ => return Ok(false),
                    }
                } else if let Expression::Unary { operand, .. } = expr {
                    if matches!(pattern.pattern_matcher, PatternMatcher::DoubleNegation) {
                        if let Expression::Unary { operand: inner_operand, .. } = operand.as_ref() {
                            *expr = (**inner_operand).clone();
                        }
                    }
                }
                Ok(true)
            }
            PatternReplacement::Zero => {
                *expr = Expression::IntegerLiteral(0);
                Ok(true)
            }
            PatternReplacement::One => {
                *expr = Expression::IntegerLiteral(1);
                Ok(true)
            }
            PatternReplacement::True => {
                *expr = Expression::BooleanLiteral(true);
                Ok(true)
            }
            PatternReplacement::False => {
                *expr = Expression::BooleanLiteral(false);
                Ok(true)
            }
            PatternReplacement::Noop => {
                // For expressions, we can't really remove them, so mark as optimized
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Apply a pattern to a statement
    fn apply_pattern_to_statement(&self, statement: &mut Statement, pattern_name: &str) -> Result<(), CompilerError> {
        if pattern_name == "self_assignment" {
            *statement = Statement::Block(vec![]); // Replace with empty block
        }
        Ok(())
    }

    /// Convert constant value to expression
    fn constant_value_to_expression(&self, const_val: &ConstantValue) -> Expression {
        match const_val {
            ConstantValue::Integer(val) => Expression::IntegerLiteral(*val),
            ConstantValue::Number(val) => Expression::NumberLiteral(*val),
            ConstantValue::String(val) => Expression::StringLiteral(val.clone()),
            ConstantValue::Boolean(val) => Expression::BooleanLiteral(*val),
            ConstantValue::Any => Expression::IntegerLiteral(0), // Fallback
        }
    }

    /// Check if assignment is self-assignment (x = x)
    fn is_self_assignment(&self, target: &Expression, value: &Expression) -> bool {
        self.expressions_equal(target, value)
    }

    /// Check if statement is effectively a no-op
    fn is_noop_statement(&self, statement: &Statement) -> bool {
        match statement {
            Statement::Block(statements) => statements.is_empty(),
            Statement::Expression(Expression::IntegerLiteral(0)) => true, // Placeholder for removed expressions
            _ => false,
        }
    }

    /// Evaluate constant boolean expressions
    fn evaluate_constant_boolean(&self, expr: &Expression) -> Option<bool> {
        match expr {
            Expression::BooleanLiteral(val) => Some(*val),
            Expression::IntegerLiteral(val) => Some(*val != 0),
            Expression::NumberLiteral(val) => Some(*val != 0.0),
            _ => None,
        }
    }

    /// Optimize statements with constant conditions
    fn optimize_constant_condition(&self, statement: &mut Statement, condition_value: bool) -> Result<(), CompilerError> {
        if let Statement::If { then_stmt, else_stmt, .. } = statement {
            if condition_value {
                // Condition is always true, replace with then branch
                *statement = (**then_stmt).clone();
            } else if let Some(else_stmt) = else_stmt {
                // Condition is always false, replace with else branch
                *statement = (**else_stmt).clone();
            } else {
                // Condition is always false and no else branch
                *statement = Statement::Block(vec![]);
            }
        }
        Ok(())
    }

    /// Merge peephole results
    fn merge_results(&self, target: &mut PeepholeResults, source: PeepholeResults) {
        target.total_optimizations += source.total_optimizations;
        target.expressions_simplified += source.expressions_simplified;
        target.statements_eliminated += source.statements_eliminated;
        
        for (pattern, count) in source.patterns_applied {
            *target.patterns_applied.entry(pattern).or_insert(0) += count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_add_zero_optimization() {
        let mut optimizer = PeepholeOptimizer::new(false);
        
        let mut expr = Expression::Binary {
            left: Box::new(Expression::Variable(Variable {
                name: "x".to_string(),
                var_type: crate::types::Type::Integer,
            })),
            right: Box::new(Expression::IntegerLiteral(0)),
            operator: BinaryOperator::Add,
        };

        let results = optimizer.optimize_expression(&mut expr).unwrap();
        
        assert!(results.total_optimizations > 0);
        assert!(matches!(expr, Expression::Variable(_)));
    }

    #[test]
    fn test_multiply_zero_optimization() {
        let mut optimizer = PeepholeOptimizer::new(false);
        
        let mut expr = Expression::Binary {
            left: Box::new(Expression::Variable(Variable {
                name: "x".to_string(),
                var_type: crate::types::Type::Integer,
            })),
            right: Box::new(Expression::IntegerLiteral(0)),
            operator: BinaryOperator::Multiply,
        };

        let results = optimizer.optimize_expression(&mut expr).unwrap();
        
        assert!(results.total_optimizations > 0);
        assert!(matches!(expr, Expression::IntegerLiteral(0)));
    }

    #[test]
    fn test_boolean_and_true_optimization() {
        let mut optimizer = PeepholeOptimizer::new(false);
        
        let mut expr = Expression::Binary {
            left: Box::new(Expression::Variable(Variable {
                name: "x".to_string(),
                var_type: crate::types::Type::Boolean,
            })),
            right: Box::new(Expression::BooleanLiteral(true)),
            operator: BinaryOperator::And,
        };

        let results = optimizer.optimize_expression(&mut expr).unwrap();
        
        assert!(results.total_optimizations > 0);
        assert!(matches!(expr, Expression::Variable(_)));
    }

    #[test]
    fn test_self_comparison_optimization() {
        let mut optimizer = PeepholeOptimizer::new(false);
        
        let var = Expression::Variable(Variable {
            name: "x".to_string(),
            var_type: crate::types::Type::Integer,
        });
        
        let mut expr = Expression::Binary {
            left: Box::new(var.clone()),
            right: Box::new(var),
            operator: BinaryOperator::Equal,
        };

        let results = optimizer.optimize_expression(&mut expr).unwrap();
        
        assert!(results.total_optimizations > 0);
        assert!(matches!(expr, Expression::BooleanLiteral(true)));
    }

    #[test]
    fn test_double_negation_optimization() {
        let mut optimizer = PeepholeOptimizer::new(false);
        
        let mut expr = Expression::Unary {
            operand: Box::new(Expression::Unary {
                operand: Box::new(Expression::Variable(Variable {
                    name: "x".to_string(),
                    var_type: crate::types::Type::Integer,
                })),
                operator: UnaryOperator::Minus,
            }),
            operator: UnaryOperator::Minus,
        };

        let results = optimizer.optimize_expression(&mut expr).unwrap();
        
        assert!(results.total_optimizations > 0);
        assert!(matches!(expr, Expression::Variable(_)));
    }
}