use crate::error::CompilerError;
use crate::ast::{Expression, Statement, Function, Program, BinaryOperator, UnaryOperator};
use std::collections::HashMap;

/// Constant folding optimizer
/// 
/// Evaluates constant expressions at compile time:
/// - Arithmetic operations (2 + 3 -> 5)
/// - Boolean operations (true && false -> false)  
/// - String operations ("hello" + " world" -> "hello world")
/// - Conditional expressions with constant conditions
/// - Array indexing with constant indices
pub struct ConstantFolder {
    debug: bool,
    folded_constants: HashMap<String, Expression>,
}

/// Results from constant folding
#[derive(Debug, Default)]
pub struct CFResults {
    pub constants_folded: usize,
    pub expressions_simplified: usize,
    pub branches_eliminated: usize,
}

impl ConstantFolder {
    pub fn new(debug: bool) -> Self {
        Self { 
            debug,
            folded_constants: HashMap::new(),
        }
    }

    /// Fold constants in the entire program
    pub fn fold(&mut self, program: &mut Program) -> Result<CFResults, CompilerError> {
        let mut results = CFResults::default();

        for function in &mut program.functions {
            let func_results = self.fold_in_function(function)?;
            results.constants_folded += func_results.constants_folded;
            results.expressions_simplified += func_results.expressions_simplified;
            results.branches_eliminated += func_results.branches_eliminated;
        }

        if self.debug {
            println!("CF Results: {} constants folded, {} expressions simplified, {} branches eliminated",
                     results.constants_folded, results.expressions_simplified, results.branches_eliminated);
        }

        Ok(results)
    }

    /// Fold constants within a function
    fn fold_in_function(&mut self, function: &mut Function) -> Result<CFResults, CompilerError> {
        let mut results = CFResults::default();

        for statement in &mut function.body {
            let stmt_results = self.fold_in_statement(statement)?;
            results.constants_folded += stmt_results.constants_folded;
            results.expressions_simplified += stmt_results.expressions_simplified;
            results.branches_eliminated += stmt_results.branches_eliminated;
        }

        Ok(results)
    }

    /// Fold constants in a statement
    fn fold_in_statement(&mut self, statement: &mut Statement) -> Result<CFResults, CompilerError> {
        let mut results = CFResults::default();

        match statement {
            Statement::Expression(expr) => {
                if self.fold_expression(expr)? {
                    results.constants_folded += 1;
                }
            }
            Statement::Variable { initializer: Some(expr), name, .. } => {
                if self.fold_expression(expr)? {
                    results.constants_folded += 1;
                    // Store constant value for later use
                    if self.is_constant_expression(expr) {
                        self.folded_constants.insert(name.clone(), expr.clone());
                    }
                }
            }
            Statement::Assignment { target, value } => {
                if self.fold_expression(value)? {
                    results.constants_folded += 1;
                }
                if self.fold_expression(target)? {
                    results.expressions_simplified += 1;
                }
            }
            Statement::If { condition, then_stmt, else_stmt } => {
                if self.fold_expression(condition)? {
                    results.constants_folded += 1;
                }

                // Check for constant condition branches
                if self.is_constant_expression(condition) {
                    if let Some(const_value) = self.evaluate_boolean_expression(condition) {
                        if const_value {
                            // Condition is always true, replace with then branch
                            if self.debug {
                                println!("CF: Eliminating false branch (condition always true)");
                            }
                            let then_results = self.fold_in_statement(then_stmt)?;
                            results.branches_eliminated += 1;
                            results.constants_folded += then_results.constants_folded;
                            results.expressions_simplified += then_results.expressions_simplified;
                        } else if let Some(else_stmt) = else_stmt {
                            // Condition is always false, replace with else branch
                            if self.debug {
                                println!("CF: Eliminating true branch (condition always false)");
                            }
                            let else_results = self.fold_in_statement(else_stmt)?;
                            results.branches_eliminated += 1;
                            results.constants_folded += else_results.constants_folded;
                            results.expressions_simplified += else_results.expressions_simplified;
                        } else {
                            // Condition is always false and no else branch, remove if statement
                            results.branches_eliminated += 1;
                        }
                    }
                } else {
                    // Process branches normally
                    let then_results = self.fold_in_statement(then_stmt)?;
                    results.constants_folded += then_results.constants_folded;
                    results.expressions_simplified += then_results.expressions_simplified;
                    results.branches_eliminated += then_results.branches_eliminated;

                    if let Some(else_stmt) = else_stmt {
                        let else_results = self.fold_in_statement(else_stmt)?;
                        results.constants_folded += else_results.constants_folded;
                        results.expressions_simplified += else_results.expressions_simplified;
                        results.branches_eliminated += else_results.branches_eliminated;
                    }
                }
            }
            Statement::While { condition, body } => {
                if self.fold_expression(condition)? {
                    results.constants_folded += 1;
                }

                // Check for constant false condition (infinite loop detection)
                if self.is_constant_expression(condition) {
                    if let Some(const_value) = self.evaluate_boolean_expression(condition) {
                        if !const_value && self.debug {
                            println!("CF: Warning - while loop with constant false condition");
                        }
                    }
                }

                let body_results = self.fold_in_statement(body)?;
                results.constants_folded += body_results.constants_folded;
                results.expressions_simplified += body_results.expressions_simplified;
                results.branches_eliminated += body_results.branches_eliminated;
            }
            Statement::For { init, condition, update, body } => {
                if let Some(init) = init {
                    let init_results = self.fold_in_statement(init)?;
                    results.constants_folded += init_results.constants_folded;
                    results.expressions_simplified += init_results.expressions_simplified;
                }
                if let Some(condition) = condition {
                    if self.fold_expression(condition)? {
                        results.constants_folded += 1;
                    }
                }
                if let Some(update) = update {
                    if self.fold_expression(update)? {
                        results.constants_folded += 1;
                    }
                }

                let body_results = self.fold_in_statement(body)?;
                results.constants_folded += body_results.constants_folded;
                results.expressions_simplified += body_results.expressions_simplified;
                results.branches_eliminated += body_results.branches_eliminated;
            }
            Statement::Return { value: Some(expr) } => {
                if self.fold_expression(expr)? {
                    results.constants_folded += 1;
                }
            }
            Statement::Block(statements) => {
                for stmt in statements {
                    let stmt_results = self.fold_in_statement(stmt)?;
                    results.constants_folded += stmt_results.constants_folded;
                    results.expressions_simplified += stmt_results.expressions_simplified;
                    results.branches_eliminated += stmt_results.branches_eliminated;
                }
            }
            _ => {}
        }

        Ok(results)
    }

    /// Fold constants in an expression, returns true if expression was modified
    fn fold_expression(&mut self, expr: &mut Expression) -> Result<bool, CompilerError> {
        let mut modified = false;

        match expr {
            Expression::Binary { left, right, operator } => {
                // First, fold operands
                if self.fold_expression(left)? {
                    modified = true;
                }
                if self.fold_expression(right)? {
                    modified = true;
                }

                // Then try to fold the binary operation
                if let Some(folded) = self.fold_binary_operation(left, right, operator)? {
                    if self.debug {
                        println!("CF: Folding binary operation: {:?} {} {:?} -> {:?}", 
                                left, self.binary_op_to_string(operator), right, folded);
                    }
                    *expr = folded;
                    modified = true;
                }
            }
            Expression::Unary { operand, operator } => {
                if self.fold_expression(operand)? {
                    modified = true;
                }

                if let Some(folded) = self.fold_unary_operation(operand, operator)? {
                    if self.debug {
                        println!("CF: Folding unary operation: {} {:?} -> {:?}", 
                                self.unary_op_to_string(operator), operand, folded);
                    }
                    *expr = folded;
                    modified = true;
                }
            }
            Expression::Conditional { condition, then_expr, else_expr } => {
                if self.fold_expression(condition)? {
                    modified = true;
                }
                if self.fold_expression(then_expr)? {
                    modified = true;
                }
                if self.fold_expression(else_expr)? {
                    modified = true;
                }

                // Fold conditional with constant condition
                if self.is_constant_expression(condition) {
                    if let Some(const_value) = self.evaluate_boolean_expression(condition) {
                        if const_value {
                            *expr = (**then_expr).clone();
                        } else {
                            *expr = (**else_expr).clone();
                        }
                        modified = true;
                    }
                }
            }
            Expression::Array(elements) => {
                for element in elements {
                    if self.fold_expression(element)? {
                        modified = true;
                    }
                }
            }
            Expression::Index { array, index } => {
                if self.fold_expression(array)? {
                    modified = true;
                }
                if self.fold_expression(index)? {
                    modified = true;
                }

                // Fold constant array indexing
                if let Some(folded) = self.fold_array_index(array, index)? {
                    *expr = folded;
                    modified = true;
                }
            }
            Expression::Call { function, arguments } => {
                if self.fold_expression(function)? {
                    modified = true;
                }
                for arg in arguments {
                    if self.fold_expression(arg)? {
                        modified = true;
                    }
                }
            }
            Expression::MethodCall { object, arguments, .. } => {
                if self.fold_expression(object)? {
                    modified = true;
                }
                for arg in arguments {
                    if self.fold_expression(arg)? {
                        modified = true;
                    }
                }
            }
            Expression::PropertyAccess { object, .. } => {
                if self.fold_expression(object)? {
                    modified = true;
                }
            }
            _ => {}
        }

        Ok(modified)
    }

    /// Fold a binary operation if both operands are constants
    fn fold_binary_operation(&self, left: &Expression, right: &Expression, op: &BinaryOperator) 
        -> Result<Option<Expression>, CompilerError> {
        
        if !self.is_constant_expression(left) || !self.is_constant_expression(right) {
            return Ok(None);
        }

        match (left, right, op) {
            // Integer arithmetic
            (Expression::IntegerLiteral(a), Expression::IntegerLiteral(b), op) => {
                match op {
                    BinaryOperator::Add => Ok(Some(Expression::IntegerLiteral(a + b))),
                    BinaryOperator::Subtract => Ok(Some(Expression::IntegerLiteral(a - b))),
                    BinaryOperator::Multiply => Ok(Some(Expression::IntegerLiteral(a * b))),
                    BinaryOperator::Divide if *b != 0 => Ok(Some(Expression::NumberLiteral(*a as f64 / *b as f64))),
                    BinaryOperator::Modulo if *b != 0 => Ok(Some(Expression::IntegerLiteral(a % b))),
                    BinaryOperator::Equal => Ok(Some(Expression::BooleanLiteral(a == b))),
                    BinaryOperator::NotEqual => Ok(Some(Expression::BooleanLiteral(a != b))),
                    BinaryOperator::Less => Ok(Some(Expression::BooleanLiteral(a < b))),
                    BinaryOperator::LessEqual => Ok(Some(Expression::BooleanLiteral(a <= b))),
                    BinaryOperator::Greater => Ok(Some(Expression::BooleanLiteral(a > b))),
                    BinaryOperator::GreaterEqual => Ok(Some(Expression::BooleanLiteral(a >= b))),
                    _ => Ok(None),
                }
            }
            // Number arithmetic
            (Expression::NumberLiteral(a), Expression::NumberLiteral(b), op) => {
                match op {
                    BinaryOperator::Add => Ok(Some(Expression::NumberLiteral(a + b))),
                    BinaryOperator::Subtract => Ok(Some(Expression::NumberLiteral(a - b))),
                    BinaryOperator::Multiply => Ok(Some(Expression::NumberLiteral(a * b))),
                    BinaryOperator::Divide if *b != 0.0 => Ok(Some(Expression::NumberLiteral(a / b))),
                    BinaryOperator::Modulo if *b != 0.0 => Ok(Some(Expression::NumberLiteral(a % b))),
                    BinaryOperator::Equal => Ok(Some(Expression::BooleanLiteral((a - b).abs() < f64::EPSILON))),
                    BinaryOperator::NotEqual => Ok(Some(Expression::BooleanLiteral((a - b).abs() >= f64::EPSILON))),
                    BinaryOperator::Less => Ok(Some(Expression::BooleanLiteral(a < b))),
                    BinaryOperator::LessEqual => Ok(Some(Expression::BooleanLiteral(a <= b))),
                    BinaryOperator::Greater => Ok(Some(Expression::BooleanLiteral(a > b))),
                    BinaryOperator::GreaterEqual => Ok(Some(Expression::BooleanLiteral(a >= b))),
                    _ => Ok(None),
                }
            }
            // Mixed integer/number arithmetic
            (Expression::IntegerLiteral(a), Expression::NumberLiteral(b), op) => {
                let a_f = *a as f64;
                match op {
                    BinaryOperator::Add => Ok(Some(Expression::NumberLiteral(a_f + b))),
                    BinaryOperator::Subtract => Ok(Some(Expression::NumberLiteral(a_f - b))),
                    BinaryOperator::Multiply => Ok(Some(Expression::NumberLiteral(a_f * b))),
                    BinaryOperator::Divide if *b != 0.0 => Ok(Some(Expression::NumberLiteral(a_f / b))),
                    BinaryOperator::Equal => Ok(Some(Expression::BooleanLiteral((a_f - b).abs() < f64::EPSILON))),
                    BinaryOperator::NotEqual => Ok(Some(Expression::BooleanLiteral((a_f - b).abs() >= f64::EPSILON))),
                    BinaryOperator::Less => Ok(Some(Expression::BooleanLiteral(a_f < *b))),
                    BinaryOperator::LessEqual => Ok(Some(Expression::BooleanLiteral(a_f <= *b))),
                    BinaryOperator::Greater => Ok(Some(Expression::BooleanLiteral(a_f > *b))),
                    BinaryOperator::GreaterEqual => Ok(Some(Expression::BooleanLiteral(a_f >= *b))),
                    _ => Ok(None),
                }
            }
            (Expression::NumberLiteral(a), Expression::IntegerLiteral(b), op) => {
                let b_f = *b as f64;
                match op {
                    BinaryOperator::Add => Ok(Some(Expression::NumberLiteral(a + b_f))),
                    BinaryOperator::Subtract => Ok(Some(Expression::NumberLiteral(a - b_f))),
                    BinaryOperator::Multiply => Ok(Some(Expression::NumberLiteral(a * b_f))),
                    BinaryOperator::Divide if *b != 0 => Ok(Some(Expression::NumberLiteral(a / b_f))),
                    BinaryOperator::Equal => Ok(Some(Expression::BooleanLiteral((a - b_f).abs() < f64::EPSILON))),
                    BinaryOperator::NotEqual => Ok(Some(Expression::BooleanLiteral((a - b_f).abs() >= f64::EPSILON))),
                    BinaryOperator::Less => Ok(Some(Expression::BooleanLiteral(*a < b_f))),
                    BinaryOperator::LessEqual => Ok(Some(Expression::BooleanLiteral(*a <= b_f))),
                    BinaryOperator::Greater => Ok(Some(Expression::BooleanLiteral(*a > b_f))),
                    BinaryOperator::GreaterEqual => Ok(Some(Expression::BooleanLiteral(*a >= b_f))),
                    _ => Ok(None),
                }
            }
            // Boolean operations
            (Expression::BooleanLiteral(a), Expression::BooleanLiteral(b), op) => {
                match op {
                    BinaryOperator::And => Ok(Some(Expression::BooleanLiteral(*a && *b))),
                    BinaryOperator::Or => Ok(Some(Expression::BooleanLiteral(*a || *b))),
                    BinaryOperator::Equal => Ok(Some(Expression::BooleanLiteral(a == b))),
                    BinaryOperator::NotEqual => Ok(Some(Expression::BooleanLiteral(a != b))),
                    _ => Ok(None),
                }
            }
            // String operations
            (Expression::StringLiteral(a), Expression::StringLiteral(b), BinaryOperator::Add) => {
                Ok(Some(Expression::StringLiteral(format!("{}{}", a, b))))
            }
            (Expression::StringLiteral(a), Expression::StringLiteral(b), op) => {
                match op {
                    BinaryOperator::Equal => Ok(Some(Expression::BooleanLiteral(a == b))),
                    BinaryOperator::NotEqual => Ok(Some(Expression::BooleanLiteral(a != b))),
                    _ => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    /// Fold a unary operation if operand is constant
    fn fold_unary_operation(&self, operand: &Expression, op: &UnaryOperator) 
        -> Result<Option<Expression>, CompilerError> {
        
        if !self.is_constant_expression(operand) {
            return Ok(None);
        }

        match (operand, op) {
            (Expression::IntegerLiteral(value), UnaryOperator::Minus) => {
                Ok(Some(Expression::IntegerLiteral(-value)))
            }
            (Expression::NumberLiteral(value), UnaryOperator::Minus) => {
                Ok(Some(Expression::NumberLiteral(-value)))
            }
            (Expression::BooleanLiteral(value), UnaryOperator::Not) => {
                Ok(Some(Expression::BooleanLiteral(!value)))
            }
            _ => Ok(None),
        }
    }

    /// Fold constant array indexing
    fn fold_array_index(&self, array: &Expression, index: &Expression) 
        -> Result<Option<Expression>, CompilerError> {
        
        if let (Expression::Array(elements), Expression::IntegerLiteral(idx)) = (array, index) {
            if *idx >= 0 && (*idx as usize) < elements.len() {
                return Ok(Some(elements[*idx as usize].clone()));
            }
        }
        
        Ok(None)
    }

    /// Check if an expression is a constant
    fn is_constant_expression(&self, expr: &Expression) -> bool {
        match expr {
            Expression::IntegerLiteral(_) |
            Expression::NumberLiteral(_) |
            Expression::StringLiteral(_) |
            Expression::BooleanLiteral(_) => true,
            Expression::Array(elements) => {
                elements.iter().all(|e| self.is_constant_expression(e))
            }
            _ => false,
        }
    }

    /// Evaluate a boolean expression if it's constant
    fn evaluate_boolean_expression(&self, expr: &Expression) -> Option<bool> {
        match expr {
            Expression::BooleanLiteral(value) => Some(*value),
            Expression::IntegerLiteral(value) => Some(*value != 0),
            Expression::NumberLiteral(value) => Some(*value != 0.0),
            Expression::StringLiteral(value) => Some(!value.is_empty()),
            _ => None,
        }
    }

    fn binary_op_to_string(&self, op: &BinaryOperator) -> &str {
        match op {
            BinaryOperator::Add => "+",
            BinaryOperator::Subtract => "-",
            BinaryOperator::Multiply => "*",
            BinaryOperator::Divide => "/",
            BinaryOperator::Modulo => "%",
            BinaryOperator::Equal => "==",
            BinaryOperator::NotEqual => "!=",
            BinaryOperator::Less => "<",
            BinaryOperator::LessEqual => "<=",
            BinaryOperator::Greater => ">",
            BinaryOperator::GreaterEqual => ">=",
            BinaryOperator::And => "&&",
            BinaryOperator::Or => "||",
        }
    }

    fn unary_op_to_string(&self, op: &UnaryOperator) -> &str {
        match op {
            UnaryOperator::Minus => "-",
            UnaryOperator::Not => "!",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_integer_arithmetic_folding() {
        let mut folder = ConstantFolder::new(false);
        
        let mut expr = Expression::Binary {
            left: Box::new(Expression::IntegerLiteral(5)),
            right: Box::new(Expression::IntegerLiteral(3)),
            operator: BinaryOperator::Add,
        };

        let result = folder.fold_expression(&mut expr).unwrap();
        assert!(result);
        assert!(matches!(expr, Expression::IntegerLiteral(8)));
    }

    #[test]
    fn test_boolean_operation_folding() {
        let mut folder = ConstantFolder::new(false);
        
        let mut expr = Expression::Binary {
            left: Box::new(Expression::BooleanLiteral(true)),
            right: Box::new(Expression::BooleanLiteral(false)),
            operator: BinaryOperator::And,
        };

        let result = folder.fold_expression(&mut expr).unwrap();
        assert!(result);
        assert!(matches!(expr, Expression::BooleanLiteral(false)));
    }

    #[test]
    fn test_string_concatenation_folding() {
        let mut folder = ConstantFolder::new(false);
        
        let mut expr = Expression::Binary {
            left: Box::new(Expression::StringLiteral("hello".to_string())),
            right: Box::new(Expression::StringLiteral(" world".to_string())),
            operator: BinaryOperator::Add,
        };

        let result = folder.fold_expression(&mut expr).unwrap();
        assert!(result);
        assert!(matches!(expr, Expression::StringLiteral(ref s) if s == "hello world"));
    }

    #[test]
    fn test_conditional_with_constant_condition() {
        let mut folder = ConstantFolder::new(false);
        
        let mut expr = Expression::Conditional {
            condition: Box::new(Expression::BooleanLiteral(true)),
            then_expr: Box::new(Expression::StringLiteral("then".to_string())),
            else_expr: Box::new(Expression::StringLiteral("else".to_string())),
        };

        let result = folder.fold_expression(&mut expr).unwrap();
        assert!(result);
        assert!(matches!(expr, Expression::StringLiteral(ref s) if s == "then"));
    }
}